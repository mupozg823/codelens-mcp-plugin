use super::parser::{
    extend_start_to_doc_comments, flatten_symbol_infos, slice_source, to_symbol_info,
};
use super::ranking::prune_to_budget;
use super::scoring::score_symbol;
use super::types::{
    IndexStats, RankedContextResult, SymbolInfo, SymbolKind, SymbolProvenance, make_symbol_id,
    parse_symbol_id,
};
use super::{ReadDb, collect_candidate_files, file_modified_ms};
use crate::db::{self, IndexDb, content_hash, index_db_path};
use crate::project::ProjectRoot;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

/// SQLite-backed symbol index for a project.
///
/// Architecture: writer `Mutex<IndexDb>` for mutations + per-query read-only
/// connections for `_cached` methods. This makes `SymbolIndex: Send + Sync`,
/// enabling `Arc<SymbolIndex>` without an external Mutex.
pub struct SymbolIndex {
    pub(super) project: ProjectRoot,
    pub(super) db_path: PathBuf,
    pub(super) writer: std::sync::Mutex<IndexDb>,
    /// In-memory mode flag (tests) — when true, _cached reads use the writer.
    pub(super) in_memory: bool,
}

impl SymbolIndex {
    pub fn new(project: ProjectRoot) -> Self {
        let db_path = index_db_path(project.as_path());
        let db = IndexDb::open(&db_path).unwrap_or_else(|error| {
            tracing::warn!(
                path = %db_path.display(),
                error = %error,
                "failed to open DB, falling back to in-memory"
            );
            IndexDb::open_memory().unwrap()
        });
        let in_memory = !db_path.is_file();
        let mut idx = Self {
            project,
            db_path,
            writer: std::sync::Mutex::new(db),
            in_memory,
        };
        if idx.writer().file_count().unwrap_or(0) == 0 {
            let _ = idx.migrate_from_json();
        }
        idx
    }

