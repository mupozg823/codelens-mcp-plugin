use crate::call_graph::extract_calls;
use crate::db::{index_db_path, IndexDb};
use crate::project::{collect_files, ProjectRoot};
use anyhow::{bail, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

// ── Python ────────────────────────────────────────────────────────────────────
static PY_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_.,\s]+)").unwrap());
static PY_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*from\s+([A-Za-z0-9_\.]+)\s+import\s+").unwrap());

// ── JavaScript / TypeScript ───────────────────────────────────────────────────
static JS_IMPORT_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\bimport\s+[^;]*?\sfrom\s+["']([^"']+)["']"#).unwrap());
static JS_IMPORT_SIDE_EFFECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\bimport\s+["']([^"']+)["']"#).unwrap());
static JS_REQUIRE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"require\(\s*["']([^"']+)["']\s*\)"#).unwrap());
static JS_DYNAMIC_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\(\s*["']([^"']+)["']\s*\)"#).unwrap());

// ── Go ────────────────────────────────────────────────────────────────────────
static GO_SINGLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*import\s+"([^"]+)""#).unwrap());
static GO_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+)""#).unwrap());
static GO_BLOCK_SECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)\bimport\s*\(([^)]*)\)"#).unwrap());

// ── Java ──────────────────────────────────────────────────────────────────────
static JAVA_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*import\s+(?:static\s+)?([A-Za-z0-9_.]+)\s*;").unwrap());

// ── Kotlin ────────────────────────────────────────────────────────────────────
static KT_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_.]+)(?:\s+as\s+[A-Za-z0-9_]+)?").unwrap()
});

// ── Rust ──────────────────────────────────────────────────────────────────────
static RS_USE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([A-Za-z0-9_]+(?:::[A-Za-z0-9_]+)*)(?:::\{([^}]+)\})?")
        .unwrap()
});
static RS_MOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z0-9_]+)\s*;").unwrap()
});

// ── Ruby ──────────────────────────────────────────────────────────────────────
static RB_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:require|require_relative|load)\s+["']([^"']+)["']"#).unwrap()
});

// ── C / C++ ───────────────────────────────────────────────────────────────────
static C_INCLUDE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*#\s*include\s+[<"]([^>"]+)[>"]"#).unwrap());

// ── PHP ───────────────────────────────────────────────────────────────────────
static PHP_USE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*use\s+([A-Za-z0-9_\\]+)\s*;").unwrap());
static PHP_REQ_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:require|require_once|include|include_once)\s+["']([^"']+)["']\s*;"#)
        .unwrap()
});

// ── C# ───────────────────────────────────────────────────────────────────────
static CS_USING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*using\s+(?:static\s+)?([A-Za-z0-9_.]+)\s*;").unwrap());

// ── Dart ─────────────────────────────────────────────────────────────────────
static DART_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*import\s+["']([^"']+)["']"#).unwrap());
static DART_EXPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*export\s+["']([^"']+)["']"#).unwrap());

// ── collect_top_level_funcs patterns ─────────────────────────────────────────
static TLF_PY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^def ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static TLF_JS_RE1: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^function ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static TLF_JS_RE2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:export\s+)?(?:async\s+)?function ([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});
static TLF_GO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^func ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static TLF_JVM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:public|private|protected|static|\s)+\s+\w+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")
        .unwrap()
});
static TLF_RS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:pub(?:\([^)]*\))?\s+)?fn ([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});

/// Use lang_registry as the single source of truth for supported extensions.
pub fn is_import_supported(ext: &str) -> bool {
    crate::lang_registry::supports_imports(ext)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlastRadiusEntry {
    pub file: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImporterEntry {
    pub file: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportanceEntry {
    pub file: String,
    pub score: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntry {
    pub file: String,
    pub symbol: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub(crate) imports: HashSet<String>,
    pub(crate) imported_by: HashSet<String>,
}

pub struct GraphCache {
    inner: Mutex<GraphCacheInner>,
}

struct GraphCacheInner {
    graph: Option<Arc<HashMap<String, FileNode>>>,
    built_at: Option<Instant>,
    ttl: Duration,
}

impl GraphCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(GraphCacheInner {
                graph: None,
                built_at: None,
                ttl: Duration::from_secs(ttl_secs),
            }),
        }
    }

    pub fn get_or_build(&self, project: &ProjectRoot) -> Result<Arc<HashMap<String, FileNode>>> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("graph cache lock poisoned"))?;
        if let (Some(graph), Some(built_at)) = (&inner.graph, inner.built_at) {
            if built_at.elapsed() < inner.ttl {
                return Ok(Arc::clone(graph));
            }
        }
        let graph = Arc::new(build_graph(project)?);
        inner.graph = Some(Arc::clone(&graph));
        inner.built_at = Some(Instant::now());
        Ok(graph)
    }

    /// Return per-file PageRank scores from the cached graph.
    pub fn file_pagerank_scores(&self, project: &ProjectRoot) -> HashMap<String, f64> {
        let graph = match self.get_or_build(project) {
            Ok(g) => g,
            Err(_) => return HashMap::new(),
        };
        compute_pagerank(&graph)
    }

    pub fn invalidate(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.graph = None;
            inner.built_at = None;
        }
    }
}

