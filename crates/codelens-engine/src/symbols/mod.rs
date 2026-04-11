mod parser;
mod ranking;
mod reader;
pub mod scoring;
#[cfg(test)]
mod tests;
mod types;
mod writer;

use parser::{flatten_symbol_infos, flatten_symbols, parse_symbols, slice_source, to_symbol_info};
use ranking::prune_to_budget;
use scoring::score_symbol;
pub use scoring::{
    sparse_coverage_bonus_from_fields, sparse_max_bonus, sparse_threshold, sparse_weighting_enabled,
};
pub(crate) use types::ReadDb;
pub use types::{
    make_symbol_id, parse_symbol_id, IndexStats, RankedContextEntry, RankedContextResult,
    SymbolInfo, SymbolKind,
};

use crate::db::{self, content_hash, index_db_path, IndexDb};
// Re-export language_for_path so downstream crate modules keep working.
pub(crate) use crate::lang_config::{language_for_path, LanguageConfig};
use crate::project::ProjectRoot;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

use crate::project::{collect_files, is_excluded};

// Types (SymbolKind, SymbolInfo, ParsedSymbol, IndexStats, RankedContextEntry,
// RankedContextResult, ReadDb) are in types.rs, re-exported above.

/// SQLite-backed symbol index for a project.
///
/// Architecture: writer `Mutex<IndexDb>` for mutations + per-query read-only
/// connections for `_cached` methods. This makes `SymbolIndex: Send + Sync`,
/// enabling `Arc<SymbolIndex>` without an external Mutex.
pub struct SymbolIndex {
    project: ProjectRoot,
    db_path: PathBuf,
    writer: std::sync::Mutex<IndexDb>,
    /// In-memory mode flag (tests) — when true, _cached reads use the writer.
    in_memory: bool,
}

