use super::parser::{flatten_symbol_infos, slice_source};
use super::ranking::{prune_to_budget, rank_symbols, RankingContext};
use super::types::{make_symbol_id, parse_symbol_id, RankedContextResult, SymbolInfo, SymbolKind};
use super::SymbolIndex;
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
        // Compute file scores and import-graph proximity inside a block so the
        // ReadDb guard is dropped before calling find_symbol_cached /
        // get_symbols_overview_cached, which also acquire the reader lock.
        // Holding both causes a deadlock when in_memory=true (same Mutex).
        let (top_files, importer_files) = {
            let db = self.reader()?;
            let all_paths = db.all_file_paths()?;

            let query_lower = query.to_ascii_lowercase();
            let query_tokens: Vec<&str> = query_lower
                .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
                .filter(|t| t.len() >= 2)
                .collect();

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

        let db_rows = db.find_symbols_by_name(name, file_path, exact_match, max_matches)?;
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
            let all_paths = db.all_file_paths()?;
            let mut symbols = Vec::new();
            for rel in all_paths {
                if !rel.starts_with(&prefix) && prefix != "." && prefix != "" {
                    continue;
                }
                let file_row = match db.get_file(&rel)? {
                    Some(row) => row,
                    None => continue,
                };
                let file_symbols = db.get_file_symbols(file_row.id)?;
                if !file_symbols.is_empty() {
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
                        name_path: rel,
                        id,
                        body: None,
                        children: file_symbols
                            .into_iter()
                            .map(|row| {
                                let kind = SymbolKind::from_str_label(&row.kind);
                                let sid = make_symbol_id("", &kind, &row.name_path);
                                SymbolInfo {
                                    name: row.name,
                                    kind,
                                    file_path: String::new(),
                                    line: row.line as usize,
                                    column: row.column_num as usize,
                                    signature: row.signature,
                                    name_path: row.name_path,
                                    id: sid,
                                    body: None,
                                    children: Vec::new(),
                                    start_byte: row.start_byte as u32,
                                    end_byte: row.end_byte as u32,
                                }
                            })
                            .collect(),
                        start_byte: 0,
                        end_byte: 0,
                    });
                }
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
                SymbolInfo {
                    name: row.name,
                    kind,
                    file_path: relative.clone(),
                    line: row.line as usize,
                    column: row.column_num as usize,
                    signature: row.signature,
                    name_path: row.name_path,
                    id,
                    body: None,
                    children: Vec::new(),
                    start_byte: row.start_byte as u32,
                    end_byte: row.end_byte as u32,
                }
            })
            .collect())
    }

    /// Ranked context from DB without lazy indexing.
    /// If `graph_cache` is provided, PageRank scores boost symbols in highly-imported files.
    pub fn get_ranked_context_cached(
        &self,
        query: &str,
        path: Option<&str>,
        max_tokens: usize,
        include_body: bool,
        depth: usize,
        graph_cache: Option<&crate::import_graph::GraphCache>,
    ) -> Result<RankedContextResult> {
        let all_symbols = if let Some(path) = path {
            self.get_symbols_overview_cached(path, depth)?
        } else {
            self.select_solve_symbols_cached(query, depth)?
        };

        let ranking_ctx = match graph_cache {
            Some(gc) => RankingContext::with_pagerank(gc.file_pagerank_scores(&self.project)),
            None => RankingContext::text_only(),
        };

        let flat_symbols: Vec<SymbolInfo> = all_symbols
            .into_iter()
            .flat_map(flatten_symbol_infos)
            .collect();

        let scored = rank_symbols(query, flat_symbols, &ranking_ctx);

        let (selected, chars_used) =
            prune_to_budget(scored, max_tokens, include_body, self.project.as_path());

        Ok(RankedContextResult {
            query: query.to_owned(),
            count: selected.len(),
            symbols: selected,
            token_budget: max_tokens,
            chars_used,
        })
    }

    /// Helper: convert DB rows to SymbolInfo with optional body.
    pub(super) fn rows_to_symbol_infos(
        project: &ProjectRoot,
        db: &IndexDb,
        rows: Vec<crate::db::SymbolRow>,
        include_body: bool,
    ) -> Result<Vec<SymbolInfo>> {
        let mut results = Vec::new();
        for row in rows {
            let rel_path = db.get_file_path(row.file_id)?.unwrap_or_default();
            let body = if include_body {
                let abs = project.as_path().join(&rel_path);
                fs::read_to_string(&abs)
                    .ok()
                    .map(|source| slice_source(&source, row.start_byte as u32, row.end_byte as u32))
            } else {
                None
            };
            let kind = SymbolKind::from_str_label(&row.kind);
            let id = make_symbol_id(&rel_path, &kind, &row.name_path);
            results.push(SymbolInfo {
                name: row.name,
                kind,
                file_path: rel_path,
                line: row.line as usize,
                column: row.column_num as usize,
                signature: row.signature,
                name_path: row.name_path,
                id,
                body,
                children: Vec::new(),
                start_byte: row.start_byte as u32,
                end_byte: row.end_byte as u32,
            });
        }
        Ok(results)
    }
}