pub fn supports_import_graph(file_path: &str) -> bool {
    crate::lang_registry::supports_imports_for_path(Path::new(file_path))
}

pub fn get_blast_radius(
    project: &ProjectRoot,
    file_path: &str,
    max_depth: usize,
    cache: &GraphCache,
) -> Result<Vec<BlastRadiusEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = cache.get_or_build(project)?;
    let target = normalize_key(file_path);
    let mut result = HashMap::new();
    let mut queue = VecDeque::from([(target.clone(), 0usize)]);

    while let Some((current, depth)) = queue.pop_front() {
        if depth > max_depth || result.contains_key(&current) {
            continue;
        }
        if current != target {
            result.insert(current.clone(), depth);
        }

        let Some(node) = graph.get(&current) else {
            continue;
        };
        for importer in &node.imported_by {
            if !result.contains_key(importer) {
                queue.push_back((importer.clone(), depth + 1));
            }
        }
    }

    let mut entries: Vec<_> = result
        .into_iter()
        .map(|(file, depth)| BlastRadiusEntry { file, depth })
        .collect();
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.file.cmp(&b.file)));
    Ok(entries)
}

pub fn get_importers(
    project: &ProjectRoot,
    file_path: &str,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<ImporterEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = cache.get_or_build(project)?;
    let target = normalize_key(file_path);
    let importers = graph
        .get(&target)
        .map(|node| {
            let mut entries = node
                .imported_by
                .iter()
                .cloned()
                .map(|file| ImporterEntry { file })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.file.cmp(&b.file));
            if max_results > 0 && entries.len() > max_results {
                entries.truncate(max_results);
            }
            entries
        })
        .unwrap_or_default();
    Ok(importers)
}

/// PageRank over the import graph (damping=0.85, 20 iterations).
fn compute_pagerank(graph: &HashMap<String, FileNode>) -> HashMap<String, f64> {
    if graph.is_empty() {
        return HashMap::new();
    }
    let damping = 0.85;
    let n = graph.len() as f64;
    let mut scores: HashMap<String, f64> = graph.keys().cloned().map(|k| (k, 1.0 / n)).collect();
    let out_degree: HashMap<&str, usize> = graph
        .iter()
        .map(|(k, node)| (k.as_str(), node.imports.len()))
        .collect();
    for _ in 0..20 {
        let mut next: HashMap<String, f64> = HashMap::new();
        for (key, node) in graph.iter() {
            let mut incoming = 0.0;
            for importer in &node.imported_by {
                let importer_score = scores.get(importer).copied().unwrap_or(0.0);
                let degree = out_degree
                    .get(importer.as_str())
                    .copied()
                    .unwrap_or(1)
                    .max(1) as f64;
                incoming += importer_score / degree;
            }
            next.insert(key.clone(), (1.0 - damping) / n + damping * incoming);
        }
        scores = next;
    }
    scores
}

pub fn get_importance(
    project: &ProjectRoot,
    top_n: usize,
    cache: &GraphCache,
) -> Result<Vec<ImportanceEntry>> {
    let graph = cache.get_or_build(project)?;
    let scores = compute_pagerank(&graph);

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut entries: Vec<_> = ranked
        .into_iter()
        .map(|(file, score)| ImportanceEntry {
            file,
            score: format!("{score:.4}"),
        })
        .collect();
    if top_n > 0 && entries.len() > top_n {
        entries.truncate(top_n);
    }
    Ok(entries)
}