impl SymbolIndex {
    pub fn new(project: ProjectRoot) -> Self {
        let db_path = index_db_path(project.as_path());
        let db = IndexDb::open(&db_path).unwrap_or_else(|e| {
            tracing::warn!(
                path = %db_path.display(),
                error = %e,
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
        // Auto-migrate from legacy JSON index if DB is empty
        if idx.writer().file_count().unwrap_or(0) == 0 {
            let _ = idx.migrate_from_json();
        }
        idx
    }

    /// Acquire the writer connection (poison-safe).
    fn writer(&self) -> std::sync::MutexGuard<'_, IndexDb> {
        self.writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Open a read-only DB connection for queries (or fall back to writer for in-memory).
    fn reader(&self) -> Result<ReadDb<'_>> {
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
                Ok(c) => c,
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

    /// SelectSolve file pre-filtering: score files by name relevance to query,
    /// then extract symbols only from top-scoring files.
    fn select_solve_symbols(&self, query: &str, depth: usize) -> Result<Vec<SymbolInfo>> {
        // Collect file paths and compute top matches inside a block so the
        // MutexGuard (ReadDb::Writer) is dropped before we call find_symbol /
        // get_symbols_overview_cached, which also need the lock.  Holding the
        // guard across those calls causes a deadlock with in-memory DBs.
        let top_files: Vec<String> = {
            let db = self.reader()?;
            let all_paths = db.all_file_paths()?;

            let query_lower = query.to_ascii_lowercase();
            let query_tokens: Vec<&str> = query_lower
                .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
                .filter(|t| t.len() >= 3)
                .collect();

            let mut file_scores: Vec<(String, usize)> = all_paths
                .into_iter()
                .map(|path| {
                    let path_lower = path.to_ascii_lowercase();
                    let score = query_tokens
                        .iter()
                        .filter(|token| path_lower.contains(**token))
                        .count();
                    (path, score)
                })
                .collect();

            file_scores.sort_by(|a, b| b.1.cmp(&a.1));
            file_scores
                .into_iter()
                .filter(|(_, score)| *score > 0)
                .take(10)
                .map(|(path, _)| path)
                .collect()
            // db (MutexGuard) dropped here
        };

        // If no file matches, fall back to direct symbol name search
        if top_files.is_empty() {
            return self.find_symbol(query, None, false, false, 500);
        }

        // Collect symbols from top files
        let mut all_symbols = Vec::new();
        for file_path in &top_files {
            if let Ok(symbols) = self.get_symbols_overview_cached(file_path, depth) {
                all_symbols.extend(symbols);
            }
        }

        // Also include direct symbol name matches (for exact/substring hits)
        let mut seen_ids: std::collections::HashSet<String> =
            all_symbols.iter().map(|s| s.id.clone()).collect();

        if let Ok(direct) = self.find_symbol(query, None, false, false, 50) {
            for sym in direct {
                if seen_ids.insert(sym.id.clone()) {
                    all_symbols.push(sym);
                }
            }
        }

        // For multi-word queries, also search individual tokens as symbol names
        // (e.g., "dispatch tool call" → search for "dispatch", "tool", "call")
        let query_lower = query.to_ascii_lowercase();
        let tokens: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|t| t.len() >= 3)
            .collect();
        if tokens.len() >= 2 {
            for token in &tokens {
                match self.find_symbol(token, None, false, false, 10) {
                    Ok(hits) => {
                        for sym in hits {
                            if seen_ids.insert(sym.id.clone()) {
                                all_symbols.push(sym);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(token, error = %e, "token find_symbol failed");
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

    pub fn get_symbols_overview(&self, path: &str, depth: usize) -> Result<Vec<SymbolInfo>> {
        let resolved = self.project.resolve(path)?;
        if resolved.is_dir() {
            let mut symbols = Vec::new();
            for file in WalkDir::new(&resolved)
                .into_iter()
                .filter_entry(|entry| !is_excluded(entry.path()))
            {
                let file = file?;
                if !file.file_type().is_file() || language_for_path(file.path()).is_none() {
                    continue;
                }
                let relative = self.project.to_relative(file.path());
                let parsed = self.ensure_indexed(file.path(), &relative)?;
                if !parsed.is_empty() {
                    let id = make_symbol_id(&relative, &SymbolKind::File, &relative);
                    symbols.push(SymbolInfo {
                        name: relative.clone(),
                        kind: SymbolKind::File,
                        file_path: relative.clone(),
                        line: 0,
                        column: 0,
                        signature: format!(
                            "{} ({} symbols)",
                            file.file_name().to_string_lossy(),
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
        // Fast path: if name looks like a stable symbol ID, parse and do targeted lookup
        if let Some((id_file, _id_kind, id_name_path)) = parse_symbol_id(name) {
            let resolved = self.project.resolve(id_file)?;
            let relative = self.project.to_relative(&resolved);
            self.ensure_indexed(&resolved, &relative)?;
            // Extract the leaf name from name_path (after last '/')
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
                        slice_source(&source, row.start_byte as u32, row.end_byte as u32)
                    })
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
            return Ok(results);
        }

        // Ensure target files are indexed first
        if let Some(fp) = file_path {
            let resolved = self.project.resolve(fp)?;
            let relative = self.project.to_relative(&resolved);
            self.ensure_indexed(&resolved, &relative)?;
        } else {
            // Ensure all files are indexed for a global search
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
            // SelectSolve: file pre-filtering → top files → symbol extraction
            self.select_solve_symbols(query, depth)?
        };

        let mut scored = all_symbols
            .into_iter()
            .flat_map(flatten_symbol_infos)
            .filter_map(|symbol| score_symbol(query, &symbol).map(|score| (symbol, score)))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.cmp(&left.1));

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

    /// Access the underlying database (e.g. for import graph queries).
    pub fn db(&self) -> std::sync::MutexGuard<'_, IndexDb> {
        self.writer()
    }
}

pub fn get_symbols_overview(
    project: &ProjectRoot,
    path: &str,
    depth: usize,
) -> Result<Vec<SymbolInfo>> {
    let resolved = project.resolve(path)?;
    if resolved.is_dir() {
        return get_directory_symbols(project, &resolved, depth);
    }
    get_file_symbols(project, &resolved, depth)
}

/// Find the byte range (start_byte, end_byte) of a named symbol in a file.
/// If name_path is provided (e.g. "ClassName/method"), matches by full name_path;
/// otherwise matches by symbol name alone.
pub fn find_symbol_range(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
) -> Result<(usize, usize)> {
    let file = project.resolve(relative_path)?;
    let rel = project.to_relative(&file);
    let Some(language_config) = language_for_path(&file) else {
        bail!("unsupported file type: {}", file.display());
    };
    let source =
        fs::read_to_string(&file).with_context(|| format!("failed to read {}", file.display()))?;
    let parsed = parse_symbols(&language_config, &rel, &source, false)?;
    let flat = flatten_symbols(parsed);

    let candidate = if let Some(np) = name_path {
        flat.into_iter()
            .find(|sym| sym.name_path == np || sym.name == symbol_name)
    } else {
        flat.into_iter().find(|sym| sym.name == symbol_name)
    };

    match candidate {
        Some(sym) => Ok((sym.start_byte as usize, sym.end_byte as usize)),
        None => bail!(
            "symbol '{}' not found in {}",
            name_path.unwrap_or(symbol_name),
            relative_path
        ),
    }
}

pub fn find_symbol(
    project: &ProjectRoot,
    name: &str,
    file_path: Option<&str>,
    include_body: bool,
    exact_match: bool,
    max_matches: usize,
) -> Result<Vec<SymbolInfo>> {
    // Fast path: stable symbol ID
    if let Some((id_file, _id_kind, id_name_path)) = parse_symbol_id(name) {
        let resolved = project.resolve(id_file)?;
        let rel = project.to_relative(&resolved);
        let Some(language_config) = language_for_path(&resolved) else {
            return Ok(Vec::new());
        };
        let source = fs::read_to_string(&resolved)?;
        let parsed = parse_symbols(&language_config, &rel, &source, include_body)?;
        let mut results = Vec::new();
        for symbol in flatten_symbols(parsed) {
            if symbol.name_path == id_name_path {
                results.push(to_symbol_info(symbol, usize::MAX));
                if results.len() >= max_matches {
                    return Ok(results);
                }
            }
        }
        return Ok(results);
    }

    let files = match file_path {
        Some(path) => vec![project.resolve(path)?],
        None => collect_candidate_files(project.as_path())?,
    };

    let query = name.to_lowercase();
    let mut results = Vec::new();

    for file in files {
        let rel = project.to_relative(&file);
        let Some(language_config) = language_for_path(&file) else {
            continue;
        };
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let parsed = parse_symbols(&language_config, &rel, &source, include_body)?;
        for symbol in flatten_symbols(parsed) {
            let matched = if exact_match {
                symbol.name == name
            } else {
                symbol.name.to_lowercase().contains(&query)
            };
            if matched {
                results.push(to_symbol_info(symbol, usize::MAX));
                if results.len() >= max_matches {
                    return Ok(results);
                }
            }
        }
    }

    Ok(results)
}

fn get_directory_symbols(
    project: &ProjectRoot,
    dir: &Path,
    depth: usize,
) -> Result<Vec<SymbolInfo>> {
    let mut symbols = Vec::new();
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if language_for_path(path).is_none() {
            continue;
        }
        let file_symbols = get_file_symbols(project, path, depth)?;
        if !file_symbols.is_empty() {
            let relative = project.to_relative(path);
            let id = make_symbol_id(&relative, &SymbolKind::File, &relative);
            symbols.push(SymbolInfo {
                name: relative.clone(),
                kind: SymbolKind::File,
                file_path: relative.clone(),
                line: 0,
                column: 0,
                signature: format!(
                    "{} ({} symbols)",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                    file_symbols.len()
                ),
                name_path: relative,
                id,
                body: None,
                children: file_symbols,
                start_byte: 0,
                end_byte: 0,
            });
        }
    }
    Ok(symbols)
}

fn get_file_symbols(project: &ProjectRoot, file: &Path, depth: usize) -> Result<Vec<SymbolInfo>> {
    let relative = project.to_relative(file);
    let Some(language_config) = language_for_path(file) else {
        return Ok(Vec::new());
    };
    let source =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let parsed = parse_symbols(&language_config, &relative, &source, false)?;
    Ok(parsed
        .into_iter()
        .map(|symbol| to_symbol_info(symbol, depth))
        .collect())
}

fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| language_for_path(path).is_some())
}

fn file_modified_ms(path: &Path) -> Result<u128> {
    let modified = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for {}", path.display()))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis())
}
