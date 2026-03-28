use crate::db::{content_hash, index_db_path, IndexDb, NewImport, NewSymbol};
use crate::import_graph::{extract_imports_for_file, resolve_module_for_file};
use crate::project::ProjectRoot;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCapture, QueryCursor};
use walkdir::WalkDir;

const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".idea",
    ".gradle",
    "build",
    "dist",
    "out",
    "node_modules",
    "__pycache__",
    "target",
    ".next",
    ".venv",
];

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SymbolInfo>,
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

/// SQLite-backed symbol index for a project.
pub struct SymbolIndex {
    project: ProjectRoot,
    db: IndexDb,
}

impl SymbolIndex {
    pub fn new(project: ProjectRoot) -> Self {
        let db_path = index_db_path(project.as_path());
        let db = IndexDb::open(&db_path).unwrap_or_else(|_| IndexDb::open_memory().unwrap());
        let mut idx = Self { project, db };
        // Auto-migrate from legacy JSON index if DB is empty
        if idx.db.file_count().unwrap_or(0) == 0 {
            let _ = idx.migrate_from_json();
        }
        idx
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
        // Trigger a full refresh which populates the DB, then remove the old file
        self.refresh_all()?;
        let _ = fs::remove_file(&json_path);
        Ok(())
    }

    /// Create an in-memory index (for tests and benchmarks — no disk persistence).
    pub fn new_memory(project: ProjectRoot) -> Self {
        let db = IndexDb::open_memory().unwrap();
        Self { project, db }
    }

    pub fn stats(&self) -> Result<IndexStats> {
        let supported_files = collect_candidate_files(self.project.as_path())?;
        let indexed_files = self.db.file_count()?;
        let indexed_paths = self.db.all_file_paths()?;

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
            if self.db.get_fresh_file(rel, mtime, &hash)?.is_none() {
                stale += 1;
            }
        }