pub fn find_dead_code(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntry>> {
    let graph = cache.get_or_build(project)?;
    let mut dead: Vec<_> = graph
        .iter()
        .filter(|(_, node)| node.imported_by.is_empty())
        .map(|(file, _)| DeadCodeEntry {
            file: file.clone(),
            symbol: None,
            reason: "no importers".to_owned(),
        })
        .collect();
    dead.sort_by(|a, b| a.file.cmp(&b.file));
    if max_results > 0 && dead.len() > max_results {
        dead.truncate(max_results);
    }
    Ok(dead)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntryV2 {
    pub file: String,
    pub symbol: Option<String>,
    pub kind: Option<String>,
    pub line: Option<usize>,
    pub reason: String,
    pub pass: u8,
}

/// Exception file names that should not be flagged as dead (entry points / init files).
fn is_entry_point_file(file: &str) -> bool {
    let name = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file);
    matches!(
        name,
        "__init__.py"
            | "mod.rs"
            | "lib.rs"
            | "main.rs"
            | "index.ts"
            | "index.js"
            | "index.tsx"
            | "index.jsx"
    )
}

/// Exception symbol names that should not be flagged as dead.
fn is_entry_point_symbol(name: &str) -> bool {
    name == "main"
        || name == "__init__"
        || name == "setUp"
        || name == "tearDown"
        || name.starts_with("test_")
        || name.starts_with("Test")
}

/// Check whether the line immediately before a symbol definition starts with `@`
/// (decorator pattern). `lines` is the 0-indexed source lines; `symbol_line` is
/// 1-indexed (as returned by tree-sitter / SymbolInfo).
fn has_decorator(lines: &[&str], symbol_line: usize) -> bool {
    if symbol_line < 2 {
        return false;
    }
    let prev_idx = symbol_line - 2; // convert to 0-indexed, then go one line back
    lines
        .get(prev_idx)
        .map(|l| l.trim_start().starts_with('@'))
        .unwrap_or(false)
}

pub fn find_dead_code_v2(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntryV2>> {
    let mut results: Vec<DeadCodeEntryV2> = Vec::new();

    // ── Pass 1: unreferenced files ────────────────────────────────────────────
    let graph = cache.get_or_build(project)?;
    for (file, node) in graph.iter() {
        if node.imported_by.is_empty() && !is_entry_point_file(file) {
            results.push(DeadCodeEntryV2 {
                file: file.clone(),
                symbol: None,
                kind: None,
                line: None,
                reason: "no importers".to_owned(),
                pass: 1,
            });
        }
    }

    // ── Pass 2: unreferenced symbols ─────────────────────────────────────────
    // Build a set of all callee names across the entire project using call_graph.
    let candidate_files = collect_candidate_files(project.as_path())?;
    let mut all_callees: HashSet<String> = HashSet::new();
    for path in &candidate_files {
        for edge in extract_calls(path) {
            all_callees.insert(edge.callee_name);
        }
    }

    // For each file, parse its symbols (via tree-sitter call graph func detection)
    // and check whether the symbol name appears as a callee anywhere.
    for path in &candidate_files {
        let relative = project.to_relative(path);

        // Skip files that are already flagged in pass 1 (no importers)
        if results.iter().any(|e| e.file == relative && e.pass == 1) {
            continue;
        }
        // Skip entry-point files
        if is_entry_point_file(&relative) {
            continue;
        }

        // Read source for decorator detection
        let source = fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = source.lines().collect();

        // Use call_graph's func extraction: we derive defined functions from call edges
        // by collecting all unique caller_name values within this file.
        let edges = extract_calls(path);
        let mut defined_funcs: HashMap<String, usize> = HashMap::new();
        for edge in &edges {
            // Use the first seen line for the function definition as a proxy.
            // We only have call-site lines here; use 0 as sentinel.
            defined_funcs.entry(edge.caller_name.clone()).or_insert(0);
        }
        // Also handle files that define functions but make no calls — we need a
        // separate pass with the func query. Re-use extract_calls which already
        // collects func_ranges internally; approximate by reading all unique callers.
        // For symbols with no outgoing calls we won't see them in edges; however
        // the call graph doesn't expose func_ranges directly. We use a lightweight
        // regex fallback to also catch top-level defs not in edges.
        collect_top_level_funcs(path, &source, &mut defined_funcs);

        for (func_name, func_line) in defined_funcs {
            if func_name == "<module>" {
                continue;
            }
            // Pass 3 exception filter
            if is_entry_point_symbol(&func_name) {
                continue;
            }
            if func_line > 0 && has_decorator(&lines, func_line) {
                continue;
            }
            if !all_callees.contains(&func_name) {
                results.push(DeadCodeEntryV2 {
                    file: relative.clone(),
                    symbol: Some(func_name),
                    kind: Some("function".to_owned()),
                    line: if func_line > 0 { Some(func_line) } else { None },
                    reason: "unreferenced symbol".to_owned(),
                    pass: 2,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        a.pass
            .cmp(&b.pass)
            .then(a.file.cmp(&b.file))
            .then(a.symbol.cmp(&b.symbol))
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}

/// Lightweight regex-based top-level function name extractor.
/// Fills `funcs` map with (name -> line_number). Does not overwrite existing entries.
fn collect_top_level_funcs(path: &Path, source: &str, funcs: &mut HashMap<String, usize>) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let regexes: &[&Regex] = match ext.as_str() {
        "py" => &[&*TLF_PY_RE],
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" => &[&*TLF_JS_RE1, &*TLF_JS_RE2],
        "go" => &[&*TLF_GO_RE],
        "java" | "kt" | "cs" => &[&*TLF_JVM_RE],
        "rs" => &[&*TLF_RS_RE],
        "dart" => &[&*TLF_PY_RE, &*TLF_JVM_RE],
        _ => return,
    };

    for re in regexes {
        for cap in re.captures_iter(source) {
            let Some(m) = cap.get(1) else { continue };
            let name = m.as_str().to_owned();
            if !name.is_empty() {
                // Derive approximate line number from byte offset
                let offset = m.start();
                let line = source[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
                funcs.entry(name).or_insert(line);
            }
        }
    }
}

/// Public accessor for the import graph, used by sibling modules (e.g. circular).
pub(crate) fn build_graph_pub(
    project: &ProjectRoot,
    cache: &GraphCache,
) -> Result<Arc<HashMap<String, FileNode>>> {
    cache.get_or_build(project)
}

fn build_graph(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    // Try to load from SQLite first
    let db_path = index_db_path(project.as_path());
    if db_path.is_file() {
        if let Ok(db) = IndexDb::open(&db_path) {
            if db.file_count()? > 0 {
                return build_graph_from_db(&db);
            }
        }
    }

    // Fallback: scan files directly
    build_graph_from_files(project)
}

fn build_graph_from_db(db: &IndexDb) -> Result<HashMap<String, FileNode>> {
    let db_graph = db.build_import_graph()?;
    let mut graph = HashMap::new();
    for (path, (imports, imported_by)) in db_graph {
        graph.insert(
            path,
            FileNode {
                imports: imports.into_iter().collect(),
                imported_by: imported_by.into_iter().collect(),
            },
        );
    }
    Ok(graph)
}

fn build_graph_from_files(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    let files = collect_candidate_files(project.as_path())?;
    let mut graph = HashMap::new();

    for file in &files {
        let rel = project.to_relative(file);
        let imports = extract_imports(file)
            .into_iter()
            .filter_map(|module| resolve_module(project, file, &module))
            .collect::<HashSet<_>>();
        graph.insert(
            rel.clone(),
            FileNode {
                imports,
                imported_by: HashSet::new(),
            },
        );
    }

    let edges: Vec<(String, String)> = graph
        .iter()
        .flat_map(|(from_file, node)| {
            node.imports
                .iter()
                .cloned()
                .map(|to_file| (from_file.clone(), to_file))
                .collect::<Vec<_>>()
        })
        .collect();

    for (from_file, to_file) in edges {
        if let Some(node) = graph.get_mut(&to_file) {
            node.imported_by.insert(from_file);
        }
    }

    Ok(graph)
}

fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| {
        crate::lang_registry::supports_imports_for_path(path)
    })
}

/// Extract raw import strings from a file. Public for use by the indexer.
pub fn extract_imports_for_file(path: &Path) -> Vec<String> {
    extract_imports(path)
}

/// Resolve a raw import string to a relative path within the project. Public for use by the indexer.
pub fn resolve_module_for_file(
    project: &ProjectRoot,
    source_file: &Path,
    module: &str,
) -> Option<String> {
    resolve_module(project, source_file, module)
}

fn extract_imports(path: &Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => extract_python_imports(&content),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => extract_js_imports(&content),
        "go" => extract_go_imports(&content),
        "java" => extract_java_imports(&content),
        "kt" | "kts" => extract_kotlin_imports(&content),
        "rs" => extract_rust_imports(&content),
        "rb" => extract_ruby_imports(&content),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => extract_c_imports(&content),
        "php" => extract_php_imports(&content),
        "cs" => extract_csharp_imports(&content),
        "dart" => extract_dart_imports(&content),
        _ => Vec::new(),
    }
}

fn extract_python_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for capture in PY_IMPORT_RE.captures_iter(content) {
        let Some(modules) = capture.get(1) else {
            continue;
        };
        for module in modules.as_str().split(',') {
            let module = module.trim().split_whitespace().next().unwrap_or_default();
            if !module.is_empty() {
                imports.push(module.to_owned());
            }
        }
    }
    for capture in PY_FROM_RE.captures_iter(content) {
        let Some(module) = capture.get(1) else {
            continue;
        };
        imports.push(module.as_str().trim().to_owned());
    }
    imports
}

fn extract_js_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for regex in [
        &*JS_IMPORT_FROM_RE,
        &*JS_IMPORT_SIDE_EFFECT_RE,
        &*JS_REQUIRE_RE,
        &*JS_DYNAMIC_IMPORT_RE,
    ] {
        for capture in regex.captures_iter(content) {
            let Some(module) = capture.get(1) else {
                continue;
            };
            imports.push(module.as_str().trim().to_owned());
        }
    }
    imports
}

fn extract_go_imports(content: &str) -> Vec<String> {
    // Handles: import "path" and import ( "path" ... )
    let mut imports = Vec::new();
    // Single-line imports
    for cap in GO_SINGLE_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            imports.push(m.as_str().to_owned());
        }
    }
    // Block imports: find import ( ... ) sections
    for section in GO_BLOCK_SECTION_RE.captures_iter(content) {
        if let Some(body) = section.get(1) {
            for cap in GO_BLOCK_RE.captures_iter(body.as_str()) {
                if let Some(m) = cap.get(1) {
                    imports.push(m.as_str().to_owned());
                }
            }
        }
    }
    imports
}

