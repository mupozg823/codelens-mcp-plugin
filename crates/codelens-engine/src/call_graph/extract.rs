use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

use super::js_imports::LocalBindingScope;
use super::language::call_language_for_path;
use super::noise::is_noise_callee_for_lang;
use super::types::CallEdge;

/// Cached compiled tree-sitter Query for call graph extraction.
/// Key: (canonical language key, query string pointer as usize).
type CallQueryCacheKey = (&'static str, usize);
type CallQueryCache = Mutex<HashMap<CallQueryCacheKey, Arc<Query>>>;

static CALL_QUERY_CACHE: LazyLock<CallQueryCache> = LazyLock::new(|| Mutex::new(HashMap::new()));
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
/// Parse a file and extract all call edges within each function.
pub fn extract_calls(path: &Path) -> Vec<CallEdge> {
    let Ok(source) = fs::read_to_string(path) else {
        return Vec::new();
    };
    extract_calls_from_source(path, &source)
}

fn collect_identifier_names(node: Node<'_>, source_bytes: &[u8], names: &mut HashSet<String>) {
    if node.kind() == "identifier" {
        if let Ok(name) = std::str::from_utf8(&source_bytes[node.start_byte()..node.end_byte()]) {
            let name = name.trim();
            if !name.is_empty() {
                names.insert(name.to_owned());
            }
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_identifier_names(child, source_bytes, names);
    }
}

fn collect_rust_closure_binding_scopes(
    node: Node<'_>,
    source_bytes: &[u8],
    scopes: &mut Vec<LocalBindingScope>,
) {
    if node.kind() == "closure_expression" {
        let mut names = HashSet::new();
        if let Some(parameters) = node.child_by_field_name("parameters") {
            collect_identifier_names(parameters, source_bytes, &mut names);
        }
        if !names.is_empty() {
            scopes.push(LocalBindingScope {
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                names,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_closure_binding_scopes(child, source_bytes, scopes);
    }
}

fn is_argument_identifier_capture(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            "arguments" | "argument_list" | "value_arguments" | "value_argument"
        )
    })
}

fn shadowed_by_rust_closure_binding(
    scopes: &[LocalBindingScope],
    start_byte: usize,
    end_byte: usize,
    name: &str,
) -> bool {
    scopes.iter().any(|scope| {
        scope.start_byte <= start_byte && scope.end_byte >= end_byte && scope.names.contains(name)
    })
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
    let rust_closure_binding_scopes = if config.language_key == "rs" {
        let mut scopes = Vec::new();
        collect_rust_closure_binding_scopes(tree.root_node(), source_bytes, &mut scopes);
        scopes
    } else {
        Vec::new()
    };

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
            if config.language_key == "rs"
                && is_argument_identifier_capture(cap.node)
                && shadowed_by_rust_closure_binding(
                    &rust_closure_binding_scopes,
                    start,
                    end,
                    &callee_name,
                )
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