        Ok(IndexStats {
            indexed_files,
            supported_files: supported_files.len(),
            stale_files: stale,
        })
    }

    pub fn refresh_all(&mut self) -> Result<IndexStats> {
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
                Some((
                    relative,
                    mtime,
                    hash,
                    content.len() as i64,
                    ext,
                    symbols,
                    new_imports,
                ))
            })
            .collect();

        // Phase 2: sequential DB write (SQLite single-writer)
        self.db.begin_transaction()?;
        let mut on_disk = HashSet::new();
        for (relative, mtime, hash, size, ext, symbols, new_imports) in parsed {
            on_disk.insert(relative.clone());
            if self.db.get_fresh_file(&relative, mtime, &hash)?.is_some() {
                continue;
            }
            let file_id = self
                .db
                .upsert_file(&relative, mtime, &hash, size, Some(&ext))?;
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
            self.db.insert_symbols(file_id, &new_syms)?;
            if !new_imports.is_empty() {
                self.db.insert_imports(file_id, &new_imports)?;
            }
        }

        // Remove files that no longer exist on disk
        for indexed_path in self.db.all_file_paths()? {
            if !on_disk.contains(&indexed_path) {
                self.db.delete_file(&indexed_path)?;
            }
        }

        self.db.commit()?;
        self.stats()
    }

    pub fn get_symbols_overview(&mut self, path: &str, depth: usize) -> Result<Vec<SymbolInfo>> {
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
                        body: None,
                        children: parsed
                            .into_iter()
                            .map(|symbol| to_symbol_info(symbol, depth))
                            .collect(),
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
        &mut self,
        name: &str,
        file_path: Option<&str>,
        include_body: bool,
        exact_match: bool,
        max_matches: usize,
    ) -> Result<Vec<SymbolInfo>> {
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

        let db_rows = self
            .db
            .find_symbols_by_name(name, file_path, exact_match, max_matches)?;

        let mut results = Vec::new();
        for row in db_rows {
            let rel_path = self.db.get_file_path(row.file_id)?.unwrap_or_default();
            let body = if include_body {
                let abs = self.project.as_path().join(&rel_path);
                fs::read_to_string(&abs).ok().map(|source| {
                    slice_source(&source, row.start_byte as usize, row.end_byte as usize)
                })
            } else {
                None
            };
            results.push(SymbolInfo {
                name: row.name,
                kind: str_to_kind(&row.kind),
                file_path: rel_path,
                line: row.line as usize,
                column: row.column_num as usize,
                signature: row.signature,
                name_path: row.name_path,
                body,
                children: Vec::new(), // flat result from DB
            });
        }
        Ok(results)
    }

    pub fn get_ranked_context(
        &mut self,
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

        let mut selected = Vec::new();
        let mut char_budget = max_chars;

        for (symbol, score) in scored {
            let body = if include_body {
                self.find_symbol(&symbol.name, Some(&symbol.file_path), true, true, 20)?
                    .into_iter()
                    .find(|candidate| candidate.name_path == symbol.name_path)
                    .or_else(|| {
                        self.find_symbol(&symbol.name, Some(&symbol.file_path), true, true, 20)
                            .ok()
                            .and_then(|mut matches| matches.drain(..).next())
                    })
                    .and_then(|candidate| candidate.body)
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
    pub fn db(&self) -> &IndexDb {
        &self.db
    }

    /// Ensure a file is indexed; returns parsed symbols for immediate use.
    fn ensure_indexed(&mut self, file: &Path, relative: &str) -> Result<Vec<ParsedSymbol>> {
        let content =
            fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;
        let mtime = file_modified_ms(file)? as i64;
        let hash = content_hash(&content);

        // Check if already fresh
        if self.db.get_fresh_file(relative, mtime, &hash)?.is_some() {
            // Re-parse from source for the caller (symbols are in DB but caller needs ParsedSymbol tree)
            let source = String::from_utf8_lossy(&content);
            if let Some(config) = language_for_path(file) {
                return parse_symbols(&config, relative, &source, false);
            }
            return Ok(Vec::new());
        }

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
            self.db
                .upsert_file(relative, mtime, &hash, content.len() as i64, ext.as_deref())?;

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
        self.db.insert_symbols(file_id, &new_syms)?;

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
            self.db.insert_imports(file_id, &new_imports)?;
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
            .find(|sym| sym.name_path == np || sym.name == symbol_name && sym.name_path == np)
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
                body: None,
                children: file_symbols,
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
    let query = Query::new(&config.language, config.query)
        .with_context(|| format!("invalid query for {}", config.extension))?;
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

    while let Some(symbol) = queue.pop_front() {
        queue.extend(symbol.children.iter().cloned());
        flat.push(symbol);
    }

    flat
}

fn flatten_symbol_infos(symbol: SymbolInfo) -> Vec<SymbolInfo> {
    let mut flattened = vec![symbol.clone()];
    for child in symbol.children {
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
        let key = (
            symbol.file_path.clone(),
            symbol.name.clone(),
            symbol.kind.clone(),
            symbol.start_byte,
            symbol.end_byte,
        );
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

    SymbolInfo {
        name: symbol.name,
        kind: symbol.kind,
        file_path: symbol.file_path,
        line: symbol.line,
        column: symbol.column,
        signature: symbol.signature,
        name_path: symbol.name_path,
        body: source
            .map(|source| slice_source(source, symbol.start_byte, symbol.end_byte))
            .or(symbol.body),
        children,
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
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if entry.file_type().is_file() && language_for_path(entry.path()).is_some() {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
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

fn is_excluded(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        EXCLUDED_DIRS.contains(&value.as_ref())
    })
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

struct LanguageConfig {
    extension: &'static str,
    language: Language,
    query: &'static str,
}

fn language_for_path(path: &Path) -> Option<LanguageConfig> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "py" => Some(LanguageConfig {
            extension: "py",
            language: tree_sitter_python::LANGUAGE.into(),
            query: PYTHON_QUERY,
        }),
        "js" | "mjs" | "cjs" => Some(LanguageConfig {
            extension: "js",
            language: tree_sitter_javascript::LANGUAGE.into(),
            query: JAVASCRIPT_QUERY,
        }),
        "ts" => Some(LanguageConfig {
            extension: "ts",
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            query: TYPESCRIPT_QUERY,
        }),
        "tsx" | "jsx" => Some(LanguageConfig {
            extension: "tsx",
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            query: TYPESCRIPT_QUERY,
        }),
        "go" => Some(LanguageConfig {
            extension: "go",
            language: tree_sitter_go::LANGUAGE.into(),
            query: GO_QUERY,
        }),
        "java" => Some(LanguageConfig {
            extension: "java",
            language: tree_sitter_java::LANGUAGE.into(),
            query: JAVA_QUERY,
        }),
        "kt" | "kts" => Some(LanguageConfig {
            extension: "kt",
            language: tree_sitter_kotlin::LANGUAGE.into(),
            query: KOTLIN_QUERY,
        }),
        "rs" => Some(LanguageConfig {
            extension: "rs",
            language: tree_sitter_rust::LANGUAGE.into(),
            query: RUST_QUERY,
        }),
        "c" | "h" => Some(LanguageConfig {
            extension: "c",
            language: tree_sitter_c::LANGUAGE.into(),
            query: C_QUERY,
        }),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(LanguageConfig {
            extension: "cpp",
            language: tree_sitter_cpp::LANGUAGE.into(),
            query: CPP_QUERY,
        }),
        "php" => Some(LanguageConfig {
            extension: "php",
            language: tree_sitter_php::LANGUAGE_PHP.into(),
            query: PHP_QUERY,
        }),
        "swift" => Some(LanguageConfig {
            extension: "swift",
            language: tree_sitter_swift::LANGUAGE.into(),
            query: SWIFT_QUERY,
        }),
        "scala" | "sc" => Some(LanguageConfig {
            extension: "scala",
            language: tree_sitter_scala::LANGUAGE.into(),
            query: SCALA_QUERY,
        }),
        "rb" => Some(LanguageConfig {
            extension: "rb",
            language: tree_sitter_ruby::LANGUAGE.into(),
            query: RUBY_QUERY,
        }),
        _ => None,
    }
}

const PYTHON_QUERY: &str = r#"
    (class_definition name: (identifier) @class.name) @class.def
    (function_definition name: (identifier) @function.name) @function.def
    (decorated_definition definition: (class_definition name: (identifier) @class.name)) @class.def
    (decorated_definition definition: (function_definition name: (identifier) @function.name)) @function.def
    (assignment left: (identifier) @variable.name) @variable.def
"#;

const JAVASCRIPT_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
    (method_definition name: (property_identifier) @method.name) @method.def
    (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
    (variable_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
"#;

const TYPESCRIPT_QUERY: &str = r#"
    (class_declaration name: (type_identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
    (method_definition name: (property_identifier) @method.name) @method.def
    (interface_declaration name: (type_identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (type_alias_declaration name: (type_identifier) @type_alias.name) @type_alias.def
    (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
"#;

const GO_QUERY: &str = r#"
    (function_declaration name: (identifier) @function.name) @function.def
    (method_declaration name: (field_identifier) @method.name) @method.def
    (type_declaration (type_spec name: (type_identifier) @class.name)) @class.def
"#;

const JAVA_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (interface_declaration name: (identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (method_declaration name: (identifier) @method.name) @method.def
    (constructor_declaration name: (identifier) @method.name) @method.def
"#;

const KOTLIN_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (object_declaration name: (identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
"#;

const RUST_QUERY: &str = r#"
    (struct_item name: (type_identifier) @class.name) @class.def
    (enum_item name: (type_identifier) @enum.name) @enum.def
    (trait_item name: (type_identifier) @interface.name) @interface.def
    (function_item name: (identifier) @function.name) @function.def
    (const_item name: (identifier) @variable.name) @variable.def
    (static_item name: (identifier) @variable.name) @variable.def
    (type_item name: (type_identifier) @typealias.name) @typealias.def
"#;

const C_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(type_definition declarator: (type_identifier) @typealias.name) @typealias.def
"#;

const CPP_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(class_specifier name: (type_identifier) @class.name) @class.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(namespace_definition name: (identifier) @module.name) @module.def
"#;

const PHP_QUERY: &str = r#"
(class_declaration name: (name) @class.name) @class.def
(interface_declaration name: (name) @interface.name) @interface.def
(trait_declaration name: (name) @interface.name) @interface.def
(enum_declaration name: (name) @enum.name) @enum.def
(function_definition name: (name) @function.name) @function.def
(method_declaration name: (name) @method.name) @method.def
"#;

const SWIFT_QUERY: &str = r#"
(class_declaration name: (type_identifier) @class.name) @class.def
(protocol_declaration name: (type_identifier) @interface.name) @interface.def
(function_declaration name: (simple_identifier) @function.name) @function.def
"#;

const SCALA_QUERY: &str = r#"
    (class_definition name: (identifier) @class.name) @class.def
    (object_definition name: (identifier) @class.name) @class.def
    (trait_definition name: (identifier) @interface.name) @interface.def
    (function_definition name: (identifier) @function.name) @function.def
"#;

const RUBY_QUERY: &str = r#"
    (class name: [(constant) (scope_resolution)] @class.name) @class.def
    (module name: [(constant) (scope_resolution)] @module.name) @module.def
    (method name: [(identifier) (constant) (simple_symbol) (delimited_symbol) (setter)] @method.name) @method.def
    (singleton_method name: [(identifier) (constant) (simple_symbol) (delimited_symbol) (setter)] @method.name) @method.def
"#;

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
        let mut index = SymbolIndex::new_memory(project.clone());

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
        let mut index = SymbolIndex::new_memory(project);
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
        let mut index = SymbolIndex::new(project.clone());
        index.refresh_all().expect("refresh all");

        let reloaded = SymbolIndex::new(project);
        let stats = reloaded.stats().expect("stats");
        assert_eq!(stats.indexed_files, 2);
    }

    #[test]
    fn ranked_context_prefers_exact_matches_and_respects_budget() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let mut index = SymbolIndex::new_memory(project);

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
}