fn extract_java_imports(content: &str) -> Vec<String> {
    // import pkg.Class; and import static pkg.Class.method;
    JAVA_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_kotlin_imports(content: &str) -> Vec<String> {
    // import pkg.Class  and  import pkg.Class as Alias
    KT_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_rust_imports(content: &str) -> Vec<String> {
    // use crate::module;  pub use super::module;  pub mod module;
    // use crate::{A, B};  use crate::foo::{Bar, Baz};
    let mut imports = Vec::new();

    // mod declarations
    for cap in RS_MOD_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            imports.push(m.as_str().to_owned());
        }
    }

    // use statements (with optional brace group)
    for cap in RS_USE_RE.captures_iter(content) {
        let base = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(brace) = cap.get(2) {
            // use crate::foo::{A, B} → emit crate::foo::A, crate::foo::B
            for item in brace.as_str().split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    imports.push(format!("{base}::{item}"));
                }
            }
        } else if !base.is_empty() {
            imports.push(base.to_owned());
        }
    }
    imports
}

fn extract_ruby_imports(content: &str) -> Vec<String> {
    // require "file"  require_relative "file"  load "file"
    RB_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_c_imports(content: &str) -> Vec<String> {
    // #include "file.h"  and  #include <file.h>
    C_INCLUDE_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_php_imports(content: &str) -> Vec<String> {
    // use Namespace\Class;
    // require/include "file"; (with or without _once)
    let mut imports = Vec::new();
    for re in [&*PHP_USE_RE, &*PHP_REQ_RE] {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                imports.push(m.as_str().to_owned());
            }
        }
    }
    imports
}

