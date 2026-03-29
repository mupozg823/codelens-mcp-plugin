use crate::db::{self, content_hash, index_db_path, IndexDb, NewCall, NewImport, NewSymbol};
use crate::import_graph::{extract_imports_for_file, resolve_module_for_file};
// Re-export language_for_path so downstream crate modules keep working.
pub(crate) use crate::lang_config::{language_for_path, LanguageConfig};
use crate::project::ProjectRoot;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::UNIX_EPOCH;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCapture, QueryCursor};
use walkdir::WalkDir;

/// Cached compiled tree-sitter Query per language extension.
static QUERY_CACHE: LazyLock<Mutex<HashMap<&'static str, Arc<Query>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_query(config: &LanguageConfig) -> Result<Arc<Query>> {
    let mut cache = QUERY_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(q) = cache.get(config.extension) {
        return Ok(Arc::clone(q));
    }
    let q = Query::new(&config.language, config.query)
        .with_context(|| format!("invalid query for {}", config.extension))?;
    let q = Arc::new(q);
    cache.insert(config.extension, Arc::clone(&q));
    Ok(q)
}

use crate::project::{collect_files, is_excluded};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    File,
    Class,
    Interface,
    Enum,
    Module,
    Method,
    Function,
    Property,
    Variable,
    TypeAlias,
    Unknown,
}

impl SymbolKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            SymbolKind::File => "file",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::Module => "module",
            SymbolKind::Method => "method",
            SymbolKind::Function => "function",
            SymbolKind::Property => "property",
            SymbolKind::Variable => "variable",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub signature: String,
    pub name_path: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SymbolInfo>,
    /// Byte offsets for batch body extraction (not serialized to API output).
    #[serde(skip)]
    pub start_byte: usize,
    #[serde(skip)]
    pub end_byte: usize,
}

/// Construct a stable symbol ID: `{file_path}#{kind}:{name_path}`
pub fn make_symbol_id(file_path: &str, kind: &SymbolKind, name_path: &str) -> String {
    format!("{}#{}:{}", file_path, kind.as_label(), name_path)
}

