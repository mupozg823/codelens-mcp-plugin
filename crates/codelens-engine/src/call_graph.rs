use crate::project::ProjectRoot;
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use crate::import_graph::GraphCache;

/// Cached compiled tree-sitter Query for call graph extraction.
/// Key: (canonical language key, query string pointer as usize).
type CallQueryCacheKey = (&'static str, usize);
type CallQueryCache = Mutex<HashMap<CallQueryCacheKey, Arc<Query>>>;

static CALL_QUERY_CACHE: LazyLock<CallQueryCache> = LazyLock::new(|| Mutex::new(HashMap::new()));
static JS_IMPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\bimport\s+([^;]+?)\s+from\s+["']([^"']+)["']"#).expect("import regex")
});

fn cached_call_query(
    language_key: &'static str,
    language: &Language,
    query_str: &'static str,
) -> Option<Arc<Query>> {
    let key = (language_key, query_str.as_ptr() as usize);
    let mut cache = CALL_QUERY_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(q) = cache.get(&key) {
        return Some(Arc::clone(q));
    }
    let q = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(error) => {
            #[cfg(test)]
            {
                panic!("invalid call graph query: {error}");
            }
            #[cfg(not(test))]
            {
                let _ = error;
                return None;
            }
        }
    };
    let q = Arc::new(q);
    cache.insert(key, Arc::clone(&q));
    Some(q)
}

use crate::project::collect_files;

#[derive(Debug, Clone, Serialize)]
pub struct CallEdge {
    pub caller_file: String,
    pub caller_name: String,
    pub callee_name: String,
    pub line: usize,
    /// Resolved file where the callee is defined (None if unresolved).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_file: Option<String>,
    /// Confidence of the resolution (0.0–1.0). Higher = more certain.
    pub confidence: f64,
    /// Which resolution strategy succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_strategy: Option<&'static str>,
    #[serde(skip_serializing)]
    pub canonical_callee_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallerEntry {
    pub file: String,
    pub function: String,
    pub line: usize,
    /// Confidence that this caller actually calls the target (0.0–1.0).
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalleeEntry {
    pub name: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_file: Option<String>,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<&'static str>,
}

struct CallLanguageConfig {
    /// Stable language/cache key. JS and TS can share query text but not compiled queries.
    language_key: &'static str,
    language: Language,
    /// Query to find function definitions: captures @func.name
    func_query: &'static str,
    /// Query to find call sites: captures @callee
    call_query: &'static str,
}

#[derive(Debug, Clone)]
struct JSImportBinding {
    imported_name: Option<String>,
    resolved_file: Option<String>,
    external: bool,
}

type JSImportBindingIndex = HashMap<String, HashMap<String, JSImportBinding>>;

/// Resolve call graph config via the unified language registry.
/// Only a subset of languages have call graph queries defined.
/// Filter out common std/builtin method calls that add noise to the call graph.
/// Covers Rust std, Python builtins, JS/TS builtins, Go builtins, and Java/Kotlin stdlib.
pub(crate) fn is_noise_callee(name: &str) -> bool {
    matches!(
        name,
        // ── cross-language common ──
        "get" | "set" | "push" | "pop" | "len" | "from" | "into"
            | "map" | "filter" | "collect" | "contains" | "insert" | "remove"
            | "format" | "print" | "clone" | "default" | "next" | "read"
            | "write" | "open" | "close" | "keys" | "values" | "sort"
            | "reverse" | "find" | "replace" | "delete" | "add" | "clear"
            | "of" | "size" | "copy"
            // ── Rust std ──
            | "is_empty" | "to_string" | "to_owned" | "as_str" | "as_ref"
            | "unwrap" | "expect" | "ok" | "err" | "and_then" | "or_else"
            | "unwrap_or" | "unwrap_or_else" | "unwrap_or_default"
            | "iter" | "into_iter" | "take" | "skip"
            | "println" | "eprintln" | "drop" | "enter" | "lock" | "cloned"
            // ── Python builtins ──
            | "range" | "enumerate" | "zip" | "sorted" | "reversed"
            | "isinstance" | "issubclass" | "hasattr" | "getattr" | "setattr" | "delattr"
            | "type" | "super" | "str" | "int" | "float" | "bool"
            | "list" | "dict" | "tuple" | "frozenset" | "bytes" | "bytearray"
            | "repr" | "abs" | "min" | "max" | "sum" | "any" | "all"
            | "ord" | "chr" | "hex" | "oct" | "bin" | "hash" | "id"
            | "input" | "vars" | "dir" | "help" | "round"
            | "append" | "extend" | "update" | "items" | "join" | "split"
            | "strip" | "startswith" | "endswith" | "encode" | "decode"
            | "upper" | "lower"
            // ── JS/TS builtins ──
            | "log" | "warn" | "error" | "info" | "debug"
            | "toString" | "valueOf" | "JSON" | "parse" | "stringify" | "assign"
            | "entries" | "forEach" | "reduce" | "findIndex" | "some" | "every"
            | "includes" | "indexOf" | "slice" | "splice" | "concat"
            | "flat" | "flatMap" | "fill" | "isArray"
            | "Promise" | "resolve" | "reject" | "then" | "catch" | "finally"
            | "setTimeout" | "setInterval" | "clearTimeout" | "clearInterval"
            | "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "require"
            // ── Go builtins ──
            | "make" | "cap" | "panic" | "recover" | "real" | "imag" | "complex"
            | "Println" | "Printf" | "Sprintf" | "Fprintf" | "Errorf" | "New"
            // ── Java/Kotlin stdlib ──
            | "equals" | "hashCode" | "compareTo" | "getClass"
            | "notify" | "notifyAll" | "wait" | "isEmpty"
            | "addAll" | "containsKey" | "containsValue" | "put" | "putAll"
            | "entrySet" | "keySet" | "charAt" | "substring" | "trim"
            | "length" | "toArray" | "stream" | "asList"
    )
}

/// Language-aware noise filter. Rust `new` is a constructor, not noise.
pub(crate) fn is_noise_callee_for_lang(name: &str, lang: Option<&str>) -> bool {
    if lang == Some("rs") && name == "new" {
        return false;
    }
    is_noise_callee(name)
}

fn call_language_for_path(path: &Path) -> Option<CallLanguageConfig> {
    let lang_config = crate::lang_config::language_for_path(path)?;
    // Map canonical extension to call graph queries (not all languages support this)
    let (language_key, func_query, call_query) = match lang_config.extension {
        "py" => ("py", PYTHON_FUNC_QUERY, PYTHON_CALL_QUERY),
        "js" => ("js", JS_FUNC_QUERY, JS_JSX_CALL_QUERY),
        "ts" => ("ts", JS_FUNC_QUERY, JS_CALL_QUERY),
        "tsx" => ("tsx", JS_FUNC_QUERY, JS_JSX_CALL_QUERY),
        "go" => ("go", GO_FUNC_QUERY, GO_CALL_QUERY),
        "java" => ("java", JAVA_FUNC_QUERY, JAVA_CALL_QUERY),
        "kt" => ("kt", KOTLIN_FUNC_QUERY, KOTLIN_CALL_QUERY),
        "rs" => ("rs", RUST_FUNC_QUERY, RUST_CALL_QUERY),
        _ => return None,
    };
    Some(CallLanguageConfig {
        language_key,
        language: lang_config.language,
        func_query,
        call_query,
    })
}

fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| call_language_for_path(path).is_some())
}