fn extract_csharp_imports(content: &str) -> Vec<String> {
    CS_USING_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_owned()))
        .collect()
}

fn extract_dart_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for re in [&*DART_IMPORT_RE, &*DART_EXPORT_RE] {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let path = m.as_str();
                // Skip dart: SDK imports
                if !path.starts_with("dart:") {
                    imports.push(path.to_owned());
                }
            }
        }
    }
    imports
}

fn resolve_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_ext = source_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    match source_ext.as_str() {
        "py" => resolve_python_module(project, source_file, module),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            resolve_js_module(project, source_file, module)
        }
        "go" => resolve_go_module(project, module),
        "java" | "kt" => resolve_jvm_module(project, module),
        "rs" => resolve_rust_module(project, source_file, module),
        "rb" => resolve_ruby_module(project, source_file, module),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => {
            resolve_c_module(project, source_file, module)
        }
        "php" => resolve_php_module(project, source_file, module),
        "cs" => resolve_csharp_module(project, module),
        "dart" => resolve_dart_module(project, source_file, module),
        _ => None,
    }
}

fn resolve_python_module(
    project: &ProjectRoot,
    source_file: &Path,
    module: &str,
) -> Option<String> {
    let source_dir = source_file.parent()?;
    let module_path = module.replace('.', "/");
    if module.starts_with('.') {
        return None;
    }

    let local_candidates = [
        source_dir.join(format!("{module_path}.py")),
        source_dir.join(module_path.clone()).join("__init__.py"),
    ];
    for candidate in local_candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }

    let root_candidates = [
        project.as_path().join(format!("{module_path}.py")),
        project.as_path().join(module_path).join("__init__.py"),
    ];
    for candidate in root_candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn resolve_js_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    if !module.starts_with('.') && !module.starts_with('/') {
        return None;
    }

    let base = if module.starts_with('/') {
        project.as_path().join(module.trim_start_matches('/'))
    } else {
        source_file.parent()?.join(module)
    };
    let candidates = js_resolution_candidates(&base);
    for candidate in candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn js_resolution_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![base.to_path_buf()];
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs"];
    if base.extension().is_none() {
        for ext in extensions {
            candidates.push(base.with_extension(ext));
        }
        for ext in extensions {
            candidates.push(base.join(format!("index.{ext}")));
        }
    }
    candidates
}

/// Go: import paths are module-relative (e.g. "github.com/user/repo/pkg").
/// We try to locate a directory named after the last path component under the project root.
fn resolve_go_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let last = module.split('/').last().unwrap_or(module);
    // Look for <last>/*.go or <last>.go at project root
    let dir_candidate = project.as_path().join(last);
    if dir_candidate.is_dir() {
        // Return the directory as a representative path (first .go file)
        if let Ok(mut rd) = std::fs::read_dir(&dir_candidate) {
            while let Some(Ok(entry)) = rd.next() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("go") {
                    return Some(project.to_relative(entry.path()));
                }
            }
        }
    }
    let file_candidate = project.as_path().join(format!("{last}.go"));
    if file_candidate.is_file() {
        return Some(project.to_relative(file_candidate));
    }
    None
}

/// Java/Kotlin: convert fully-qualified class name to file path.
/// e.g. com.example.Foo -> com/example/Foo.java (or .kt)
fn resolve_jvm_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let path_part = module.replace('.', "/");
    for ext in ["java", "kt"] {
        let candidate = project.as_path().join(format!("{path_part}.{ext}"));
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
        // Also check common src layouts: src/main/java/... src/main/kotlin/...
        for prefix in ["src/main/java", "src/main/kotlin", "src"] {
            let candidate = project
                .as_path()
                .join(prefix)
                .join(format!("{path_part}.{ext}"));
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    }
    None
}

/// Find the `src/` directory of a workspace crate given the crate name (using underscores).
/// Scans `crates/*/Cargo.toml` and matches by normalizing `-` to `_` in the directory name.
fn find_workspace_crate_dir(project: &ProjectRoot, crate_name: &str) -> Option<PathBuf> {
    let crates_dir = project.as_path().join("crates");
    if !crates_dir.is_dir() {
        return None;
    }
    for entry in std::fs::read_dir(&crates_dir).ok()?.flatten() {
        let cargo_toml = entry.path().join("Cargo.toml");
        if cargo_toml.is_file() {
            let dir_name = entry.file_name().to_string_lossy().replace('-', "_");
            if dir_name == crate_name {
                return Some(entry.path().join("src"));
            }
        }
    }
    None
}

