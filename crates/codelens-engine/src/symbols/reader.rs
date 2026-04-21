use super::SymbolIndex;
use super::parser::{extend_start_to_doc_comments, flatten_symbol_infos, slice_source};
use super::ranking::{self, RankingContext, prune_to_budget, rank_symbols};
use super::types::{
    RankedContextResult, SymbolInfo, SymbolKind, SymbolProvenance, make_symbol_id, parse_symbol_id,
};
use crate::db::IndexDb;
use crate::project::ProjectRoot;
use anyhow::Result;
use std::fs;

impl SymbolIndex {
    /// Hybrid candidate collection: fan-out to multiple retrieval paths,
    /// then merge and deduplicate. Returns a broad candidate pool for ranking.
    ///
    /// Retrieval paths:
    /// 1. File path token matching — top files whose path contains query tokens
    /// 2. Direct symbol name matching — exact/substring DB lookup
    /// 3. Import graph proximity — files that import/are imported by matched files
    pub(super) fn select_solve_symbols_cached(
        &self,
        query: &str,
        depth: usize,
    ) -> Result<Vec<SymbolInfo>> {
        let query_lower = query.to_ascii_lowercase();
        let query_tokens: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|t| t.len() >= 3)
            .collect();

        // Compute file scores and import-graph proximity inside a block so the
        // ReadDb guard is dropped before calling find_symbol_cached /
        // get_symbols_overview_cached, which also acquire the reader lock.
        // Holding both causes a deadlock when in_memory=true (same Mutex).
        let (top_files, importer_files) = {
            let db = self.reader()?;
            let all_paths = db.all_file_paths()?;

            let mut file_scores: Vec<(String, usize)> = all_paths
                .iter()
                .map(|path| {
                    let path_lower = path.to_ascii_lowercase();
                    let score = query_tokens
                        .iter()
                        .filter(|t| path_lower.contains(**t))
                        .count();
                    (path.clone(), score)
                })
                .collect();

            file_scores.sort_by(|a, b| b.1.cmp(&a.1));
            let top: Vec<String> = file_scores
                .iter()
                .filter(|(_, score)| *score > 0)
                .take(10)
                .map(|(path, _)| path.clone())
                .collect();

            // Path 3: import graph proximity
            let mut importers = Vec::new();
            if !top.is_empty() && top.len() <= 5 {
                for file_path in top.iter().take(3) {
                    if let Ok(imp) = db.get_importers(file_path) {
                        for importer_path in imp.into_iter().take(3) {
                            importers.push(importer_path);
                        }
                    }
                }
            }

            (top, importers)
            // db dropped here
        };

        let mut seen_ids = std::collections::HashSet::new();
        let mut all_symbols = Vec::new();

        // Path 1: collect symbols from path-matched files
        for file_path in &top_files {
            if let Ok(symbols) = self.get_symbols_overview_cached(file_path, depth) {
                for sym in symbols {
                    if seen_ids.insert(sym.id.clone()) {
                        all_symbols.push(sym);
                    }
                }
            }
        }

        // Path 2: direct symbol name matching
        if let Ok(direct) = self.find_symbol_cached(query, None, false, false, 50) {
            for sym in direct {
                if seen_ids.insert(sym.id.clone()) {
                    all_symbols.push(sym);
                }
            }
        }

        // Path 3: import graph proximity — related code via structural connection
        for importer_path in &importer_files {
            if let Ok(symbols) = self.get_symbols_overview_cached(importer_path, 1) {
                for sym in symbols {
                    if seen_ids.insert(sym.id.clone()) {
                        all_symbols.push(sym);
                    }
                }
            }
        }

        // Path 4: for multi-word queries, search individual tokens as symbol names
        if query_tokens.len() >= 2 {
            for token in &query_tokens {
                if let Ok(hits) = self.find_symbol_cached(token, None, false, false, 10) {
                    for sym in hits {
                        if seen_ids.insert(sym.id.clone()) {
                            all_symbols.push(sym);
                        }
                    }
                }
            }
        }

        // Fallback: if no candidates found, do a broad symbol search
        if all_symbols.is_empty() {
            return self.find_symbol_cached(query, None, false, false, 500);
        }

