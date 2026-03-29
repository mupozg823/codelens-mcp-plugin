mod parser;
mod ranking;
mod reader;
mod scoring;
mod types;
mod writer;

use parser::{flatten_symbol_infos, flatten_symbols, parse_symbols, slice_source, to_symbol_info};
use scoring::score_symbol;
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
use std::collections::HashMap;
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
                .filter(|t| t.len() >= 2)
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
        if let Ok(direct) = self.find_symbol(query, None, false, false, 50) {
            for sym in direct {
                if !all_symbols.iter().any(|s: &SymbolInfo| s.id == sym.id) {
                    all_symbols.push(sym);
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
        let max_chars = max_tokens.saturating_mul(4);
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

        // Batch body extraction: read each file once instead of N+1 DB queries.
        // Cap at 32 files to bound memory usage on large projects.
        const FILE_CACHE_LIMIT: usize = 32;
        let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut selected = Vec::new();
        let mut char_budget = max_chars;

        for (symbol, score) in scored {
            let body = if include_body && symbol.end_byte > symbol.start_byte {
                let cache_full = file_cache.len() >= FILE_CACHE_LIMIT;
                let source = file_cache
                    .entry(symbol.file_path.clone())
                    .or_insert_with(|| {
                        if cache_full {
                            return None;
                        }
                        let abs = self.project.as_path().join(&symbol.file_path);
                        fs::read_to_string(&abs).ok()
                    });
                source
                    .as_deref()
                    .map(|s| slice_source(s, symbol.start_byte, symbol.end_byte))
            } else {
                None
            };

            let entry = RankedContextEntry {
                name: symbol.name,
                kind: symbol.kind.as_label().to_owned(),
                file: symbol.file_path,
                line: symbol.line,
                signature: symbol.signature,
                body,
                relevance_score: score,
            };
            let entry_size = serde_json::to_string(&entry)?.len();
            if char_budget < entry_size && !selected.is_empty() {
                break;
            }
            char_budget = char_budget.saturating_sub(entry_size);
            selected.push(entry);
        }

        Ok(RankedContextResult {
            query: query.to_owned(),
            count: selected.len(),
            symbols: selected,
            token_budget: max_tokens,
            chars_used: max_chars.saturating_sub(char_budget),
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

#[cfg(test)]
mod tests {
    use super::{find_symbol, get_symbols_overview, SymbolIndex, SymbolKind};
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn extracts_python_symbols() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let symbols = get_symbols_overview(&project, "src/service.py", 2).expect("symbols");
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Service");
        assert_eq!(symbols[0].kind, SymbolKind::Class);
        assert_eq!(symbols[0].children[0].name, "run");
    }

    #[test]
    fn finds_typescript_symbol_with_body() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let matches =
            find_symbol(&project, "fetchUser", None, true, true, 10).expect("find symbol");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, SymbolKind::Function);
        assert!(matches[0]
            .body
            .as_ref()
            .expect("body")
            .contains("return userId"));
    }

    #[test]
    fn index_refreshes_after_file_change() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let index = SymbolIndex::new_memory(project.clone());

        let initial = index
            .find_symbol("fetchUser", None, false, true, 10)
            .expect("initial symbol lookup");
        assert_eq!(initial.len(), 1);

        fs::write(
            root.join("src/user.ts"),
            "export function loadUser(userId: string) {\n  return userId\n}\n",
        )
        .expect("rewrite ts");

        let refreshed = index
            .find_symbol("loadUser", None, true, true, 10)
            .expect("refreshed symbol lookup");
        assert_eq!(refreshed.len(), 1);
        assert!(refreshed[0]
            .body
            .as_ref()
            .expect("body")
            .contains("loadUser"));
    }

    #[test]
    fn refresh_all_populates_stats() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let index = SymbolIndex::new_memory(project);
        let stats = index.refresh_all().expect("refresh all");
        assert_eq!(stats.supported_files, 2);
        assert_eq!(stats.indexed_files, 2);
        assert_eq!(stats.stale_files, 0);
    }

    #[test]
    fn reloads_index_from_disk() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        // Use real disk-backed SymbolIndex for persistence test
        let index = SymbolIndex::new(project.clone());
        index.refresh_all().expect("refresh all");

        let reloaded = SymbolIndex::new(project);
        let stats = reloaded.stats().expect("stats");
        assert_eq!(stats.indexed_files, 2);
    }

    #[test]
    fn ranked_context_prefers_exact_matches_and_respects_budget() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let index = SymbolIndex::new_memory(project);

        let ranked = index
            .get_ranked_context("fetchUser", None, 40, true, 2)
            .expect("ranked context");

        assert_eq!(ranked.query, "fetchUser");
        assert_eq!(ranked.token_budget, 40);
        assert!(!ranked.symbols.is_empty());
        assert_eq!(ranked.symbols[0].name, "fetchUser");
        assert_eq!(ranked.symbols[0].relevance_score, 100);
        assert!(ranked.symbols[0]
            .body
            .as_ref()
            .expect("body")
            .contains("fetchUser"));
        assert!(ranked.chars_used <= ranked.token_budget * 4);
    }

    #[test]
    fn extracts_go_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-go-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(
            dir.join("main.go"),
            "package main\n\ntype Server struct{}\n\nfunc NewServer() *Server { return &Server{} }\n\nfunc (s *Server) Run() {}\n",
        )
        .expect("write go");
        let project = ProjectRoot::new(&dir).expect("project");
        let symbols = get_symbols_overview(&project, "main.go", 1).expect("symbols");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Server"),
            "expected Server type, got {names:?}"
        );
        assert!(
            names.contains(&"NewServer"),
            "expected NewServer func, got {names:?}"
        );
    }

    #[test]
    fn extracts_java_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-java-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(
            dir.join("Service.java"),
            "public class Service {\n    public Service() {}\n    public void run() {}\n}\n",
        )
        .expect("write java");
        let project = ProjectRoot::new(&dir).expect("project");
        let symbols = get_symbols_overview(&project, "Service.java", 2).expect("symbols");
        assert!(!symbols.is_empty(), "expected symbols in Service.java");
        assert_eq!(symbols[0].name, "Service");
        assert_eq!(symbols[0].kind, SymbolKind::Class);
    }

    #[test]
    fn extracts_kotlin_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-kotlin-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(
            dir.join("Main.kt"),
            "class Main {\n    fun greet(name: String): String = \"Hello $name\"\n}\n\nfun main() {}\n",
        )
        .expect("write kotlin");
        let project = ProjectRoot::new(&dir).expect("project");
        let symbols = get_symbols_overview(&project, "Main.kt", 1).expect("symbols");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Main"),
            "expected Main class, got {names:?}"
        );
    }

    #[test]
    fn extracts_rust_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rust-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(
            dir.join("lib.rs"),
            "pub struct Config { pub name: String }\n\npub trait Handler {\n    fn handle(&self);\n}\n\npub fn run() {}\n",
        )
        .expect("write rust");
        let project = ProjectRoot::new(&dir).expect("project");
        let symbols = get_symbols_overview(&project, "lib.rs", 1).expect("symbols");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Config"),
            "expected Config struct, got {names:?}"
        );
        assert!(
            names.contains(&"Handler"),
            "expected Handler trait, got {names:?}"
        );
        assert!(names.contains(&"run"), "expected run fn, got {names:?}");
    }

    #[test]
    fn make_symbol_id_format() {
        use super::make_symbol_id;
        assert_eq!(
            make_symbol_id("src/service.py", &SymbolKind::Class, "Service"),
            "src/service.py#class:Service"
        );
        assert_eq!(
            make_symbol_id("src/service.py", &SymbolKind::Method, "Service/run"),
            "src/service.py#method:Service/run"
        );
    }

    #[test]
    fn parse_symbol_id_valid() {
        use super::parse_symbol_id;
        let result = parse_symbol_id("src/service.py#function:Service/run");
        assert_eq!(result, Some(("src/service.py", "function", "Service/run")));
    }

    #[test]
    fn parse_symbol_id_plain_name_returns_none() {
        use super::parse_symbol_id;
        assert_eq!(parse_symbol_id("fetchUser"), None);
        assert_eq!(parse_symbol_id("#class:"), None);
        assert_eq!(parse_symbol_id(""), None);
    }

    #[test]
    fn find_symbol_returns_id_field() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let matches =
            find_symbol(&project, "fetchUser", None, false, true, 10).expect("find symbol");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "src/user.ts#function:fetchUser");
    }

    #[test]
    fn find_symbol_by_stable_id() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let matches = find_symbol(
            &project,
            "src/user.ts#function:fetchUser",
            None,
            true,
            true,
            10,
        )
        .expect("find by id");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "fetchUser");
        assert_eq!(matches[0].kind, SymbolKind::Function);
        assert!(matches[0].body.is_some());
    }

    #[test]
    fn find_symbol_by_nested_id() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let matches = find_symbol(
            &project,
            "src/service.py#function:Service/run",
            None,
            false,
            true,
            10,
        )
        .expect("find nested by id");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "run");
        assert_eq!(matches[0].name_path, "Service/run");
    }

    #[test]
    fn get_symbols_overview_includes_id() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let symbols = get_symbols_overview(&project, "src/service.py", 2).expect("symbols");
        // Top-level class
        assert!(!symbols[0].id.is_empty());
        assert!(symbols[0].id.contains("#class:"));
        // Nested method
        let child = &symbols[0].children[0];
        assert!(
            child.id.contains("#method:Service/run") || child.id.contains("#function:Service/run")
        );
    }

    fn fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-symbols-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).expect("create src");
        fs::write(
            dir.join("src/service.py"),
            "class Service:\n    def run(self):\n        return True\n\nvalue = 1\n",
        )
        .expect("write python");
        fs::write(
            dir.join("src/user.ts"),
            "export interface User { id: string }\nexport function fetchUser(userId: string) {\n  return userId\n}\n",
        )
        .expect("write ts");
        dir
    }

    #[test]
    fn extracts_csharp_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-csharp-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(
            dir.join("Service.cs"),
            "namespace MyApp {\n    public class Service {\n        public Service() {}\n        public void Run() {}\n    }\n    public interface IService {}\n    public enum Status { Active, Inactive }\n}\n",
        )
        .expect("write cs");
        let project = ProjectRoot::new(&dir).expect("project");
        let symbols = get_symbols_overview(&project, "Service.cs", 2).expect("symbols");
        let names: Vec<&str> = symbols
            .iter()
            .flat_map(|s| {
                let mut v = vec![s.name.as_str()];
                v.extend(s.children.iter().map(|c| c.name.as_str()));
                v
            })
            .collect();
        assert!(
            names.contains(&"MyApp"),
            "expected namespace MyApp, got {names:?}"
        );
        assert!(
            names.contains(&"Service"),
            "expected class Service, got {names:?}"
        );
        assert!(
            names.contains(&"IService"),
            "expected interface IService, got {names:?}"
        );
        assert!(
            names.contains(&"Status"),
            "expected enum Status, got {names:?}"
        );
    }

    #[test]
    fn extracts_dart_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-dart-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(
            dir.join("main.dart"),
            "class UserService {\n  void fetchUser() {}\n}\n\nenum Role { admin, user }\n\nvoid main() {}\n",
        )
        .expect("write dart");
        let project = ProjectRoot::new(&dir).expect("project");
        let symbols = get_symbols_overview(&project, "main.dart", 2).expect("symbols");
        let names: Vec<&str> = symbols
            .iter()
            .flat_map(|s| {
                let mut v = vec![s.name.as_str()];
                v.extend(s.children.iter().map(|c| c.name.as_str()));
                v
            })
            .collect();
        assert!(
            names.contains(&"UserService"),
            "expected class UserService, got {names:?}"
        );
        assert!(names.contains(&"Role"), "expected enum Role, got {names:?}");
        assert!(
            names.contains(&"main"),
            "expected function main, got {names:?}"
        );
    }
}