/// Rust: `use crate::foo::bar` -> look for src/foo/bar.rs or src/foo/bar/mod.rs.
///       `mod foo;` -> look for foo.rs or foo/mod.rs relative to source dir.
///       `use codelens_core::ProjectRoot` -> strip workspace crate prefix and look in that crate's src/.
fn resolve_rust_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let stripped = module
        .trim_start_matches("crate::")
        .trim_start_matches("super::")
        .trim_start_matches("self::");

    // Check if the first segment matches a known workspace crate name.
    // e.g. `codelens_core::ProjectRoot` → crate_name="codelens_core", rest="ProjectRoot"
    let segments: Vec<&str> = stripped.splitn(2, "::").collect();
    if segments.len() == 2 {
        let first_seg = segments[0];
        if let Some(crate_src) = find_workspace_crate_dir(project, first_seg) {
            let remaining = segments[1].replace("::", "/");
            let mut parts: Vec<&str> = remaining.split('/').collect();
            while !parts.is_empty() {
                let candidate_path = parts.join("/");
                for candidate in [
                    crate_src.join(format!("{candidate_path}.rs")),
                    crate_src.join(&candidate_path).join("mod.rs"),
                ] {
                    if candidate.is_file() {
                        return Some(project.to_relative(candidate));
                    }
                }
                parts.pop();
            }
        }
    }

    let path_part = stripped.replace("::", "/");

    // Try full path first, then progressively strip trailing segments
    // (last segment may be a type/function name, not a module)
    let mut parts: Vec<&str> = path_part.split('/').collect();
    while !parts.is_empty() {
        let candidate_path = parts.join("/");
        // Relative to source file directory (for mod declarations)
        if let Some(parent) = source_file.parent() {
            for candidate in [
                parent.join(format!("{candidate_path}.rs")),
                parent.join(&candidate_path).join("mod.rs"),
            ] {
                if candidate.is_file() {
                    return Some(project.to_relative(candidate));
                }
            }
        }
        // Relative to src/ at project root
        let src = project.as_path().join("src");
        for candidate in [
            src.join(format!("{candidate_path}.rs")),
            src.join(&candidate_path).join("mod.rs"),
        ] {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
        // Search within workspace crate directories (crates/*/src/)
        if let Ok(entries) = std::fs::read_dir(project.as_path().join("crates")) {
            for entry in entries.flatten() {
                let crate_src = entry.path().join("src");
                for candidate in [
                    crate_src.join(format!("{candidate_path}.rs")),
                    crate_src.join(&candidate_path).join("mod.rs"),
                ] {
                    if candidate.is_file() {
                        return Some(project.to_relative(candidate));
                    }
                }
            }
        }
        // Strip last segment (may be a type name, not a module)
        parts.pop();
    }
    None
}

/// Ruby: resolve require/require_relative paths to .rb files.
fn resolve_ruby_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    // require_relative paths are relative to the source file
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    let base = if module.starts_with('.') {
        source_dir.join(module)
    } else {
        project.as_path().join(module)
    };
    // Add .rb extension if not present
    let with_ext = if base.extension().is_some() {
        base.clone()
    } else {
        base.with_extension("rb")
    };
    if with_ext.is_file() {
        return Some(project.to_relative(with_ext));
    }
    if base.is_file() {
        return Some(project.to_relative(base));
    }
    None
}

/// C/C++: resolve #include "file.h" (relative includes) and <file.h> (system includes).
/// Only relative (quoted) includes can be resolved to project files.
fn resolve_c_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    // Try relative to source file first, then project root
    for base in [source_dir.join(module), project.as_path().join(module)] {
        if base.is_file() {
            return Some(project.to_relative(base));
        }
    }
    None
}

/// PHP: use Namespace\Class -> Namespace/Class.php; require/include "file"
fn resolve_php_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    // Namespace\Class style
    let by_namespace = module.replace('\\', "/");
    let source_dir = source_file.parent().unwrap_or(project.as_path());

    for base_dir in [source_dir, project.as_path()] {
        let with_php = if by_namespace.ends_with(".php") {
            base_dir.join(&by_namespace)
        } else {
            base_dir.join(format!("{by_namespace}.php"))
        };
        if with_php.is_file() {
            return Some(project.to_relative(with_php));
        }
        // Try as-is (for require "file.php" paths)
        let as_is = base_dir.join(&by_namespace);
        if as_is.is_file() {
            return Some(project.to_relative(as_is));
        }
    }
    None
}