fn is_import_sensitive_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default(),
        "js" | "jsx" | "ts" | "tsx"
    )
}

fn is_external_module_specifier(module: &str, resolved_file: Option<&String>) -> bool {
    resolved_file.is_none() && !module.starts_with('.') && !module.starts_with('/')
}

fn insert_js_binding(
    bindings: &mut HashMap<String, JSImportBinding>,
    local_name: &str,
    imported_name: Option<&str>,
    resolved_file: Option<&String>,
    external: bool,
) {
    let local_name = local_name.trim().trim_start_matches("type ").trim();
    if local_name.is_empty() {
        return;
    }
    bindings.insert(
        local_name.to_owned(),
        JSImportBinding {
            imported_name: imported_name
                .map(|value| value.trim().trim_start_matches("type ").to_owned()),
            resolved_file: resolved_file.cloned(),
            external,
        },
    );
}

fn parse_js_import_bindings(
    bindings: &mut HashMap<String, JSImportBinding>,
    clause: &str,
    resolved_file: Option<&String>,
    module: &str,
) {
    let clause = clause.trim().trim_start_matches("type ").trim();
    if clause.is_empty() {
        return;
    }
    let external = is_external_module_specifier(module, resolved_file);

    if let Some(stripped) = clause.strip_prefix("* as ") {
        insert_js_binding(bindings, stripped, Some("*"), resolved_file, external);
        return;
    }

    let mut default_part = clause;
    if let Some(start) = clause.find('{') {
        default_part = clause[..start].trim().trim_end_matches(',').trim();
        if let Some(end) = clause[start + 1..].find('}') {
            let named = &clause[start + 1..start + 1 + end];
            for item in named.split(',') {
                let item = item.trim().trim_start_matches("type ").trim();
                if item.is_empty() {
                    continue;
                }
                if let Some((imported, local)) = item.split_once(" as ") {
                    insert_js_binding(bindings, local, Some(imported), resolved_file, external);
                } else {
                    insert_js_binding(bindings, item, Some(item), resolved_file, external);
                }
            }
        }
    }

    if !default_part.is_empty() {
        insert_js_binding(bindings, default_part, None, resolved_file, external);
    }
}

fn build_js_import_binding_index(project: &ProjectRoot, files: &[PathBuf]) -> JSImportBindingIndex {
    let mut index = HashMap::new();
    for file in files {
        let relative = project.to_relative(file);
        if !is_import_sensitive_path(&relative) {
            continue;
        }
        let Ok(source) = fs::read_to_string(file) else {
            continue;
        };
        let mut bindings = HashMap::new();
        for capture in JS_IMPORT_FROM_RE.captures_iter(&source) {
            let Some(clause) = capture.get(1).map(|value| value.as_str()) else {
                continue;
            };
            let Some(module) = capture.get(2).map(|value| value.as_str()) else {
                continue;
            };
            let resolved_file = crate::import_graph::resolve_module_for_file(project, file, module);
            parse_js_import_bindings(&mut bindings, clause, resolved_file.as_ref(), module);
        }
        if !bindings.is_empty() {
            index.insert(relative, bindings);
        }
    }
    index
}

fn filter_external_import_edges(edges: &mut Vec<CallEdge>, import_bindings: &JSImportBindingIndex) {
    edges.retain(|edge| {
        import_bindings
            .get(&edge.caller_file)
            .and_then(|bindings| bindings.get(&edge.callee_name))
            .map(|binding| !binding.external)
            .unwrap_or(true)
    });
}

fn maybe_import_graph(
    project: &ProjectRoot,
    files: &[PathBuf],
    graph_cache: Option<&GraphCache>,
) -> Option<Arc<HashMap<String, crate::import_graph::FileNode>>> {
    let cache = graph_cache?;
    let needs_import_graph = files.iter().any(|file| {
        let relative = project.to_relative(file);
        crate::import_graph::supports_import_graph(&relative)
    });
    if !needs_import_graph {
        return None;
    }
    let mut graph = crate::import_graph::build_graph_pub(project, cache)
        .map(|graph| (*graph).clone())
        .unwrap_or_default();

    for file in files {
        let relative = project.to_relative(file);
        if !crate::import_graph::supports_import_graph(&relative) {
            continue;
        }
        let needs_patch = graph
            .get(&relative)
            .map(|node| node.imports.is_empty())
            .unwrap_or(true);
        if !needs_patch {
            continue;
        }

        let imports: HashSet<String> = crate::import_graph::extract_imports_for_file(file)
            .into_iter()
            .filter_map(|module| {
                crate::import_graph::resolve_module_for_file(project, file, &module)
            })
            .collect();
        let entry =
            graph
                .entry(relative.clone())
                .or_insert_with(|| crate::import_graph::FileNode {
                    imports: HashSet::new(),
                    imported_by: HashSet::new(),
                });
        entry.imports = imports.clone();

        for imported_file in imports {
            graph
                .entry(imported_file)
                .or_insert_with(|| crate::import_graph::FileNode {
                    imports: HashSet::new(),
                    imported_by: HashSet::new(),
                })
                .imported_by
                .insert(relative.clone());
        }
    }

    if graph.is_empty() {
        None
    } else {
        Some(Arc::new(graph))
    }
}

/// Parse a file and extract all call edges within each function.
pub fn extract_calls(path: &Path) -> Vec<CallEdge> {
    let Ok(source) = fs::read_to_string(path) else {
        return Vec::new();
    };
    extract_calls_from_source(path, &source)
}