    pub(super) fn writer(&self) -> std::sync::MutexGuard<'_, IndexDb> {
        self.writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn reader(&self) -> Result<ReadDb<'_>> {
        if self.in_memory {
            return Ok(ReadDb::Writer(self.writer()));
        }
        match IndexDb::open_readonly(&self.db_path)? {
            Some(db) => Ok(ReadDb::Owned(db)),
            None => Ok(ReadDb::Writer(self.writer())),
        }
    }

    /// Create an in-memory index (for tests and benchmarks — no disk persistence).
    pub fn new_memory(project: ProjectRoot) -> Self {
        let db = IndexDb::open_memory().unwrap();
        Self {
            db_path: PathBuf::new(),
            project,
            writer: std::sync::Mutex::new(db),
            in_memory: true,
        }
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let db = self.reader()?;
        let supported_files = collect_candidate_files(self.project.as_path())?;
        let indexed_files = db.file_count()?;
        let indexed_paths = db.all_file_paths()?;

        let mut stale = 0usize;
        for rel in &indexed_paths {
            let path = self.project.as_path().join(rel);
            if !path.is_file() {
                stale += 1;
                continue;
            }
            let content = match fs::read(&path) {
                Ok(content) => content,
                Err(_) => {
                    stale += 1;
                    continue;
                }
            };
            let hash = content_hash(&content);
            let mtime = file_modified_ms(&path).unwrap_or(0) as i64;
            if db.get_fresh_file(rel, mtime, &hash)?.is_none() {
                stale += 1;
            }
        }

        Ok(IndexStats {
            indexed_files,
            supported_files: supported_files.len(),
            stale_files: stale,
        })
    }

    fn select_solve_symbols(&self, query: &str, depth: usize) -> Result<Vec<SymbolInfo>> {
        let fts_file_boost: std::collections::HashSet<String> = {
            let query_lower = query.to_ascii_lowercase();
            let tokens: Vec<&str> = query_lower
                .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
                .filter(|token| token.len() >= 3)
                .collect();
            let mut boost_files = std::collections::HashSet::new();
            if let Ok(hits) = self.find_symbol(query, None, false, false, 15) {
                for symbol in hits {
                    boost_files.insert(symbol.file_path);
                }
            }
            for token in &tokens {
                if let Ok(hits) = self.find_symbol(token, None, false, false, 10) {
                    for symbol in hits {
                        boost_files.insert(symbol.file_path);
                    }
                }
            }
            boost_files
        };

        let (top_files, importer_files): (Vec<String>, Vec<String>) = {
            let db = self.reader()?;
            let all_paths = db.all_file_paths()?;

            let query_lower = query.to_ascii_lowercase();
            let query_tokens: Vec<&str> = query_lower
                .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
                .filter(|token| token.len() >= 3)
                .collect();

            let mut file_scores: Vec<(String, usize)> = all_paths
                .into_iter()
                .map(|path| {
                    let path_lower = path.to_ascii_lowercase();
                    let mut score = query_tokens
                        .iter()
                        .filter(|token| path_lower.contains(**token))
                        .count();
                    if fts_file_boost.contains(&path) {
                        score += 2;
                    }
                    (path, score)
                })
                .collect();

            file_scores.sort_by(|left, right| right.1.cmp(&left.1));
            let top: Vec<String> = file_scores
                .into_iter()
                .filter(|(_, score)| *score > 0)
                .take(10)
                .map(|(path, _)| path)
                .collect();

            let mut importers = Vec::new();
            if !top.is_empty() && top.len() <= 5 {
                for file_path in top.iter().take(3) {
                    if let Ok(importer_paths) = db.get_importers(file_path) {
                        for importer_path in importer_paths.into_iter().take(3) {
                            importers.push(importer_path);
                        }
                    }
                }
            }

            (top, importers)
        };

        if top_files.is_empty() {
            return self.find_symbol(query, None, false, false, 500);
        }

        let mut all_symbols = Vec::new();
        for file_path in &top_files {
            if let Ok(symbols) = self.get_symbols_overview_cached(file_path, depth) {
                all_symbols.extend(symbols);
            }
        }

        for importer_path in &importer_files {
            if let Ok(symbols) = self.get_symbols_overview_cached(importer_path, 1) {
                all_symbols.extend(symbols);
            }
        }

        let mut seen_ids: std::collections::HashSet<String> =
            all_symbols.iter().map(|symbol| symbol.id.clone()).collect();

        if let Ok(direct) = self.find_symbol(query, None, false, false, 50) {
            for symbol in direct {
                if seen_ids.insert(symbol.id.clone()) {
                    all_symbols.push(symbol);
                }
            }
        }

        let query_lower = query.to_ascii_lowercase();
        let tokens: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|token| token.len() >= 3)
            .collect();
        if tokens.len() >= 2 {
            for token in &tokens {
                match self.find_symbol(token, None, false, false, 10) {
                    Ok(hits) => {
                        for symbol in hits {
                            if seen_ids.insert(symbol.id.clone()) {
                                all_symbols.push(symbol);
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(token, error = %error, "token find_symbol failed");
                    }
                }
            }
        }

        Ok(all_symbols)
    }

    /// Hierarchical project structure: per-directory file count + symbol count.
    /// Used as Level 1 pruning — lets LLM decide which directories to drill into.
    pub fn get_project_structure(&self) -> Result<Vec<db::DirStats>> {
        let db = self.reader()?;
        db.dir_stats()
    }

    pub fn indexed_file_paths(&self) -> Result<Vec<String>> {
        let db = self.reader()?;
        db.all_file_paths()
    }

    pub fn get_symbols_overview(&self, path: &str, depth: usize) -> Result<Vec<SymbolInfo>> {
        let resolved = self.project.resolve(path)?;
        if resolved.is_dir() {
            let mut symbols = Vec::new();
            for file in collect_candidate_files(&resolved)? {
                let relative = self.project.to_relative(&file);
                let parsed = self.ensure_indexed(&file, &relative)?;
                if !parsed.is_empty() {
                    let id = make_symbol_id(&relative, &SymbolKind::File, &relative);
                    symbols.push(SymbolInfo {
                        name: relative.clone(),
                        kind: SymbolKind::File,
                        file_path: relative.clone(),
                        provenance: SymbolProvenance::from_path(&relative),
                        line: 0,
                        column: 0,
                        signature: format!(
                            "{} ({} symbols)",
                            file.file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or_default(),
                            parsed.len()
                        ),
                        name_path: relative,
                        id,
                        body: None,
                        children: parsed
                            .into_iter()
                            .map(|symbol| to_symbol_info(symbol, depth))
                            .collect(),
                        start_byte: 0,
                        end_byte: 0,
                        end_line: 0,
                    });
                }
            }
            return Ok(symbols);
        }

        let relative = self.project.to_relative(&resolved);
        let parsed = self.ensure_indexed(&resolved, &relative)?;
        Ok(parsed
            .into_iter()
            .map(|symbol| to_symbol_info(symbol, depth))
            .collect())
    }

    pub fn find_symbol(
        &self,
        name: &str,
        file_path: Option<&str>,
        include_body: bool,
        exact_match: bool,
        max_matches: usize,
    ) -> Result<Vec<SymbolInfo>> {
        if let Some((id_file, _id_kind, id_name_path)) = parse_symbol_id(name) {
            let resolved = self.project.resolve(id_file)?;
            let relative = self.project.to_relative(&resolved);
            self.ensure_indexed(&resolved, &relative)?;
            let leaf_name = id_name_path.rsplit('/').next().unwrap_or(id_name_path);
            let db = self.writer();
            let db_rows = db.find_symbols_by_name(leaf_name, Some(id_file), true, max_matches)?;
            let mut results = Vec::new();
            for row in db_rows {
                if row.name_path != id_name_path {
                    continue;
                }
                let rel_path = db.get_file_path(row.file_id)?.unwrap_or_default();
                let body = if include_body {
                    let abs = self.project.as_path().join(&rel_path);
                    fs::read_to_string(&abs).ok().map(|source| {
                        let extended_start =
                            extend_start_to_doc_comments(&source, row.start_byte as u32);
                        slice_source(&source, extended_start, row.end_byte as u32)
                    })
                } else {
                    None
                };
                let kind = SymbolKind::from_str_label(&row.kind);
                let id = make_symbol_id(&rel_path, &kind, &row.name_path);
                let provenance = SymbolProvenance::from_path(&rel_path);
                results.push(SymbolInfo {
                    name: row.name,
                    kind,
                    provenance,
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
                    end_line: if row.end_line > 0 {
                        row.end_line as usize
                    } else {
                        row.line as usize
                    },
                });
            }
            return Ok(results);
        }

        if let Some(file_path) = file_path {
            let resolved = self.project.resolve(file_path)?;
            let relative = self.project.to_relative(&resolved);
            self.ensure_indexed(&resolved, &relative)?;
        } else {
            let files = collect_candidate_files(self.project.as_path())?;
            for file in &files {
                let relative = self.project.to_relative(file);
                self.ensure_indexed(file, &relative)?;
            }
        }

        let db = self.writer();
        let db_rows = db.find_symbols_by_name(name, file_path, exact_match, max_matches)?;

        let mut results = Vec::new();
        for row in db_rows {
            let rel_path = db.get_file_path(row.file_id)?.unwrap_or_default();
            let body = if include_body {
                let abs = self.project.as_path().join(&rel_path);
                fs::read_to_string(&abs).ok().map(|source| {
                    let extended_start =
                        extend_start_to_doc_comments(&source, row.start_byte as u32);
                    slice_source(&source, extended_start, row.end_byte as u32)
                })
            } else {
                None
            };
            let kind = SymbolKind::from_str_label(&row.kind);
            let id = make_symbol_id(&rel_path, &kind, &row.name_path);
            let provenance = SymbolProvenance::from_path(&rel_path);
            results.push(SymbolInfo {
                name: row.name,
                kind,
                provenance,
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
                end_line: if row.end_line > 0 {
                    row.end_line as usize
                } else {
                    row.line as usize
                },
            });
        }
        Ok(results)
    }

    pub fn get_ranked_context(
        &self,
        query: &str,
        path: Option<&str>,
        max_tokens: usize,
        include_body: bool,
        depth: usize,
    ) -> Result<RankedContextResult> {
        let all_symbols = if let Some(path) = path {
            self.get_symbols_overview(path, depth)?
        } else {
            self.select_solve_symbols(query, depth)?
        };

        let mut scored = all_symbols
            .into_iter()
            .flat_map(flatten_symbol_infos)
            .filter_map(|symbol| score_symbol(query, &symbol).map(|score| (symbol, score)))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.cmp(&left.1));

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

    /// Access the underlying database (e.g. for import graph queries).
    pub fn db(&self) -> std::sync::MutexGuard<'_, IndexDb> {
        self.writer()
    }
}