fn resolve_csharp_module(project: &ProjectRoot, module: &str) -> Option<String> {
    // C# using: namespace-based, try mapping Namespace.Class to Namespace/Class.cs
    let as_path = module.replace('.', "/");
    let candidate = project.as_path().join(format!("{as_path}.cs"));
    if candidate.is_file() {
        return Some(project.to_relative(candidate));
    }
    // Try last segment as filename
    if let Some(last) = module.rsplit('.').next() {
        let candidate = project.as_path().join(format!("{last}.cs"));
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn resolve_dart_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    // package:foo/bar.dart or relative path
    if let Some(stripped) = module.strip_prefix("package:") {
        // package:my_app/src/service.dart → lib/src/service.dart
        if let Some(slash_pos) = stripped.find('/') {
            let rest = &stripped[slash_pos + 1..];
            let candidate = project.as_path().join("lib").join(rest);
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    } else {
        // Relative import
        let source_dir = source_file.parent().unwrap_or(project.as_path());
        let candidate = source_dir.join(module);
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn normalize_key(file_path: &str) -> String {
    file_path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        find_dead_code, get_blast_radius, get_importance, get_importers, supports_import_graph,
        GraphCache,
    };
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn calculates_python_blast_radius() {
        let dir = temp_project_dir("python");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let radius = get_blast_radius(&project, "models.py", 3, &cache).expect("blast radius");
        assert_eq!(
            radius,
            vec![
                super::BlastRadiusEntry {
                    file: "utils.py".to_owned(),
                    depth: 1,
                },
                super::BlastRadiusEntry {
                    file: "main.py".to_owned(),
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn calculates_typescript_blast_radius() {
        let dir = temp_project_dir("typescript");
        fs::create_dir_all(dir.join("lib")).expect("mkdir");
        fs::write(
            dir.join("app.ts"),
            "import { greet } from './lib/greet'\nconsole.log(greet())\n",
        )
        .expect("write app");
        fs::write(
            dir.join("lib/greet.ts"),
            "import { User } from './user'\nexport const greet = () => new User()\n",
        )
        .expect("write greet");
        fs::write(dir.join("lib/user.ts"), "export class User {}\n").expect("write user");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let radius = get_blast_radius(&project, "lib/user.ts", 3, &cache).expect("blast radius");
        assert_eq!(
            radius,
            vec![
                super::BlastRadiusEntry {
                    file: "lib/greet.ts".to_owned(),
                    depth: 1,
                },
                super::BlastRadiusEntry {
                    file: "app.ts".to_owned(),
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn reports_supported_extensions() {
        assert!(supports_import_graph("main.py"));
        assert!(supports_import_graph("main.ts"));
        assert!(supports_import_graph("Main.java"));
        assert!(supports_import_graph("main.go"));
        assert!(supports_import_graph("main.kt"));
        assert!(supports_import_graph("main.rs"));
        assert!(supports_import_graph("main.rb"));
        assert!(supports_import_graph("main.c"));
        assert!(supports_import_graph("main.cpp"));
        assert!(supports_import_graph("main.h"));
        assert!(supports_import_graph("main.php"));
        assert!(!supports_import_graph("main.swift"));
    }

    #[test]
    fn extracts_go_imports() {
        let content = r#"
package main

import "fmt"
import (
    "os"
    "path/filepath"
)
"#;
        let imports = super::extract_go_imports(content);
        assert!(imports.contains(&"fmt".to_owned()), "single import");
        assert!(imports.contains(&"os".to_owned()), "block import os");
        assert!(
            imports.contains(&"path/filepath".to_owned()),
            "block import path"
        );
    }

    #[test]
    fn extracts_java_imports() {
        let content = "import com.example.Foo;\nimport static com.example.Utils.helper;\n";
        let imports = super::extract_java_imports(content);
        assert!(imports.contains(&"com.example.Foo".to_owned()));
        assert!(imports.contains(&"com.example.Utils.helper".to_owned()));
    }

    #[test]
    fn extracts_kotlin_imports() {
        let content = "import com.example.Foo\nimport com.example.Bar as B\n";
        let imports = super::extract_kotlin_imports(content);
        assert!(imports.contains(&"com.example.Foo".to_owned()));
        assert!(imports.contains(&"com.example.Bar".to_owned()));
    }

    #[test]
    fn extracts_rust_imports() {
        let content = "use crate::utils;\nuse super::models;\nmod config;\n";
        let imports = super::extract_rust_imports(content);
        assert!(imports.contains(&"crate::utils".to_owned()));
        assert!(imports.contains(&"super::models".to_owned()));
        assert!(imports.contains(&"config".to_owned()));
    }

    #[test]
    fn extracts_rust_pub_mod_and_pub_use() {
        let content =
            "pub mod symbols;\npub(crate) mod db;\npub use crate::project::ProjectRoot;\n";
        let imports = super::extract_rust_imports(content);
        assert!(
            imports.contains(&"symbols".to_owned()),
            "pub mod should be captured"
        );
        assert!(
            imports.contains(&"db".to_owned()),
            "pub(crate) mod should be captured"
        );
        assert!(
            imports.contains(&"crate::project::ProjectRoot".to_owned()),
            "pub use should be captured"
        );
    }

    #[test]
    fn extracts_rust_brace_group_imports() {
        let content = "use crate::{symbols, db};\nuse crate::foo::{Bar, Baz};\n";
        let imports = super::extract_rust_imports(content);
        assert!(
            imports.contains(&"crate::symbols".to_owned()),
            "brace group item 1"
        );
        assert!(
            imports.contains(&"crate::db".to_owned()),
            "brace group item 2"
        );
        assert!(
            imports.contains(&"crate::foo::Bar".to_owned()),
            "nested brace 1"
        );
        assert!(
            imports.contains(&"crate::foo::Baz".to_owned()),
            "nested brace 2"
        );
    }

    #[test]
    fn extracts_ruby_imports() {
        let content = "require \"json\"\nrequire_relative \"../lib/helper\"\nload \"tasks.rb\"\n";
        let imports = super::extract_ruby_imports(content);
        assert!(imports.contains(&"json".to_owned()));
        assert!(imports.contains(&"../lib/helper".to_owned()));
        assert!(imports.contains(&"tasks.rb".to_owned()));
    }

    #[test]
    fn extracts_c_imports() {
        let content = "#include \"mylib.h\"\n#include <stdio.h>\n";
        let imports = super::extract_c_imports(content);
        assert!(imports.contains(&"mylib.h".to_owned()));
        assert!(imports.contains(&"stdio.h".to_owned()));
    }

    #[test]
    fn extracts_php_imports() {
        let content =
            "use App\\Http\\Controllers\\HomeController;\nrequire \"vendor/autoload.php\";\n";
        let imports = super::extract_php_imports(content);
        assert!(imports.contains(&"App\\Http\\Controllers\\HomeController".to_owned()));
        assert!(imports.contains(&"vendor/autoload.php".to_owned()));
    }

    #[test]
    fn returns_importers() {
        let dir = temp_project_dir("importers");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        fs::write(dir.join("utils.py"), "def greet():\n    return 1\n").expect("write utils");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let importers = get_importers(&project, "utils.py", 10, &cache).expect("importers");
        assert_eq!(
            importers,
            vec![
                super::ImporterEntry {
                    file: "main.py".to_owned(),
                },
                super::ImporterEntry {
                    file: "worker.py".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn returns_importance_ranking() {
        let dir = temp_project_dir("importance");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        fs::write(
            dir.join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let ranking = get_importance(&project, 10, &cache).expect("importance");
        assert!(!ranking.is_empty());
        assert_eq!(
            ranking.first().map(|it| it.file.as_str()),
            Some("models.py")
        );
        assert!(ranking.iter().all(|it| !it.score.is_empty()));
    }

    #[test]
    fn returns_dead_code_candidates() {
        let dir = temp_project_dir("dead-code");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(dir.join("utils.py"), "def greet():\n    return 1\n").expect("write utils");
        fs::write(dir.join("unused.py"), "def helper():\n    return 2\n").expect("write unused");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let dead = find_dead_code(&project, 10, &cache).expect("dead code");
        assert_eq!(
            dead,
            vec![
                super::DeadCodeEntry {
                    file: "main.py".to_owned(),
                    symbol: None,
                    reason: "no importers".to_owned(),
                },
                super::DeadCodeEntry {
                    file: "unused.py".to_owned(),
                    symbol: None,
                    reason: "no importers".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn resolves_cross_crate_workspace_imports() {
        // Simulate a workspace layout:
        //   <root>/crates/codelens-core/src/project.rs
        //   <root>/crates/codelens-mcp/src/main.rs  (imports codelens_core::project::ProjectRoot)
        let dir = temp_project_dir("cross-crate");
        let core_src = dir.join("crates").join("codelens-core").join("src");
        let mcp_src = dir.join("crates").join("codelens-mcp").join("src");
        fs::create_dir_all(&core_src).expect("mkdir core/src");
        fs::create_dir_all(&mcp_src).expect("mkdir mcp/src");

        // Write Cargo.toml stubs so find_workspace_crate_dir can identify each crate
        fs::write(
            dir.join("crates").join("codelens-core").join("Cargo.toml"),
            "[package]\nname = \"codelens-core\"\n",
        )
        .expect("write core Cargo.toml");
        fs::write(
            dir.join("crates").join("codelens-mcp").join("Cargo.toml"),
            "[package]\nname = \"codelens-mcp\"\n",
        )
        .expect("write mcp Cargo.toml");

        // Create the target module file
        fs::write(core_src.join("project.rs"), "pub struct ProjectRoot;\n")
            .expect("write project.rs");

        // Create main.rs that imports from codelens_core
        let main_rs = mcp_src.join("main.rs");
        fs::write(
            &main_rs,
            "use codelens_core::project::ProjectRoot;\nfn main() {}\n",
        )
        .expect("write main.rs");

        let project = ProjectRoot::new(&dir).expect("project");

        // Directly test the resolve function
        let resolved = super::resolve_module_for_file(
            &project,
            &main_rs,
            "codelens_core::project::ProjectRoot",
        );
        assert_eq!(
            resolved,
            Some("crates/codelens-core/src/project.rs".to_owned()),
            "cross-crate import should resolve to crates/codelens-core/src/project.rs"
        );
    }

    fn temp_project_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-import-graph-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        dir
    }
}