/// Extract call edges from already-loaded source content (avoids re-reading disk).
pub fn extract_calls_from_source(path: &Path, source: &str) -> Vec<CallEdge> {
    let Some(config) = call_language_for_path(path) else {
        return Vec::new();
    };

    let mut parser = Parser::new();
    if parser.set_language(&config.language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let source_bytes = source.as_bytes();

    // Build a map: byte_range_start -> caller_name for each function definition.
    // We'll use this to find which function contains each call site.
    let Some(func_query) =
        cached_call_query(config.language_key, &config.language, config.func_query)
    else {
        return Vec::new();
    };
    let mut func_ranges: Vec<(usize, usize, String)> = Vec::new(); // (start, end, name)
    let mut func_cursor = QueryCursor::new();
    let mut func_matches = func_cursor.matches(&func_query, tree.root_node(), source_bytes);
    while let Some(m) = func_matches.next() {
        let mut def_range: Option<(usize, usize)> = None;
        let mut func_name: Option<String> = None;
        for cap in m.captures.iter() {
            let cap_name = &func_query.capture_names()[cap.index as usize];
            if *cap_name == "func.def" {
                def_range = Some((cap.node.start_byte(), cap.node.end_byte()));
            } else if *cap_name == "func.name" {
                let start = cap.node.start_byte();
                let end = cap.node.end_byte();
                func_name = std::str::from_utf8(&source_bytes[start..end])
                    .ok()
                    .map(|s| s.trim().to_owned());
            }
        }
        if let (Some((s, e)), Some(name)) = (def_range, func_name)
            && !name.is_empty()
        {
            func_ranges.push((s, e, name));
        }
    }

    // Parse call sites
    let Some(call_query) =
        cached_call_query(config.language_key, &config.language, config.call_query)
    else {
        return Vec::new();
    };
    let mut call_cursor = QueryCursor::new();
    let mut call_matches = call_cursor.matches(&call_query, tree.root_node(), source_bytes);
    let file_path = path.to_string_lossy().to_string();
    let mut edges = Vec::new();

    while let Some(m) = call_matches.next() {
        for cap in m.captures.iter() {
            let cap_name = &call_query.capture_names()[cap.index as usize];
            if *cap_name != "callee" {
                continue;
            }
            let start = cap.node.start_byte();
            let end = cap.node.end_byte();
            let Ok(callee_name) = std::str::from_utf8(&source_bytes[start..end]) else {
                continue;
            };
            let callee_name = callee_name.trim().to_owned();
            if callee_name.is_empty()
                || is_noise_callee_for_lang(&callee_name, Some(config.language_key))
            {
                continue;
            }
            let line = cap.node.start_position().row + 1;

            // Find the enclosing function
            let caller_name = func_ranges
                .iter()
                .filter(|(fs, fe, _)| *fs <= start && *fe >= end)
                // pick the innermost (smallest range)
                .min_by_key(|(fs, fe, _)| fe - fs)
                .map(|(_, _, name)| name.clone())
                .unwrap_or_else(|| "<module>".to_owned());

            edges.push(CallEdge {
                caller_file: file_path.clone(),
                caller_name,
                callee_name,
                line,
                resolved_file: None,
                confidence: 0.0,
                resolution_strategy: None,
                canonical_callee_name: None,
            });
        }
    }

    edges
}

// ── 6-stage call resolution cascade ──────────────────────────────────────

/// Resolve callee names to their definition files using a 6-stage confidence cascade.
/// Mutates edges in-place, setting resolved_file, confidence, and resolution_strategy.
fn resolve_call_edges(
    edges: &mut [CallEdge],
    project: &ProjectRoot,
    import_graph: Option<&HashMap<String, crate::import_graph::FileNode>>,
    import_bindings: Option<&JSImportBindingIndex>,
) {
    // Build a name→files index from the symbol DB for stages 3-5
    let db_path = crate::db::index_db_path(project.as_path());
    let symbol_index: HashMap<String, Vec<String>> = crate::db::IndexDb::open(&db_path)
        .and_then(|db| {
            let all = db.all_symbol_names()?;
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for (name, _kind, file, _line, _signature, _name_path) in all {
                map.entry(name).or_default().push(file);
            }
            Ok(map)
        })
        .unwrap_or_default();

    for edge in edges.iter_mut() {
        if edge.confidence > 0.0 {
            continue; // already resolved
        }

        let callee = &edge.callee_name;
        let caller_file = &edge.caller_file;

        // Stage 1: Same file — local definitions beat imported or project-wide matches (0.90)
        if let Some(defs) = symbol_index.get(callee)
            && defs.iter().any(|f| f == caller_file)
        {
            edge.resolved_file = Some(caller_file.clone());
            edge.confidence = 0.90;
            edge.resolution_strategy = Some("same_file");
            continue;
        }

        // Stage 2: Import map — imported target defines the callee (0.95)
        if let Some(binding) = import_bindings
            .and_then(|index| index.get(caller_file))
            .and_then(|bindings| bindings.get(callee))
            && let Some(resolved_file) = binding.resolved_file.as_ref()
        {
            let canonical_name = binding.imported_name.as_deref().unwrap_or(callee);
            if let Some(defs) = symbol_index.get(canonical_name)
                && defs.iter().any(|f| f == resolved_file)
            {
                edge.resolved_file = Some(resolved_file.clone());
                edge.confidence = 0.95;
                edge.resolution_strategy = Some("import_map");
                edge.canonical_callee_name = Some(canonical_name.to_owned());
                continue;
            }
        }

        if let Some(graph) = import_graph
            && let Some(node) = graph.get(caller_file)
        {
            for imported_file in &node.imports {
                // Check if imported file defines callee
                if let Some(defs) = symbol_index.get(callee)
                    && defs.iter().any(|f| f == imported_file)
                {
                    edge.resolved_file = Some(imported_file.clone());
                    edge.confidence = 0.95;
                    edge.resolution_strategy = Some("import_map");
                    edge.canonical_callee_name = Some(callee.clone());
                    break;
                }
            }
        }
        if edge.confidence > 0.0 {
            continue;
        }

        // Stage 3: Import suffix — imported module suffix points at the callee (0.70)
        if let Some(graph) = import_graph
            && let Some(node) = graph.get(caller_file)
            && let Some(defs) = symbol_index.get(callee)
        {
            // Pick the candidate that is also imported (transitively)
            for def_file in defs {
                if node.imports.iter().any(|imp| {
                    // Match on full path suffix, not just filename
                    def_file.ends_with(imp)
                        || def_file.ends_with(&format!("/{imp}"))
                        || imp.ends_with(def_file)
                        || imp.ends_with(&format!("/{def_file}"))
                }) {
                    edge.resolved_file = Some(def_file.clone());
                    edge.confidence = 0.70;
                    edge.resolution_strategy = Some("import_suffix");
                    edge.canonical_callee_name = Some(callee.clone());
                    break;
                }
            }
        }
        if edge.confidence > 0.0 {
            continue;
        }

        // Stage 4: Unique name — only one definition exists project-wide (0.65).
        // For JS/TS cross-file calls without import evidence, keep this as a fallback.
        if let Some(defs) = symbol_index.get(callee)
            && defs.len() == 1
        {
            edge.resolved_file = Some(defs[0].clone());
            if is_import_sensitive_path(caller_file) && defs[0].as_str() != caller_file.as_str() {
                edge.confidence = 0.50;
                edge.resolution_strategy = Some("path_proximity");
            } else {
                edge.confidence = 0.65;
                edge.resolution_strategy = Some("unique_name");
            }
            continue;
        }

        // Stage 5: Multiple candidates — pick closest by path similarity (0.50)
        if let Some(defs) = symbol_index.get(callee)
            && !defs.is_empty()
        {
            // Pick the one with the most shared path prefix with caller_file
            let best = defs
                .iter()
                .max_by_key(|f| {
                    f.chars()
                        .zip(caller_file.chars())
                        .take_while(|(a, b)| a == b)
                        .count()
                })
                .cloned();
            if let Some(f) = best {
                edge.resolved_file = Some(f);
                edge.confidence = 0.50;
                edge.resolution_strategy = Some("path_proximity");
                continue;
            }
        }

        // Stage 6: Unresolved — callee not found in symbol DB (0.25)
        edge.confidence = 0.25;
        edge.resolution_strategy = Some("unresolved");
    }
}

/// Find all functions that call `function_name` across the project.
/// Edges are resolved via the 6-stage confidence cascade when an import graph is available.
pub fn get_callers(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<CallerEntry>> {
    let files: Vec<PathBuf> = if let Some(fp) = file_path {
        vec![project.resolve(fp)?]
    } else {
        collect_candidate_files(project.as_path())?
    };
    let mut all_edges: Vec<CallEdge> = Vec::new();

    for file in &files {
        let mut edges = extract_calls(file);
        // Relativize caller_file paths
        for edge in &mut edges {
            edge.caller_file = project.to_relative(file);
        }
        all_edges.extend(edges);
    }

    let import_bindings = build_js_import_binding_index(project, &files);
    filter_external_import_edges(&mut all_edges, &import_bindings);
    let import_graph = maybe_import_graph(project, &files, graph_cache);
    resolve_call_edges(
        &mut all_edges,
        project,
        import_graph.as_deref(),
        Some(&import_bindings),
    );

    // Filter to edges calling our target
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();

    for edge in all_edges {
        if edge.callee_name == function_name
            || edge.canonical_callee_name.as_deref() == Some(function_name)
        {
            let key = (
                edge.caller_file.clone(),
                edge.caller_name.clone(),
                edge.line,
            );
            if seen.insert(key) {
                results.push(CallerEntry {
                    file: edge.caller_file,
                    function: edge.caller_name,
                    line: edge.line,
                    confidence: edge.confidence,
                    resolution: edge.resolution_strategy,
                });
            }
        }
    }

    // Sort by confidence descending
    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}

/// Find all functions called by `function_name` (optionally restricted to a file).
/// Callee names are resolved to their definition files via the 6-stage cascade.
pub fn get_callees(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<CalleeEntry>> {
    let files: Vec<PathBuf> = if let Some(fp) = file_path {
        let resolved = project.resolve(fp)?;
        vec![resolved]
    } else {
        collect_candidate_files(project.as_path())?
    };

    let mut all_edges: Vec<CallEdge> = Vec::new();
    for file in &files {
        let mut edges = extract_calls(file);
        for edge in &mut edges {
            edge.caller_file = project.to_relative(file);
        }
        all_edges.extend(edges);
    }

    let import_bindings = build_js_import_binding_index(project, &files);
    filter_external_import_edges(&mut all_edges, &import_bindings);
    let import_graph = maybe_import_graph(project, &files, graph_cache);
    resolve_call_edges(
        &mut all_edges,
        project,
        import_graph.as_deref(),
        Some(&import_bindings),
    );

    let mut seen: HashMap<(String, usize), ()> = HashMap::new();
    let mut results = Vec::new();

    for edge in all_edges {
        if edge.caller_name == function_name {
            let key = (edge.callee_name.clone(), edge.line);
            if seen.insert(key, ()).is_none() {
                results.push(CalleeEntry {
                    name: edge.callee_name,
                    line: edge.line,
                    resolved_file: edge.resolved_file,
                    confidence: edge.confidence,
                    resolution: edge.resolution_strategy,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}

// ---- Tree-sitter queries ----

const PYTHON_FUNC_QUERY: &str = r#"
(function_definition name: (identifier) @func.name) @func.def
"#;

const PYTHON_CALL_QUERY: &str = r#"
(call function: (identifier) @callee)
(call function: (attribute attribute: (identifier) @callee))
(decorator (identifier) @callee)
(decorator (call function: (identifier) @callee))
(decorator (attribute attribute: (identifier) @callee))
(decorator (call function: (attribute attribute: (identifier) @callee)))
;; v1.11.1 (F1 follow-up): function-reference arguments. Python
;; callback patterns include `register("evt", handler)`,
;; `dispatcher.on(name, callback)`, `signal.connect(slot)`, plus
;; decorator factories like `@retry(handler)`. The 6-stage
;; resolution cascade filters identifier-arg captures against the
;; project symbol DB; variable arguments fall to `unresolved` and
;; genuine function references resolve via Stage 5 (`unique_name`)
;; at confidence 0.5.
(call arguments: (argument_list (identifier) @callee))
(call arguments: (argument_list (attribute attribute: (identifier) @callee)))
"#;

const JS_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
(method_definition name: (property_identifier) @func.name) @func.def
(lexical_declaration
    (variable_declarator
    name: (identifier) @func.name
    value: [(arrow_function) (function_expression)] @func.def))
(variable_declaration
  (variable_declarator
    name: (identifier) @func.name
    value: [(arrow_function) (function_expression)] @func.def))
"#;

const JS_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (member_expression property: (property_identifier) @callee))
;; v1.11.1 (F1 follow-up): function-reference arguments. JS/TS frequently
;; pass functions as callbacks — `setTimeout(handler, 100)`,
;; `arr.map(parseLine)`, `bus.on("evt", onEvent)`, `.then(success)`.
;; The 6-stage resolution cascade in `resolve_call_edges` filters these
;; against the symbol DB, so variable arguments fall to `unresolved`
;; while genuine function references resolve via Stage 5
;; (`unique_name`) at confidence 0.5.
(arguments (identifier) @callee)
(arguments (member_expression property: (property_identifier) @callee))
"#;

// JSX/TSX adds React-style component usage (`<Foo />`, `<Foo>`) as caller→callee
// edges. Plain TypeScript (.ts) has no JSX node types — keep this off the JS/TS
// path. tree-sitter-javascript also supports JSX, so .jsx files share this set.
const JS_JSX_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (member_expression property: (property_identifier) @callee))
(jsx_self_closing_element name: (identifier) @callee)
(jsx_opening_element name: (identifier) @callee)
(jsx_self_closing_element name: (member_expression property: (property_identifier) @callee))
(jsx_opening_element name: (member_expression property: (property_identifier) @callee))
;; v1.11.1: same function-reference patterns as JS_CALL_QUERY.
(arguments (identifier) @callee)
(arguments (member_expression property: (property_identifier) @callee))
"#;

const GO_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
(method_declaration name: (field_identifier) @func.name) @func.def
"#;

const GO_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (selector_expression field: (field_identifier) @callee))
;; v1.11.2 (F1 follow-up): function-reference arguments in Go.
;; Catches `http.HandleFunc("/", handler)`, `time.AfterFunc(d, callback)`,
;; `runtime.SetFinalizer(p, finalizer)`, and worker-pool dispatch
;; patterns where a function value is passed by name. Same resolution
;; cascade gating: variable arguments fall to `unresolved`, named
;; functions resolve via Stage 5 (`unique_name`) at confidence 0.5.
(argument_list (identifier) @callee)
(argument_list (selector_expression field: (field_identifier) @callee))
"#;

const JAVA_FUNC_QUERY: &str = r#"
(method_declaration name: (identifier) @func.name) @func.def
(constructor_declaration name: (identifier) @func.name) @func.def
"#;

const JAVA_CALL_QUERY: &str = r#"
(method_invocation name: (identifier) @callee)
(object_creation_expression type: (type_identifier) @callee)
(method_reference (identifier) @callee)
;; v1.11.2 (F1 follow-up): function-reference arguments in Java/Kotlin
;; that are passed as bare identifiers (callbacks, executor.submit
;; targets) rather than the explicit `Class::method` reference syntax
;; already covered above. The same query is shared with Kotlin via
;; the `KOTLIN_FUNC_QUERY` mapping; tree-sitter-kotlin reuses
;; `argument_list` node names for the call grammar so the pattern
;; below applies to Kotlin call sites as well.
(method_invocation arguments: (argument_list (identifier) @callee))
(method_invocation arguments: (argument_list (field_access field: (identifier) @callee)))
"#;

const KOTLIN_FUNC_QUERY: &str = r#"
(function_declaration (identifier) @func.name) @func.def
"#;

const KOTLIN_CALL_QUERY: &str = r#"
;; Direct call: prepare()
(call_expression (identifier) @callee)

;; Method/navigation call: exec.submit(...) — last identifier in
;; navigation_expression is the method name (anchor `.` selects last child).
(call_expression
  (navigation_expression
    (identifier) @callee .))

;; v1.12.3: function-reference arguments — submit(onTick),
;; register("err", onError). Same noise-filter behavior as Rust:
;; non-function identifiers (variables) are dropped at resolution time.
(call_expression
  (value_arguments
    (value_argument
      (identifier) @callee)))

;; v1.12.4 (Codex P1): Kotlin callable references.
;; - bare form `::onTick` parses as
;;     value_argument > callable_reference > identifier.
;; - qualified form `this::onTick` parses as
;;     value_argument > navigation_expression(`::`) > identifier
;;   (tree-sitter-kotlin-ng folds the `::` token into a
;;   navigation_expression rather than a dedicated callable_reference
;;   node). Both shapes are common in Executor / event-bus callbacks.
(call_expression
  (value_arguments
    (value_argument
      (callable_reference (identifier) @callee))))

(call_expression
  (value_arguments
    (value_argument
      (navigation_expression (identifier) @callee .))))
"#;

const RUST_FUNC_QUERY: &str = r#"
(function_item name: (identifier) @func.name) @func.def
"#;

const RUST_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (field_expression field: (field_identifier) @callee))
(call_expression function: (scoped_identifier name: (identifier) @callee))
(macro_invocation macro: (identifier) @callee)
(macro_invocation macro: (scoped_identifier name: (identifier) @callee))
;; v1.11.0 (F1): function-reference patterns. A function passed as an
;; argument (closure construction, callback registration, builder
;; accumulators) is a real caller→callee edge that the call_expression
;; rules above miss. Examples:
;;   LazyLock::new(build_tools)
;;   OnceCell::get_or_init(make_state)
;;   iter.map(parse_line).collect()
;;   bus.register("evt", on_event)
;; Many argument identifiers are variables, not functions. The
;; resolution cascade in `resolve_call_edges` filters those: the name
;; must exist in the symbol DB or the edge is dropped as `unresolved`
;; (confidence 0). Genuine function references resolve via Stage 5
;; (unique_name) at confidence 0.5 — honest, lower than import_map but
;; higher than nothing.
(arguments (identifier) @callee)
(arguments (scoped_identifier name: (identifier) @callee))
"#;

#[cfg(test)]
mod tests {
    use super::{CallEdge, extract_calls, get_callees, get_callers, resolve_call_edges};
    use crate::GraphCache;
    use crate::ProjectRoot;
    use crate::db::{IndexDb, NewSymbol, index_db_path};
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-callgraph-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        dir
    }

    #[test]
    fn extracts_python_calls() {
        let dir = temp_dir("py");
        let path = dir.join("main.py");
        fs::write(
            &path,
            "def greet(name):\n    return helper(name)\n\ndef helper(x):\n    return x\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "greet" && e.callee_name == "helper"),
            "expected greet->helper edge, got {edges:?}"
        );
    }

    #[test]
    fn extracts_python_decorator_callers() {
        // Python decorator pattern is THE most common Flask/FastAPI/click usage.
        // tree-sitter call extractor previously missed it entirely (Flask: 1/292
        // recall on `route`). Decorators must be treated as caller→callee edges.
        let dir = temp_dir("py-deco");
        let path = dir.join("views.py");
        fs::write(
            &path,
            "from flask import Flask\napp = Flask(__name__)\n\
             @app.route('/')\ndef home():\n    return 'hi'\n\n\
             @app.route('/x')\ndef x_view():\n    return 'x'\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        let route_edges = edges.iter().filter(|e| e.callee_name == "route").count();
        assert!(
            route_edges >= 2,
            "expected at least 2 caller edges for `route` decorator, got {route_edges}: {edges:?}"
        );
    }

    #[test]
    fn extracts_jsx_component_callers() {
        // JSX <Component /> usage is THE core React pattern. Previously
        // tree-sitter call extractor missed it entirely (rg-family: 0/14
        // on `<Footer />`). JSX elements must be treated as caller→callee
        // edges to the component function.
        let dir = temp_dir("tsx");
        let path = dir.join("page.tsx");
        fs::write(
            &path,
            "import Footer from './Footer';\nimport { Button } from './ui';\n\
             export default function Page() {\n  return (<div><Footer />\n\
             <Button>OK</Button></div>);\n}\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        let footer_edges = edges.iter().filter(|e| e.callee_name == "Footer").count();
        let button_edges = edges.iter().filter(|e| e.callee_name == "Button").count();
        assert!(
            footer_edges >= 1,
            "expected at least 1 caller edge for `<Footer />`, got {footer_edges}: {edges:?}"
        );
        assert!(
            button_edges >= 1,
            "expected at least 1 caller edge for `<Button>`, got {button_edges}: {edges:?}"
        );
    }

    #[test]
    fn extracts_rust_calls() {
        let dir = temp_dir("rs");
        let path = dir.join("main.rs");
        fs::write(&path, "fn main() {\n    run();\n}\n\nfn run() {}\n").expect("write");
        let edges = extract_calls(&path);
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "main" && e.callee_name == "run"),
            "expected main->run edge, got {edges:?}"
        );
    }

    /// Rust macro invocations (`vec!`, `assert_eq!`, project-defined macros,
    /// scoped macros like `mycrate::log!`) are extremely common — but before
    /// 2026-04-26 they were silently dropped from the call graph because
    /// `macro_invocation` is a distinct AST node from `call_expression`.
    ///
    /// `println!` / `eprintln!` / `format!` / `print!` are intentionally
    /// filtered by `is_noise_callee` to keep std-debug lines out of the
    /// graph; the query DOES discover them but the noise filter drops them.
    /// Project-named macros and `vec!` / `assert_eq!` survive — those are
    /// the meaningful edges this PR unlocks.
    #[test]
    fn extracts_rust_macro_invocations_as_callers() {
        let dir = temp_dir("rs-macros");
        let path = dir.join("macros.rs");
        fs::write(
            &path,
            r#"macro_rules! my_log { ($($t:tt)*) => {} }
fn run() {
    let v = vec![1, 2, 3];
    assert_eq!(v.len(), 3);
    my_log!("hello");
}
"#,
        )
        .expect("write");
        let edges = extract_calls(&path);
        for expected in ["vec", "assert_eq", "my_log"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "run" && e.callee_name == expected),
                "expected run->{expected} macro edge, got {edges:?}"
            );
        }
    }

    /// Scoped macro invocations (`mycrate::my_macro!`). Uses project-named
    /// macros so they survive the std-noise filter.
    #[test]
    fn extracts_rust_scoped_macro_invocations() {
        let dir = temp_dir("rs-scoped-macros");
        let path = dir.join("scoped.rs");
        fs::write(
            &path,
            "fn run() {\n    mycrate::trace_event!(\"hi\");\n    helpers::record_metric!(42);\n}\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        for expected in ["trace_event", "record_metric"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "run" && e.callee_name == expected),
                "expected run->{expected} scoped macro edge, got {edges:?}"
            );
        }
    }

    #[test]
    fn extracts_js_arrow_function_callers() {
        let dir = temp_dir("js-arrow");
        let path = dir.join("handler.js");
        fs::write(
            &path,
            "const handleRequest = async (req) => {\n    validateUser(req);\n    service.run(req);\n};\nfunction validateUser(req) { return req; }\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "handleRequest" && e.callee_name == "validateUser"),
            "expected handleRequest->validateUser edge, got {edges:?}"
        );
    }

    /// Java `new Foo()` — `object_creation_expression`, NOT method_invocation.
    /// Before C-2 the constructor target was silently dropped; only the
    /// follow-up `.method()` call was captured.
    #[test]
    fn extracts_java_constructor_invocations() {
        let dir = temp_dir("java-ctor");
        let path = dir.join("App.java");
        fs::write(
            &path,
            "class App { void caller() { Foo f = new Foo(); Bar b = new Bar(1, 2); f.process(); } }\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        for expected in ["Foo", "Bar", "process"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "caller" && e.callee_name == expected),
                "expected caller->{expected} edge, got {edges:?}"
            );
        }
    }

    /// Java method references (`Foo::bar`). Modern Java + streams uses
    /// these heavily; pre-C-3 they emitted no edges because tree-sitter-java
    /// models `method_reference` as a distinct AST node from
    /// `method_invocation`. Uses non-noise method names so edges survive
    /// the std-noise filter (forEach/stream/map/println/toUpperCase are
    /// all in is_noise_callee).
    #[test]
    fn extracts_java_method_references() {
        let dir = temp_dir("java-mref");
        let path = dir.join("App.java");
        fs::write(
            &path,
            "class App { void caller(Bus b) { b.attach(Handler::dispatchEvent); b.subscribe(MyService::handleRequest); } }\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        for expected in ["attach", "dispatchEvent", "subscribe", "handleRequest"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "caller" && e.callee_name == expected),
                "expected caller->{expected} edge, got {edges:?}"
            );
        }
    }

    #[test]
    fn extracts_ts_typed_arrow_function_callers() {
        let dir = temp_dir("ts-arrow");
        let path = dir.join("handler.ts");
        fs::write(
            &path,
            "type Request = { userId: string };\nconst handleRequest = async (req: Request): Promise<Request> => {\n    return validateUser(req);\n};\nfunction validateUser(req: Request) { return req; }\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "handleRequest" && e.callee_name == "validateUser"),
            "expected handleRequest->validateUser edge, got {edges:?}"
        );
    }

    #[test]
    fn shared_js_ts_queries_do_not_cross_language_cache() {
        let dir = temp_dir("js-ts-cache");
        let js_path = dir.join("handler.js");
        let ts_path = dir.join("handler.ts");
        fs::write(
            &js_path,
            "const handleJs = () => {\n    validateJs();\n};\nfunction validateJs() {}\n",
        )
        .expect("write js");
        fs::write(
            &ts_path,
            "type Request = { userId: string };\nconst handleTs = (req: Request): Request => {\n    return validateTs(req);\n};\nfunction validateTs(req: Request) { return req; }\n",
        )
        .expect("write ts");

        let js_edges = extract_calls(&js_path);
        assert!(
            js_edges
                .iter()
                .any(|e| e.caller_name == "handleJs" && e.callee_name == "validateJs"),
            "expected handleJs->validateJs edge, got {js_edges:?}"
        );

        let ts_edges = extract_calls(&ts_path);
        assert!(
            ts_edges
                .iter()
                .any(|e| e.caller_name == "handleTs" && e.callee_name == "validateTs"),
            "expected handleTs->validateTs edge after JS extraction, got {ts_edges:?}"
        );
    }

    #[test]
    fn extracts_rust_scoped_function_calls() {
        let dir = temp_dir("rs-scoped");
        let path = dir.join("main.rs");
        fs::write(
            &path,
            "mod auth { pub fn verify() {} }\nfn handler() {\n    auth::verify();\n}\n",
        )
        .expect("write");
        let edges = extract_calls(&path);
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "handler" && e.callee_name == "verify"),
            "expected handler->verify edge, got {edges:?}"
        );
    }

    /// v1.11.0 (F1): function-reference callers — a function passed as an
    /// argument is a real caller→callee edge. Pre-v1.11.0 these were
    /// silently dropped because the tree-sitter call query only matched
    /// `call_expression`, not identifiers in argument position. The
    /// canonical cliff was the registry pattern in
    /// `codelens-mcp/src/tool_defs/build.rs`:
    /// `static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(build_tools);`
    /// where `get_callers("build_tools")` returned 0 callers.
    ///
    /// This test pins the regression by reproducing the same shape: a
    /// function used as a function-reference argument to `LazyLock::new`,
    /// and a closure-style `iter.map(parse_line)` reference. Both must
    /// surface as `<top>` callers (no enclosing fn) for the named
    /// callee.
    #[test]
    fn extracts_rust_function_reference_arguments() {
        let dir = temp_dir("rs-fn-refs");
        let path = dir.join("registry.rs");
        fs::write(
            &path,
            r#"
fn build_tools() -> Vec<u32> { vec![1, 2, 3] }
fn parse_line(s: &str) -> u32 { s.len() as u32 }

static TOOLS: std::sync::LazyLock<Vec<u32>> =
    std::sync::LazyLock::new(build_tools);

fn run() {
    let lines = ["a", "bb"];
    let parsed: Vec<_> = lines.iter().map(parse_line).collect();
    let _ = parsed;
}
"#,
        )
        .expect("write");
        let edges = extract_calls(&path);
        assert!(
            edges.iter().any(|e| e.callee_name == "build_tools"),
            "expected a function-reference caller for build_tools, got {edges:?}"
        );
        assert!(
            edges.iter().any(|e| e.callee_name == "parse_line"),
            "expected a function-reference caller for parse_line, got {edges:?}"
        );
    }

    /// v1.11.1 (F1 follow-up): JS/TS function-reference callbacks. The
    /// canonical patterns are `setTimeout(handler, 100)`,
    /// `arr.map(parseLine)`, `bus.on("evt", onEvent)`, `.then(success)`.
    /// Pre-v1.11.1 these were silently dropped because the JS call
    /// query only matched `call_expression`-position function nodes.
    #[test]
    fn extracts_js_function_reference_arguments() {
        let dir = temp_dir("js-fn-refs");
        let path = dir.join("callbacks.js");
        fs::write(
            &path,
            r#"
function parseLine(line) { return line.trim(); }
function onEvent(payload) { return payload; }
function timeoutHandler() { return 1; }

function setup() {
    const lines = ["a", "b"];
    const parsed = lines.map(parseLine);
    bus.on("evt", onEvent);
    setTimeout(timeoutHandler, 100);
    return parsed;
}
"#,
        )
        .expect("write");
        let edges = extract_calls(&path);
        for callee in ["parseLine", "onEvent", "timeoutHandler"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "setup" && e.callee_name == callee),
                "expected setup->{callee} function-reference edge, got {edges:?}"
            );
        }
    }

    /// v1.11.1: Python function-reference arguments — the
    /// `register("evt", handler)` and `dispatcher.on(name, callback)`
    /// shapes that callback-heavy Python code uses. Like the JS path,
    /// this depends on the resolution cascade filtering variable
    /// arguments against the symbol DB.
    #[test]
    fn extracts_python_function_reference_arguments() {
        let dir = temp_dir("py-fn-refs");
        let path = dir.join("registry.py");
        fs::write(
            &path,
            r#"
def parse_line(line):
    return line.strip()

def on_event(payload):
    return payload

def setup():
    register("evt", on_event)
    pipe = list(map(parse_line, ["a", "b"]))
    return pipe
"#,
        )
        .expect("write");
        let edges = extract_calls(&path);
        for callee in ["parse_line", "on_event"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "setup" && e.callee_name == callee),
                "expected setup->{callee} function-reference edge, got {edges:?}"
            );
        }
    }

    /// v1.11.2 (F1 follow-up): Go function-reference arguments. Common
    /// in HTTP server registration (`http.HandleFunc("/", handler)`),
    /// scheduler dispatch (`time.AfterFunc(d, fn)`), finalizers, and
    /// worker pools. Pre-v1.11.2, only the call-expression form was
    /// captured; the function-reference form was silently dropped.
    #[test]
    fn extracts_go_function_reference_arguments() {
        let dir = temp_dir("go-fn-refs");
        let path = dir.join("server.go");
        fs::write(
            &path,
            r#"package main

func handler(w int, r int) {}
func teardown() {}

func setup() {
    Register("/api", handler)
    Schedule(teardown)
}
"#,
        )
        .expect("write");
        let edges = extract_calls(&path);
        for callee in ["handler", "teardown"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "setup" && e.callee_name == callee),
                "expected setup->{callee} function-reference edge, got {edges:?}"
            );
        }
    }

    /// v1.11.2 (F1 follow-up): Java function-reference arguments —
    /// callbacks passed as bare identifiers (executor submit, listener
    /// registration) rather than via the explicit `Class::method`
    /// syntax that was already covered.
    #[test]
    fn extracts_java_function_reference_arguments() {
        let dir = temp_dir("java-fn-refs");
        let path = dir.join("Service.java");
        fs::write(
            &path,
            r#"public class Service {
    public void onTick() {}
    public void onError(String e) {}

    public void start(Executor exec, Bus bus) {
        exec.submit(onTick);
        bus.register("err", onError);
    }
}
"#,
        )
        .expect("write");
        let edges = extract_calls(&path);
        for callee in ["onTick", "onError"] {
            assert!(
                edges
                    .iter()
                    .any(|e| e.caller_name == "start" && e.callee_name == callee),
                "expected start->{callee} function-reference edge, got {edges:?}"
            );
        }
    }

    /// v1.11.0 (F1): false-positive guard. A bare variable passed as an
    /// argument (e.g., `f(local_var)`) is also an `(arguments
    /// (identifier))` shape, but `local_var` is not a function in the
    /// project symbol DB. The 6-stage resolution cascade should mark it
    /// `unresolved` (confidence 0). Without DB access we just verify
    /// the extractor doesn't blow up on this shape — resolution is
    /// covered by the integration tests in `codelens-mcp` that drive
    /// the whole pipeline.
    #[test]
    fn function_reference_extraction_is_resilient_to_variable_arguments() {
        let dir = temp_dir("rs-fn-ref-noise");
        let path = dir.join("noise.rs");
        fs::write(
            &path,
            r#"
fn outer(local_var: i32) {
    println!("v={}", local_var);
    let other = local_var + 1;
    consume(other);
}
fn consume(x: i32) -> i32 { x }
"#,
        )
        .expect("write");
        // Should not panic and should still find the direct call to consume.
        let edges = extract_calls(&path);
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "outer" && e.callee_name == "consume"),
            "direct call edge outer->consume must survive function-reference extraction, got {edges:?}"
        );
    }

    #[test]
    fn get_callers_finds_callers() {
        let dir = temp_dir("callers");
        fs::write(dir.join("a.py"), "def foo():\n    bar()\n    baz()\n").expect("write a");
        fs::write(dir.join("b.py"), "def qux():\n    bar()\n").expect("write b");
        fs::write(dir.join("c.py"), "def bar():\n    pass\n").expect("write c");

        let project = ProjectRoot::new(&dir).expect("project");
        let callers = get_callers(&project, "bar", None, 50, None).expect("callers");
        let names: Vec<&str> = callers.iter().map(|c| c.function.as_str()).collect();
        assert!(
            names.contains(&"foo"),
            "expected foo as caller, got {names:?}"
        );
        assert!(
            names.contains(&"qux"),
            "expected qux as caller, got {names:?}"
        );
    }

    #[test]
    fn get_callees_finds_callees() {
        let dir = temp_dir("callees");
        fs::write(
            dir.join("main.py"),
            "def main():\n    foo()\n    bar()\n\ndef foo():\n    pass\n\ndef bar():\n    pass\n",
        )
        .expect("write");

        let project = ProjectRoot::new(&dir).expect("project");
        let callees = get_callees(&project, "main", None, 50, None).expect("callees");
        let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"foo"),
            "expected foo as callee, got {names:?}"
        );
        assert!(
            names.contains(&"bar"),
            "expected bar as callee, got {names:?}"
        );
    }

    #[test]
    fn get_callees_resolves_definition_file_path() {
        let dir = temp_dir("callees-file-path");
        fs::write(dir.join("main.py"), "def main():\n    helper()\n").expect("write main");
        fs::write(dir.join("helpers.py"), "def helper():\n    pass\n").expect("write helper");
        let db = IndexDb::open(&index_db_path(&dir)).expect("db");
        let helper_file = db
            .upsert_file("helpers.py", 100, "helpers", 24, Some("py"))
            .expect("helpers file");
        db.insert_symbols(
            helper_file,
            &[NewSymbol {
                name: "helper",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 24,
                signature: "def helper():",
                name_path: "helper",
                parent_id: None,
            }],
        )
        .expect("helper symbol");

        let project = ProjectRoot::new(&dir).expect("project");
        let callees = get_callees(&project, "main", Some("main.py"), 50, None).expect("callees");
        let helper = callees
            .iter()
            .find(|callee| callee.name == "helper")
            .expect("helper callee");

        assert_eq!(helper.resolved_file.as_deref(), Some("helpers.py"));
    }

    #[test]
    fn ts_cross_file_unique_resolution_is_fallback_without_import_evidence() {
        let dir = temp_dir("ts-cross-file-unique");
        fs::write(
            dir.join("page.tsx"),
            "export function Page() { handleSubmit(); }\n",
        )
        .expect("write page");
        fs::create_dir_all(dir.join("components")).expect("components");
        fs::write(
            dir.join("components").join("CommentSection.tsx"),
            "export function handleSubmit() {}\n",
        )
        .expect("write component");
        let db = IndexDb::open(&index_db_path(&dir)).expect("db");
        let file_id = db
            .upsert_file(
                "components/CommentSection.tsx",
                100,
                "component",
                34,
                Some("tsx"),
            )
            .expect("component file");
        db.insert_symbols(
            file_id,
            &[NewSymbol {
                name: "handleSubmit",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 34,
                signature: "export function handleSubmit() {}",
                name_path: "handleSubmit",
                parent_id: None,
            }],
        )
        .expect("component symbol");

        let project = ProjectRoot::new(&dir).expect("project");
        let mut edges = vec![CallEdge {
            caller_file: "page.tsx".to_owned(),
            caller_name: "Page".to_owned(),
            callee_name: "handleSubmit".to_owned(),
            line: 1,
            resolved_file: None,
            confidence: 0.0,
            resolution_strategy: None,
            canonical_callee_name: None,
        }];

        resolve_call_edges(&mut edges, &project, None, None);

        assert_eq!(
            edges[0].resolved_file.as_deref(),
            Some("components/CommentSection.tsx")
        );
        assert_eq!(edges[0].resolution_strategy, Some("path_proximity"));
        assert!(edges[0].confidence <= 0.60);
    }

    #[test]
    fn get_callees_scoped_to_file() {
        let dir = temp_dir("callees-file");
        fs::write(dir.join("a.py"), "def process():\n    helper()\n").expect("write a");
        fs::write(dir.join("b.py"), "def process():\n    other()\n").expect("write b");

        let project = ProjectRoot::new(&dir).expect("project");
        let callees = get_callees(&project, "process", Some("a.py"), 50, None).expect("callees");
        let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"helper"), "expected helper, got {names:?}");
        assert!(!names.contains(&"other"), "should not have other from b.py");
    }

    #[test]
    fn get_callers_scoped_to_file() {
        let dir = temp_dir("callers-file");
        fs::write(dir.join("a.py"), "def foo():\n    bar()\n").expect("write a");
        fs::write(dir.join("b.py"), "def qux():\n    bar()\n").expect("write b");
        fs::write(dir.join("c.py"), "def bar():\n    pass\n").expect("write c");

        let project = ProjectRoot::new(&dir).expect("project");
        let callers = get_callers(&project, "bar", Some("a.py"), 50, None).expect("callers");
        let names: Vec<&str> = callers.iter().map(|c| c.function.as_str()).collect();
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn ts_cross_file_resolution_prefers_import_evidence() {
        let dir = temp_dir("ts-import-map");
        fs::write(
            dir.join("page.tsx"),
            "import { handleSubmit } from \"./actions\";\nexport function Page() { handleSubmit(); }\n",
        )
        .expect("write page");
        fs::write(
            dir.join("actions.ts"),
            "export function handleSubmit() {}\n",
        )
        .expect("write actions");
        let db = IndexDb::open(&index_db_path(&dir)).expect("db");
        let file_id = db
            .upsert_file("actions.ts", 100, "actions", 34, Some("ts"))
            .expect("actions file");
        db.insert_symbols(
            file_id,
            &[NewSymbol {
                name: "handleSubmit",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 34,
                signature: "export function handleSubmit() {}",
                name_path: "handleSubmit",
                parent_id: None,
            }],
        )
        .expect("action symbol");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let callees =
            get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
        let submit = callees
            .iter()
            .find(|callee| callee.name == "handleSubmit")
            .expect("handleSubmit callee");
        assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
        assert!(
            matches!(submit.resolution, Some("import_map" | "import_suffix")),
            "expected import evidence resolution, got {:?}",
            submit.resolution
        );
    }

    #[test]
    fn same_file_beats_import_match() {
        let dir = temp_dir("same-file-over-import");
        fs::write(
            dir.join("page.ts"),
            "import { helper } from \"./helpers\";\nfunction helper() {}\nexport function main() { helper(); }\n",
        )
        .expect("write page");
        fs::write(dir.join("helpers.ts"), "export function helper() {}\n").expect("write helpers");
        let db = IndexDb::open(&index_db_path(&dir)).expect("db");
        let page_file = db
            .upsert_file("page.ts", 100, "page", 92, Some("ts"))
            .expect("page file");
        let helpers_file = db
            .upsert_file("helpers.ts", 100, "helpers", 28, Some("ts"))
            .expect("helpers file");
        db.insert_symbols(
            page_file,
            &[NewSymbol {
                name: "helper",
                kind: "function",
                line: 2,
                column_num: 0,
                start_byte: 37,
                end_byte: 57,
                signature: "function helper() {}",
                name_path: "helper",
                parent_id: None,
            }],
        )
        .expect("page helper symbol");
        db.insert_symbols(
            helpers_file,
            &[NewSymbol {
                name: "helper",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 28,
                signature: "export function helper() {}",
                name_path: "helper",
                parent_id: None,
            }],
        )
        .expect("imported helper symbol");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let callees =
            get_callees(&project, "main", Some("page.ts"), 50, Some(&cache)).expect("callees");
        let helper = callees
            .iter()
            .find(|callee| callee.name == "helper")
            .expect("helper callee");
        assert_eq!(helper.resolved_file.as_deref(), Some("page.ts"));
        assert_eq!(helper.resolution, Some("same_file"));
    }

    #[test]
    fn ts_import_alias_resolves_and_callers_match_canonical_name() {
        let dir = temp_dir("ts-import-alias");
        fs::write(
            dir.join("page.tsx"),
            "import { handleSubmit as onSubmit } from \"./actions\";\nexport function Page() { onSubmit(); }\n",
        )
        .expect("write page");
        fs::write(
            dir.join("actions.ts"),
            "export function handleSubmit() {}\n",
        )
        .expect("write actions");
        let db = IndexDb::open(&index_db_path(&dir)).expect("db");
        let file_id = db
            .upsert_file("actions.ts", 100, "actions", 34, Some("ts"))
            .expect("actions file");
        db.insert_symbols(
            file_id,
            &[NewSymbol {
                name: "handleSubmit",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 34,
                signature: "export function handleSubmit() {}",
                name_path: "handleSubmit",
                parent_id: None,
            }],
        )
        .expect("action symbol");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let callees =
            get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
        let submit = callees
            .iter()
            .find(|callee| callee.name == "onSubmit")
            .expect("aliased callee");
        assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
        assert_eq!(submit.resolution, Some("import_map"));

        let callers =
            get_callers(&project, "handleSubmit", None, 50, Some(&cache)).expect("callers");
        let page = callers
            .iter()
            .find(|caller| caller.function == "Page")
            .expect("Page caller");
        assert_eq!(page.file, "page.tsx");
    }

    #[test]
    fn ts_external_import_calls_are_filtered_from_project_graph() {
        let dir = temp_dir("ts-external-import-filter");
        fs::write(
            dir.join("page.tsx"),
            "import { useState } from \"react\";\nimport { handleSubmit } from \"./actions\";\nexport function Page() { useState(); handleSubmit(); }\n",
        )
        .expect("write page");
        fs::write(
            dir.join("actions.ts"),
            "export function handleSubmit() {}\n",
        )
        .expect("write actions");
        let db = IndexDb::open(&index_db_path(&dir)).expect("db");
        let file_id = db
            .upsert_file("actions.ts", 100, "actions", 34, Some("ts"))
            .expect("actions file");
        db.insert_symbols(
            file_id,
            &[NewSymbol {
                name: "handleSubmit",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 34,
                signature: "export function handleSubmit() {}",
                name_path: "handleSubmit",
                parent_id: None,
            }],
        )
        .expect("action symbol");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let callees =
            get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
        assert!(
            callees.iter().any(|callee| callee.name == "handleSubmit"),
            "expected internal imported callee in {callees:?}"
        );
        assert!(
            !callees.iter().any(|callee| callee.name == "useState"),
            "external imported binding should not appear in project call graph: {callees:?}"
        );
    }

    #[test]
    fn get_callers_finds_rust_new_constructor() {
        let dir = temp_dir("rs-callers-new");
        fs::write(
            dir.join("lib.rs"),
            r#"pub struct Foo;
impl Foo {
    pub fn new() -> Self { Self }
}

pub fn make_foo() -> Foo {
    Foo::new()
}

pub fn make_another() -> Foo {
    Self::new()
}
"#,
        )
        .expect("write lib.rs");

        let project = ProjectRoot::new(&dir).expect("project");
        let callers = get_callers(&project, "new", None, 50, None).expect("callers");
        let names: Vec<&str> = callers.iter().map(|c| c.function.as_str()).collect();
        assert!(
            names.contains(&"make_foo"),
            "expected make_foo as caller of new, got {names:?}"
        );
        assert!(
            names.contains(&"make_another"),
            "expected make_another as caller of new, got {names:?}"
        );
    }
}