/// Parse a stable symbol ID. Returns `(file_path, kind_label, name_path)` or `None`.
pub fn parse_symbol_id(input: &str) -> Option<(&str, &str, &str)> {
    let hash_pos = input.find('#')?;
    let after_hash = &input[hash_pos + 1..];
    let colon_pos = after_hash.find(':')?;
    let file_path = &input[..hash_pos];
    let kind = &after_hash[..colon_pos];
    let name_path = &after_hash[colon_pos + 1..];
    if file_path.is_empty() || kind.is_empty() || name_path.is_empty() {
        return None;
    }
    Some((file_path, kind, name_path))
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub indexed_files: usize,
    pub supported_files: usize,
    pub stale_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedContextEntry {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub relevance_score: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedContextResult {
    pub query: String,
    pub symbols: Vec<RankedContextEntry>,
    pub count: usize,
    pub token_budget: usize,
    pub chars_used: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParsedSymbol {
    name: String,
    kind: SymbolKind,
    file_path: String,
    line: usize,
    column: usize,
    start_byte: usize,
    end_byte: usize,
    signature: String,
    body: Option<String>,
    name_path: String,
    children: Vec<ParsedSymbol>,
}

/// Read-only DB access — either an owned read-only connection or a borrowed writer guard.
enum ReadDb<'a> {
    Owned(IndexDb),
    Writer(std::sync::MutexGuard<'a, IndexDb>),
}

impl std::ops::Deref for ReadDb<'_> {
    type Target = IndexDb;
    fn deref(&self) -> &IndexDb {
        match self {
            ReadDb::Owned(db) => db,
            ReadDb::Writer(guard) => guard,
        }
    }
}

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
            eprintln!(
                "[codelens] WARNING: failed to open DB at {}, falling back to in-memory: {e}",
                db_path.display()
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

    /// One-time migration from legacy symbols-v1.json to SQLite.
    fn migrate_from_json(&mut self) -> Result<()> {
        let json_path = self
            .project
            .as_path()
            .join(".codelens/index/symbols-v1.json");
        if !json_path.is_file() {
            return Ok(());
        }
        // Trigger a full refresh which populates the DB
        let stats = self.refresh_all()?;
        // Only remove the old JSON file after DB is confirmed populated
        if stats.indexed_files > 0 || stats.stale_files == 0 {
            let _ = fs::remove_file(&json_path);
        } else {
            eprintln!(
                "[codelens] WARNING: migration from JSON produced 0 indexed files, keeping {}",
                json_path.display()
            );
        }
        Ok(())
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

    pub fn refresh_all(&self) -> Result<IndexStats> {
        use rayon::prelude::*;

        let files = collect_candidate_files(self.project.as_path())?;

        // Phase 1: parallel parse (CPU-bound, no DB access)
        let parsed: Vec<_> = files
            .par_iter()
            .filter_map(|file| {
                let relative = self.project.to_relative(file);
                let content = fs::read(file).ok()?;
                let mtime = file_modified_ms(file).ok()? as i64;
                let hash = content_hash(&content);
                let source = String::from_utf8_lossy(&content);
                let ext = file.extension()?.to_str()?.to_ascii_lowercase();
                let symbols = language_for_path(file)
                    .and_then(|config| parse_symbols(&config, &relative, &source, false).ok())
                    .unwrap_or_default();
                let raw_imports = extract_imports_for_file(file);
                let new_imports: Vec<NewImport> = raw_imports
                    .iter()
                    .filter_map(|raw| {
                        resolve_module_for_file(&self.project, file, raw).map(|target| NewImport {
                            target_path: target,
                            raw_import: raw.clone(),
                        })
                    })
                    .collect();
                // Extract call edges for call graph caching
                let call_edges: Vec<NewCall> = crate::call_graph::extract_calls(file)
                    .into_iter()
                    .map(|e| NewCall {
                        caller_name: e.caller_name,
                        callee_name: e.callee_name,
                        line: e.line as i64,
                    })
                    .collect();
                Some((
                    relative,
                    mtime,
                    hash,
                    content.len() as i64,
                    ext,
                    symbols,
                    new_imports,
                    call_edges,
                ))
            })
            .collect();

        // Phase 2: sequential DB write in RAII transaction (auto-rollback on error)
        self.writer().with_transaction(|conn| {
            let mut on_disk = HashSet::new();
            for (relative, mtime, hash, size, ext, symbols, new_imports, call_edges) in parsed {
                on_disk.insert(relative.clone());
                if db::get_fresh_file(conn, &relative, mtime, &hash)?.is_some() {
                    continue;
                }
                let file_id = db::upsert_file(conn, &relative, mtime, &hash, size, Some(&ext))?;
                let flat = flatten_symbols(symbols);
                let new_syms: Vec<NewSymbol> = flat
                    .iter()
                    .map(|s| NewSymbol {
                        name: s.name.clone(),
                        kind: s.kind.as_label().to_owned(),
                        line: s.line as i64,
                        column_num: s.column as i64,
                        start_byte: s.start_byte as i64,
                        end_byte: s.end_byte as i64,
                        signature: s.signature.clone(),
                        name_path: s.name_path.clone(),
                        parent_id: None,
                    })
                    .collect();
                db::insert_symbols(conn, file_id, &new_syms)?;
                if !new_imports.is_empty() {
                    db::insert_imports(conn, file_id, &new_imports)?;
                }
                if !call_edges.is_empty() {
                    db::insert_calls(conn, file_id, &call_edges)?;
                }
            }

            // Remove files that no longer exist on disk
            for indexed_path in db::all_file_paths(conn)? {
                if !on_disk.contains(&indexed_path) {
                    db::delete_file(conn, &indexed_path)?;
                }
            }

            Ok(())
        })?;
        self.stats()
    }

    /// Incrementally re-index only the given files (changed/created).
    /// Deleted files should be passed separately via `remove_files`.
    pub fn index_files(&self, paths: &[PathBuf]) -> Result<usize> {
        use rayon::prelude::*;

        let parsed: Vec<_> = paths
            .par_iter()
            .filter_map(|file| {
                if !file.is_file() {
                    return None;
                }
                let config = language_for_path(file)?;
                let relative = self.project.to_relative(file);
                let content = fs::read(file).ok()?;
                let mtime = file_modified_ms(file).ok()? as i64;
                let hash = content_hash(&content);
                let source = String::from_utf8_lossy(&content);
                let ext = file.extension()?.to_str()?.to_ascii_lowercase();
                let symbols = parse_symbols(&config, &relative, &source, false)
                    .ok()
                    .unwrap_or_default();
                let raw_imports = extract_imports_for_file(file);
                let new_imports: Vec<NewImport> = raw_imports
                    .iter()
                    .filter_map(|raw| {
                        resolve_module_for_file(&self.project, file, raw).map(|target| NewImport {
                            target_path: target,
                            raw_import: raw.clone(),
                        })
                    })
                    .collect();
                let call_edges: Vec<NewCall> = crate::call_graph::extract_calls(file)
                    .into_iter()
                    .map(|e| NewCall {
                        caller_name: e.caller_name,
                        callee_name: e.callee_name,
                        line: e.line as i64,
                    })
                    .collect();
                Some((
                    relative,
                    mtime,
                    hash,
                    content.len() as i64,
                    ext,
                    symbols,
                    new_imports,
                    call_edges,
                ))
            })
            .collect();

        let count = parsed.len();
        if count == 0 {
            return Ok(0);
        }

        self.writer().with_transaction(|conn| {
            for (relative, mtime, hash, size, ext, symbols, new_imports, call_edges) in parsed {
                if db::get_fresh_file(conn, &relative, mtime, &hash)?.is_some() {
                    continue;
                }
                let file_id = db::upsert_file(conn, &relative, mtime, &hash, size, Some(&ext))?;
                let flat = flatten_symbols(symbols);
                let new_syms: Vec<NewSymbol> = flat
                    .iter()
                    .map(|s| NewSymbol {
                        name: s.name.clone(),
                        kind: s.kind.as_label().to_owned(),
                        line: s.line as i64,
                        column_num: s.column as i64,
                        start_byte: s.start_byte as i64,
                        end_byte: s.end_byte as i64,
                        signature: s.signature.clone(),
                        name_path: s.name_path.clone(),
                        parent_id: None,
                    })
                    .collect();
                db::insert_symbols(conn, file_id, &new_syms)?;
                if !new_imports.is_empty() {
                    db::insert_imports(conn, file_id, &new_imports)?;
                }
                if !call_edges.is_empty() {
                    db::insert_calls(conn, file_id, &call_edges)?;
                }
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// Remove deleted files from the index.
    pub fn remove_files(&self, paths: &[PathBuf]) -> Result<usize> {
        let count = paths.len();
        let relatives: Vec<String> = paths.iter().map(|p| self.project.to_relative(p)).collect();
        self.writer().with_transaction(|conn| {
            for relative in &relatives {
                db::delete_file(conn, relative)?;
            }
            Ok(())
        })?;
        Ok(count)
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
                        slice_source(&source, row.start_byte as usize, row.end_byte as usize)
                    })
                } else {
                    None
                };
                let kind = str_to_kind(&row.kind);
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
                    start_byte: row.start_byte as usize,
                    end_byte: row.end_byte as usize,
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
                fs::read_to_string(&abs).ok().map(|source| {
                    slice_source(&source, row.start_byte as usize, row.end_byte as usize)
                })
            } else {
                None
            };
            let kind = str_to_kind(&row.kind);
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
                start_byte: row.start_byte as usize,
                end_byte: row.end_byte as usize,
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
            self.find_symbol(query, None, false, false, 500)?
        };

        let mut scored = all_symbols
            .into_iter()
            .flat_map(flatten_symbol_infos)
            .filter_map(|symbol| score_symbol(query, &symbol).map(|score| (symbol, score)))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.cmp(&left.1));

        // Batch body extraction: read each file once instead of N+1 DB queries.
        let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut selected = Vec::new();
        let mut char_budget = max_chars;

        for (symbol, score) in scored {
            let body = if include_body && symbol.end_byte > symbol.start_byte {
                let source = file_cache
                    .entry(symbol.file_path.clone())
                    .or_insert_with(|| {
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

    // ---- Read-only methods (no ensure_indexed, DB-only queries) ----
    // These take &self and are safe to call under RwLock::read().

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
            // For directories, collect all DB-indexed files under the path
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
                                let kind = str_to_kind(&row.kind);
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
                                    start_byte: row.start_byte as usize,
                                    end_byte: row.end_byte as usize,
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
                let kind = str_to_kind(&row.kind);
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
                    start_byte: row.start_byte as usize,
                    end_byte: row.end_byte as usize,
                }
            })
            .collect())
    }

    /// Ranked context from DB without lazy indexing.
    pub fn get_ranked_context_cached(
        &self,
        query: &str,
        path: Option<&str>,
        max_tokens: usize,
        include_body: bool,
        depth: usize,
    ) -> Result<RankedContextResult> {
        let max_chars = max_tokens.saturating_mul(4);
        let all_symbols = if let Some(path) = path {
            self.get_symbols_overview_cached(path, depth)?
        } else {
            self.find_symbol_cached(query, None, false, false, 500)?
        };

        let mut scored = all_symbols
            .into_iter()
            .flat_map(flatten_symbol_infos)
            .filter_map(|symbol| score_symbol(query, &symbol).map(|score| (symbol, score)))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.cmp(&left.1));

        let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut selected = Vec::new();
        let mut char_budget = max_chars;

        for (symbol, score) in scored {
            let body = if include_body && symbol.end_byte > symbol.start_byte {
                let source = file_cache
                    .entry(symbol.file_path.clone())
                    .or_insert_with(|| {
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

    /// Helper: convert DB rows to SymbolInfo with optional body.
    fn rows_to_symbol_infos(
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
                fs::read_to_string(&abs).ok().map(|source| {
                    slice_source(&source, row.start_byte as usize, row.end_byte as usize)
                })
            } else {
                None
            };
            let kind = str_to_kind(&row.kind);
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
                start_byte: row.start_byte as usize,
                end_byte: row.end_byte as usize,
            });
        }
        Ok(results)
    }

    /// Access the underlying database (e.g. for import graph queries).
    pub fn db(&self) -> std::sync::MutexGuard<'_, IndexDb> {
        self.writer()
    }

    /// Ensure a file is indexed; returns parsed symbols for immediate use.
    fn ensure_indexed(&self, file: &Path, relative: &str) -> Result<Vec<ParsedSymbol>> {
        let mtime = file_modified_ms(file)? as i64;
        let db = self.writer();

        // Fast path: mtime unchanged → symbols already in DB, re-parse from source
        if db.get_fresh_file_by_mtime(relative, mtime)?.is_some() {
            let source = fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            if let Some(config) = language_for_path(file) {
                return parse_symbols(&config, relative, &source, false);
            }
            return Ok(Vec::new());
        }

        // Slow path: file changed or new — read, hash, parse, index
        let content =
            fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;
        let hash = content_hash(&content);
        let source = String::from_utf8_lossy(&content);
        let symbols = if let Some(config) = language_for_path(file) {
            parse_symbols(&config, relative, &source, false)?
        } else {
            Vec::new()
        };

        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        let file_id =
            db.upsert_file(relative, mtime, &hash, content.len() as i64, ext.as_deref())?;

        // Flatten and insert symbols
        let flat = flatten_symbols(symbols.clone());
        let new_syms: Vec<NewSymbol> = flat
            .iter()
            .map(|s| NewSymbol {
                name: s.name.clone(),
                kind: s.kind.as_label().to_owned(),
                line: s.line as i64,
                column_num: s.column as i64,
                start_byte: s.start_byte as i64,
                end_byte: s.end_byte as i64,
                signature: s.signature.clone(),
                name_path: s.name_path.clone(),
                parent_id: None,
            })
            .collect();
        db.insert_symbols(file_id, &new_syms)?;

        // Index imports
        let raw_imports = extract_imports_for_file(file);
        let new_imports: Vec<NewImport> = raw_imports
            .iter()
            .filter_map(|raw| {
                resolve_module_for_file(&self.project, file, raw).map(|target| NewImport {
                    target_path: target,
                    raw_import: raw.clone(),
                })
            })
            .collect();
        if !new_imports.is_empty() {
            db.insert_imports(file_id, &new_imports)?;
        }

        // Index call graph edges (consistent with refresh_all / index_files)
        let call_edges: Vec<NewCall> = crate::call_graph::extract_calls(file)
            .into_iter()
            .map(|e| NewCall {
                caller_name: e.caller_name,
                callee_name: e.callee_name,
                line: e.line as i64,
            })
            .collect();
        if !call_edges.is_empty() {
            db.insert_calls(file_id, &call_edges)?;
        }

        Ok(symbols)
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
        Some(sym) => Ok((sym.start_byte, sym.end_byte)),
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

fn parse_symbols(
    config: &LanguageConfig,
    file_path: &str,
    source: &str,
    include_body: bool,
) -> Result<Vec<ParsedSymbol>> {
    let mut parser = Parser::new();
    parser.set_language(&config.language).with_context(|| {
        format!(
            "failed to set tree-sitter language for {}",
            config.extension
        )
    })?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("failed to parse source"))?;
    let query = cached_query(config)?;
    let source_bytes = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut symbols = Vec::new();

    let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);
    while let Some(matched) = matches.next() {
        let mut def_capture: Option<(&QueryCapture<'_>, &str)> = None;
        let mut name_capture: Option<(&QueryCapture<'_>, &str)> = None;

        for capture in matched.captures.iter() {
            let capture_name = &query.capture_names()[capture.index as usize];
            if capture_name.ends_with(".def") && def_capture.is_none() {
                def_capture = Some((capture, capture_name));
            }
            if capture_name.ends_with(".name") && name_capture.is_none() {
                name_capture = Some((capture, capture_name));
            }
        }

        let Some((def_capture, capture_name)) = def_capture else {
            continue;
        };
        let Some((name_capture, _)) = name_capture else {
            continue;
        };

        let def_node = def_capture.node;
        let name_node = name_capture.node;
        let name = node_text(name_node, source_bytes).trim().to_owned();
        if name.is_empty() {
            continue;
        }

        let body = include_body.then(|| node_text(def_node, source_bytes).to_owned());
        symbols.push(ParsedSymbol {
            name: name.clone(),
            kind: capture_name_to_kind(capture_name),
            file_path: file_path.to_owned(),
            line: def_node.start_position().row + 1,
            column: name_node.start_position().column + 1,
            start_byte: def_node.start_byte(),
            end_byte: def_node.end_byte(),
            signature: build_signature(def_node, source_bytes, &name),
            body,
            name_path: name,
            children: Vec::new(),
        });
    }

    Ok(nest_symbols(dedup_symbols(symbols)))
}

fn flatten_symbols(symbols: Vec<ParsedSymbol>) -> Vec<ParsedSymbol> {
    let mut queue: VecDeque<ParsedSymbol> = symbols.into();
    let mut flat = Vec::new();

    while let Some(mut symbol) = queue.pop_front() {
        let children = std::mem::take(&mut symbol.children);
        queue.extend(children);
        flat.push(symbol);
    }

    flat
}

fn flatten_symbol_infos(mut symbol: SymbolInfo) -> Vec<SymbolInfo> {
    let children = std::mem::take(&mut symbol.children);
    let mut flattened = vec![symbol];
    for child in children {
        flattened.extend(flatten_symbol_infos(child));
    }
    flattened
}

fn score_symbol(query: &str, symbol: &SymbolInfo) -> Option<i32> {
    let query_lower = query.to_lowercase();
    if symbol.name.eq_ignore_ascii_case(query) {
        Some(100)
    } else if symbol.name.to_lowercase().contains(&query_lower) {
        Some(60)
    } else if symbol.signature.to_lowercase().contains(&query_lower) {
        Some(30)
    } else if symbol.name_path.to_lowercase().contains(&query_lower) {
        Some(20)
    } else {
        None
    }
}

fn nest_symbols(symbols: Vec<ParsedSymbol>) -> Vec<ParsedSymbol> {
    let mut sorted = symbols;
    sorted.sort_by_key(|symbol| symbol.start_byte);

    let mut roots = Vec::new();
    for symbol in sorted {
        insert_symbol(&mut roots, symbol);
    }
    roots
}

fn dedup_symbols(symbols: Vec<ParsedSymbol>) -> Vec<ParsedSymbol> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for symbol in symbols {
        // (start_byte, end_byte) is unique per symbol within a file
        let key = (symbol.start_byte, symbol.end_byte);
        if seen.insert(key) {
            deduped.push(symbol);
        }
    }

    deduped
}

fn insert_symbol(container: &mut Vec<ParsedSymbol>, mut symbol: ParsedSymbol) {
    if let Some(parent) = container.iter_mut().rev().find(|candidate| {
        candidate.start_byte <= symbol.start_byte && candidate.end_byte >= symbol.end_byte
    }) {
        symbol.name_path = format!("{}/{}", parent.name_path, symbol.name);
        insert_symbol(&mut parent.children, symbol);
    } else {
        container.push(symbol);
    }
}

fn to_symbol_info(symbol: ParsedSymbol, depth: usize) -> SymbolInfo {
    to_symbol_info_with_source(symbol, depth, None)
}

fn to_symbol_info_with_source(
    symbol: ParsedSymbol,
    depth: usize,
    source: Option<&str>,
) -> SymbolInfo {
    let children = if depth == 0 || depth > 1 {
        symbol
            .children
            .into_iter()
            .map(|child| to_symbol_info_with_source(child, depth.saturating_sub(1), source))
            .collect()
    } else {
        Vec::new()
    };

    let id = make_symbol_id(&symbol.file_path, &symbol.kind, &symbol.name_path);
    SymbolInfo {
        name: symbol.name,
        kind: symbol.kind,
        file_path: symbol.file_path,
        line: symbol.line,
        column: symbol.column,
        signature: symbol.signature,
        name_path: symbol.name_path,
        id,
        body: source
            .map(|source| slice_source(source, symbol.start_byte, symbol.end_byte))
            .or(symbol.body),
        children,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    }
}

fn slice_source(source: &str, start_byte: usize, end_byte: usize) -> String {
    source
        .as_bytes()
        .get(start_byte..end_byte)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or_default()
        .to_owned()
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

fn str_to_kind(s: &str) -> SymbolKind {
    match s {
        "class" => SymbolKind::Class,
        "interface" => SymbolKind::Interface,
        "enum" => SymbolKind::Enum,
        "module" => SymbolKind::Module,
        "method" => SymbolKind::Method,
        "function" => SymbolKind::Function,
        "property" => SymbolKind::Property,
        "variable" => SymbolKind::Variable,
        "type_alias" => SymbolKind::TypeAlias,
        _ => SymbolKind::Unknown,
    }
}

fn capture_name_to_kind(capture_name: &str) -> SymbolKind {
    if capture_name.starts_with("class") {
        SymbolKind::Class
    } else if capture_name.starts_with("interface") {
        SymbolKind::Interface
    } else if capture_name.starts_with("enum") {
        SymbolKind::Enum
    } else if capture_name.starts_with("module") {
        SymbolKind::Module
    } else if capture_name.starts_with("method") {
        SymbolKind::Method
    } else if capture_name.starts_with("function") {
        SymbolKind::Function
    } else if capture_name.starts_with("property") {
        SymbolKind::Property
    } else if capture_name.starts_with("variable") {
        SymbolKind::Variable
    } else if capture_name.starts_with("type_alias") {
        SymbolKind::TypeAlias
    } else {
        SymbolKind::Unknown
    }
}

fn build_signature(node: Node<'_>, source_bytes: &[u8], fallback: &str) -> String {
    let first_line = node_text(node, source_bytes)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or(fallback);

    if first_line.len() > 200 {
        format!("{}...", &first_line[..200])
    } else {
        first_line.to_owned()
    }
}

fn node_text<'a>(node: Node<'_>, source_bytes: &'a [u8]) -> &'a str {
    let start = node.start_byte();
    let end = node.end_byte();
    std::str::from_utf8(&source_bytes[start..end]).unwrap_or_default()
}

// LanguageConfig, language_for_path, and tree-sitter query constants
// are now in crate::lang_config.

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