        Ok(all_symbols)
    }

    /// Query symbols from DB without lazy indexing. Returns empty if file not yet indexed.
    pub fn find_symbol_cached(
        &self,
        name: &str,
        file_path: Option<&str>,
        include_body: bool,
        exact_match: bool,
        max_matches: usize,
    ) -> Result<Vec<SymbolInfo>> {
        let db = self.reader()?;
        // Stable ID fast path
        if let Some((id_file, _id_kind, id_name_path)) = parse_symbol_id(name) {
            let leaf_name = id_name_path.rsplit('/').next().unwrap_or(id_name_path);
            let db_rows = db.find_symbols_by_name(leaf_name, Some(id_file), true, max_matches)?;
            return Self::rows_to_symbol_infos(&self.project, &db, db_rows, include_body);
        }

        // Resolve file_path (handles symlinks → canonical relative path)
        let resolved_fp = file_path.and_then(|fp| {
            self.project
                .resolve(fp)
                .ok()
                .map(|abs| self.project.to_relative(&abs))
        });
        let fp_ref = resolved_fp.as_deref().or(file_path);

        let db_rows = db.find_symbols_by_name(name, fp_ref, exact_match, max_matches)?;
        Self::rows_to_symbol_infos(&self.project, &db, db_rows, include_body)
    }

    /// Get symbols overview from DB without lazy indexing.
    pub fn get_symbols_overview_cached(
        &self,
        path: &str,
        _depth: usize,
    ) -> Result<Vec<SymbolInfo>> {
        let db = self.reader()?;
        let resolved = self.project.resolve(path)?;
        if resolved.is_dir() {
            let prefix = self.project.to_relative(&resolved);
            // Single JOIN query instead of N+1 (all_file_paths + get_file + get_file_symbols per file)
            let file_groups = db.get_symbols_for_directory(&prefix)?;
            let mut symbols = Vec::new();
            for (rel, file_symbols) in file_groups {
                if file_symbols.is_empty() {
                    continue;
                }
                let id = make_symbol_id(&rel, &SymbolKind::File, &rel);
                symbols.push(SymbolInfo {
                    name: rel.clone(),
                    kind: SymbolKind::File,
                    file_path: rel.clone(),
                    line: 0,
                    column: 0,
                    signature: format!(
                        "{} ({} symbols)",
                        std::path::Path::new(&rel)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&rel),
                        file_symbols.len()
                    ),
                    name_path: rel.clone(),
                    id,
                    provenance: SymbolProvenance::from_path(&rel),
                    body: None,
                    children: file_symbols
                        .into_iter()
                        .map(|row| {
                            let kind = SymbolKind::from_str_label(&row.kind);
                            let sid = make_symbol_id(&rel, &kind, &row.name_path);
                            let row_line = row.line as usize;
                            SymbolInfo {
                                name: row.name,
                                kind,
                                file_path: rel.clone(),
                                line: row_line,
                                column: row.column_num as usize,
                                signature: row.signature,
                                name_path: row.name_path,
                                id: sid,
                                provenance: SymbolProvenance::from_path(&rel),
                                body: None,
                                children: Vec::new(),
                                start_byte: row.start_byte as u32,
                                end_byte: row.end_byte as u32,
                                // DB rows pre-date the end_line column;
                                // fall back to `line` so P1-4 proximity
                                // scoring stays valid but assumes a
                                // single-line span.
                                end_line: if row.end_line > 0 {
                                    row.end_line as usize
                                } else {
                                    row_line
                                },
                            }
                        })
                        .collect(),
                    start_byte: 0,
                    end_byte: 0,
                    end_line: 0,
                });
            }
            return Ok(symbols);
        }

        // Single file
        let relative = self.project.to_relative(&resolved);
        let file_row = match db.get_file(&relative)? {
            Some(row) => row,
            None => return Ok(Vec::new()),
        };
        let db_symbols = db.get_file_symbols(file_row.id)?;
        Ok(db_symbols
            .into_iter()
            .map(|row| {
                let kind = SymbolKind::from_str_label(&row.kind);
                let id = make_symbol_id(&relative, &kind, &row.name_path);
                let row_line = row.line as usize;
                SymbolInfo {
                    name: row.name,
                    kind,
                    file_path: relative.clone(),
                    provenance: SymbolProvenance::from_path(&relative),
                    line: row_line,
                    column: row.column_num as usize,
                    signature: row.signature,
                    name_path: row.name_path,
                    id,
                    body: None,
                    children: Vec::new(),
                    start_byte: row.start_byte as u32,
                    end_byte: row.end_byte as u32,
                    end_line: if row.end_line > 0 {
                        row.end_line as usize
                    } else {
                        row_line
                    },
                }
            })
            .collect())
    }

    /// Ranked context from DB without lazy indexing.
    /// If `graph_cache` is provided, PageRank scores boost symbols in highly-imported files.
    /// If `semantic_scores` is non-empty, vector similarity is blended into ranking.
    #[allow(clippy::too_many_arguments)]
    pub fn get_ranked_context_cached(
        &self,
        query: &str,
        path: Option<&str>,
        max_tokens: usize,
        include_body: bool,
        depth: usize,
        graph_cache: Option<&crate::import_graph::GraphCache>,
        semantic_scores: std::collections::HashMap<String, f64>,
    ) -> Result<RankedContextResult> {
        self.get_ranked_context_cached_with_query_type(
            query,
            path,
            max_tokens,
            include_body,
            depth,
            graph_cache,
            semantic_scores,
            None,
        )
    }

    /// Like `get_ranked_context_cached` but accepts an optional query type
    /// (`"identifier"`, `"natural_language"`, `"short_phrase"`) to tune
    /// ranking weights per query category.
    #[allow(clippy::too_many_arguments)]
    pub fn get_ranked_context_cached_with_query_type(
        &self,
        query: &str,
        path: Option<&str>,
        max_tokens: usize,
        include_body: bool,
        depth: usize,
        graph_cache: Option<&crate::import_graph::GraphCache>,
        semantic_scores: std::collections::HashMap<String, f64>,
        query_type: Option<&str>,
    ) -> Result<RankedContextResult> {
        self.get_ranked_context_cached_with_lsp_boost(
            query,
            path,
            max_tokens,
            include_body,
            depth,
            graph_cache,
            semantic_scores,
            query_type,
            std::collections::HashMap::new(),
            None,
        )
    }

    /// Full form that additionally accepts per-file LSP reference line
    /// information for the P1-4 per-symbol boost.
    ///
    /// `lsp_boost_refs` maps project-relative file paths to the list of
    /// reference lines that a caller-side reference probe (LSP
    /// `textDocument/references`, unioned with the tree-sitter text
    /// search) returned for the query's target symbol. Inside
    /// `rank_symbols`, each candidate is boosted by how close its
    /// declaration line is to the nearest ref line at or below it —
    /// file-level uniform boost used to over-promote unrelated helpers
    /// in large ref'd files, per-symbol proximity keeps the lift
    /// targeted on plausible containers.
    ///
    /// `lsp_signal_weight` scales the final boost. An empty ref map OR
    /// a `None` weight keeps the blend byte-identical to the pre-P1-4
    /// pipeline.
    #[allow(clippy::too_many_arguments)]
    pub fn get_ranked_context_cached_with_lsp_boost(
        &self,
        query: &str,
        path: Option<&str>,
        max_tokens: usize,
        include_body: bool,
        depth: usize,
        graph_cache: Option<&crate::import_graph::GraphCache>,
        semantic_scores: std::collections::HashMap<String, f64>,
        query_type: Option<&str>,
        mut lsp_boost_refs: std::collections::HashMap<String, Vec<usize>>,
        lsp_signal_weight: Option<f64>,
    ) -> Result<RankedContextResult> {
        // `rank_symbols` uses a binary search on the ref-line list;
        // normalise here so the contract holds regardless of the
        // caller's input shape.
        for lines in lsp_boost_refs.values_mut() {
            lines.sort_unstable();
            lines.dedup();
        }

        let mut all_symbols = if let Some(path) = path {
            self.get_symbols_overview_cached(path, depth)?
        } else {
            self.select_solve_symbols_cached(query, depth)?
        };

        // P1-4: pull symbols from every LSP-flagged file into the
        // candidate pool. Without this step the downstream gate still
        // drops a rescued caller for the trivial reason that it never
        // entered the pool — `get_symbols_overview_cached(path)` is
        // scoped to one file. The LSP boost is only meaningful when
        // the probe actually extends the candidate surface across the
        // caller graph.
        if !lsp_boost_refs.is_empty() {
            let mut seen: std::collections::HashSet<String> =
                all_symbols.iter().map(|s| s.id.clone()).collect();
            for extra_path in lsp_boost_refs.keys() {
                if Some(extra_path.as_str()) == path {
                    continue;
                }
                if let Ok(extra_symbols) = self.get_symbols_overview_cached(extra_path, depth) {
                    for sym in extra_symbols {
                        if seen.insert(sym.id.clone()) {
                            all_symbols.push(sym);
                        }
                    }
                }
            }
        }

        let ranking_ctx = match graph_cache {
            Some(gc) => {
                let pagerank = gc.file_pagerank_scores(&self.project);
                if semantic_scores.is_empty() {
                    RankingContext::with_pagerank(pagerank)
                } else {
                    RankingContext::with_pagerank_and_semantic(query, pagerank, semantic_scores)
                }
            }
            None => {
                if semantic_scores.is_empty() {
                    RankingContext::text_only()
                } else {
                    RankingContext::with_pagerank_and_semantic(
                        query,
                        std::collections::HashMap::new(),
                        semantic_scores,
                    )
                }
            }
        };

        // Apply query-type-aware weights when specified.
        let ranking_ctx = if let Some(qt) = query_type {
            let mut ctx = ranking_ctx;
            ctx.weights = ranking::weights_for_query_type(qt);
            ctx
        } else {
            ranking_ctx
        };

        // P1-4 caller wiring: fold the ref map + weight into the ctx.
        // Empty map AND `None` weight keeps pre-P1-4 behaviour
        // byte-for-byte because both the gate rescue and the blend
        // gate on `lsp_signal_weight > 0.0`.
        let ranking_ctx = if lsp_boost_refs.is_empty() && lsp_signal_weight.is_none() {
            ranking_ctx
        } else {
            let mut ctx = ranking_ctx;
            ctx.lsp_boost_refs = lsp_boost_refs;
            if let Some(w) = lsp_signal_weight {
                ctx.weights.lsp_signal = w;
            }
            ctx
        };

        let flat_symbols: Vec<SymbolInfo> = all_symbols
            .into_iter()
            .flat_map(flatten_symbol_infos)
            .collect();

        let scored = rank_symbols(query, flat_symbols, &ranking_ctx);

        let (selected, chars_used, pruned_count, last_kept_score) =
            prune_to_budget(scored, max_tokens, include_body, self.project.as_path());

        Ok(RankedContextResult {
            query: query.to_owned(),
            count: selected.len(),
            symbols: selected,
            token_budget: max_tokens,
            chars_used,
            pruned_count,
            last_kept_score,
        })
    }

    /// Helper: convert DB rows to SymbolInfo with optional body.
    /// Uses a file_id→path cache to avoid N+1 `get_file_path` queries.
    pub(super) fn rows_to_symbol_infos(
        project: &ProjectRoot,
        db: &IndexDb,
        rows: Vec<crate::db::SymbolRow>,
        include_body: bool,
    ) -> Result<Vec<SymbolInfo>> {
        let mut results = Vec::new();
        let mut path_cache: std::collections::HashMap<i64, String> =
            std::collections::HashMap::new();
        for row in rows {
            let rel_path = match path_cache.get(&row.file_id) {
                Some(p) => p.clone(),
                None => {
                    let p = db.get_file_path(row.file_id)?.unwrap_or_default();
                    path_cache.insert(row.file_id, p.clone());
                    p
                }
            };
            let body = if include_body {
                let abs = project.as_path().join(&rel_path);
                fs::read_to_string(&abs).ok().map(|source| {
                    // Serena-parity: include any immediately-preceding
                    // doc-comment block so the harness gets the intent
                    // alongside the signature, without a follow-up Read.
                    let extended_start =
                        extend_start_to_doc_comments(&source, row.start_byte as u32);
                    slice_source(&source, extended_start, row.end_byte as u32)
                })
            } else {
                None
            };
            let kind = SymbolKind::from_str_label(&row.kind);
            let id = make_symbol_id(&rel_path, &kind, &row.name_path);
            let row_line = row.line as usize;
            results.push(SymbolInfo {
                name: row.name,
                kind,
                provenance: SymbolProvenance::from_path(&rel_path),
                file_path: rel_path,
                line: row_line,
                column: row.column_num as usize,
                signature: row.signature,
                name_path: row.name_path,
                id,
                body,
                children: Vec::new(),
                start_byte: row.start_byte as u32,
                end_byte: row.end_byte as u32,
                end_line: if row.end_line > 0 {
                    row.end_line as usize
                } else {
                    row_line
                },
            });
        }
        Ok(results)
    }
}
