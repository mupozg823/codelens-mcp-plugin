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

fn rust_function_declaration_path(
    def_node: Node<'_>,
    source_bytes: &[u8],
    name: &str,
) -> Option<String> {
    let mut ancestor = def_node.parent();
    while let Some(node) = ancestor {
        if node.kind() == "impl_item" {
            let owner = node.child_by_field_name("type")?;
            let owner = std::str::from_utf8(&source_bytes[owner.start_byte()..owner.end_byte()])
                .ok()?
                .trim();
            return (!owner.is_empty()).then(|| format!("{owner}/{name}"));
        }
        ancestor = node.parent();
    }
    None
}

/// The grammar call extraction parses `path` with, if any.
pub(crate) fn call_tree_language(path: &Path) -> Option<Language> {
    call_language_for_path(path).map(|config| config.language)
}

/// Extract call edges from already-loaded source content (avoids re-reading disk).
pub fn extract_calls_from_source(path: &Path, source: &str) -> Vec<CallEdge> {
    extract_located_calls_from_source(path, source)
        .into_iter()
        .filter(|located| !located.noise_name)
        .map(|located| located.edge)
        .collect()
}

/// Source token retained for declaration-identity queries. The legacy payload
/// intentionally stays line-based, including its historical deduplication.
pub(crate) struct LocatedCallEdge {
    pub edge: CallEdge,
    pub start_byte: usize,
    pub end_byte: usize,
    pub column: usize,
    pub caller: LocatedCaller,
    /// The callee name is on the name-only builtin noise list. The legacy API
    /// drops these; exact-identity callers resolve first and decide.
    pub noise_name: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocatedCaller {
    Module,
    Declaration {
        kind: &'static str,
        start_byte: usize,
        end_byte: usize,
        name_start_byte: usize,
        name_end_byte: usize,
    },
    Unsupported {
        kind: &'static str,
    },
}

/// Keep the structural caller boundary separately from the legacy name-only
/// query. In particular, computed/string members are absent from JS_FUNC_QUERY.
/// Anonymous callback bodies retain their named lexical outer as before, but
/// a member boundary must never borrow that outer declaration's identity.
///
/// `ancestors` runs from the root to the call site's parent.
fn js_located_caller(ancestors: &[Node<'_>]) -> LocatedCaller {
    let mut unbound_function = false;
    for (index, &node) in ancestors.iter().enumerate().rev() {
        let declaration = match node.kind() {
            "method_definition"
            | "public_field_definition"
            | "field_definition"
            | "class_static_block"
            | "generator_function_declaration" => {
                return LocatedCaller::Unsupported { kind: node.kind() };
            }
            "function_declaration" => Some(node),
            "arrow_function" | "function_expression" | "generator_function" => {
                let parent = index.checked_sub(1).map(|parent| ancestors[parent]);
                if parent.is_some_and(|parent| parent.kind() == "variable_declarator") {
                    parent
                } else {
                    unbound_function = true;
                    None
                }
            }
            "pair" | "assignment_expression" | "variable_declarator" if unbound_function => {
                return LocatedCaller::Unsupported { kind: node.kind() };
            }
            _ => None,
        };
        if let Some(declaration) = declaration {
            let Some(name) = declaration.child_by_field_name("name") else {
                return LocatedCaller::Unsupported {
                    kind: declaration.kind(),
                };
            };
            if name.kind() != "identifier" {
                return LocatedCaller::Unsupported {
                    kind: declaration.kind(),
                };
            }
            return LocatedCaller::Declaration {
                kind: declaration.kind(),
                start_byte: declaration.start_byte(),
                end_byte: declaration.end_byte(),
                name_start_byte: name.start_byte(),
                name_end_byte: name.end_byte(),
            };
        }
    }
    LocatedCaller::Module
}

/// [`js_located_caller`] for every node in `callsites`, in one walk of the
/// tree. `Node::parent` re-descends from the root, so walking parents from
/// each call site cost depth squared per call in nested bundled code.
fn js_located_callers(root: Node<'_>, callsites: &HashSet<usize>) -> HashMap<usize, LocatedCaller> {
    let mut callers = HashMap::with_capacity(callsites.len());
    if callsites.is_empty() {
        return callers;
    }
    let mut cursor = root.walk();
    let mut ancestors: Vec<Node<'_>> = Vec::new();
    loop {
        let node = cursor.node();
        if callsites.contains(&node.id()) {
            callers.insert(node.id(), js_located_caller(&ancestors));
        }
        if cursor.goto_first_child() {
            ancestors.push(node);
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return callers;
            }
            ancestors.pop();
        }
    }
}

pub(crate) fn extract_located_calls_from_source(path: &Path, source: &str) -> Vec<LocatedCallEdge> {
    extract_located_calls_with_tree(path, source, None)
}

/// [`extract_located_calls_from_source`] reusing `parsed` when it is a parse
/// of `source` with this path's call grammar ([`call_tree_language`]).
pub(crate) fn extract_located_calls_with_tree(
    path: &Path,
    source: &str,
    parsed: Option<&tree_sitter::Tree>,
) -> Vec<LocatedCallEdge> {
    let Some(config) = call_language_for_path(path) else {
        return Vec::new();
    };
    let own_tree;
    let tree = match parsed {
        Some(tree) => tree,
        None => {
            let mut parser = Parser::new();
            if parser.set_language(&config.language).is_err() {
                return Vec::new();
            }
            let Some(tree) = parser.parse(source, None) else {
                return Vec::new();
            };
            own_tree = tree;
            &own_tree
        }
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
    let mut func_ranges: Vec<(usize, usize, String, Option<String>)> = Vec::new();
    let mut func_cursor = QueryCursor::new();
    let mut func_matches = func_cursor.matches(&func_query, tree.root_node(), source_bytes);
    while let Some(m) = func_matches.next() {
        let mut def_range: Option<(usize, usize)> = None;
        let mut def_node = None;
        let mut func_name: Option<String> = None;
        for cap in m.captures.iter() {
            let cap_name = &func_query.capture_names()[cap.index as usize];
            if *cap_name == "func.def" {
                def_range = Some((cap.node.start_byte(), cap.node.end_byte()));
                def_node = Some(cap.node);
            } else if *cap_name == "func.name" {
                let start = cap.node.start_byte();
                let end = cap.node.end_byte();
                func_name = std::str::from_utf8(&source_bytes[start..end])
                    .ok()
                    .map(|s| s.trim().to_owned());
            }
        }
        if let (Some((s, e)), Some(def_node), Some(name)) = (def_range, def_node, func_name)
            && !name.is_empty()
        {
            let declaration_path = if config.language_key == "rs" {
                rust_function_declaration_path(def_node, source_bytes, &name)
            } else {
                None
            };
            func_ranges.push((s, e, name, declaration_path));
        }
    }

    // Parse call sites
    let Some(call_query) =
        cached_call_query(config.language_key, &config.language, config.call_query)
    else {
        return Vec::new();
    };
    let is_js_ts = matches!(config.language_key, "js" | "ts" | "tsx");
    let mut js_callsites: Vec<(usize, usize)> = Vec::new();
    let mut call_cursor = QueryCursor::new();
    let mut call_matches = call_cursor.matches(&call_query, tree.root_node(), source_bytes);
    let file_path = path.to_string_lossy().to_string();
    let mut edges = Vec::new();

    while let Some(m) = call_matches.next() {
        let callee_qualifier = m
            .captures
            .iter()
            .find(|cap| call_query.capture_names()[cap.index as usize] == "callee.object")
            .and_then(|cap| {
                let start = cap.node.start_byte();
                let end = cap.node.end_byte();
                std::str::from_utf8(&source_bytes[start..end])
                    .ok()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            });
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
            if callee_name.is_empty() {
                continue;
            }
            let noise_name = is_noise_callee_for_lang(&callee_name, Some(config.language_key));
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
            let caller = func_ranges
                .iter()
                .filter(|(fs, fe, _, _)| *fs <= start && *fe >= end)
                // pick the innermost (smallest range)
                .min_by_key(|(fs, fe, _, _)| fe - fs);
            let (caller_name, caller_declaration_path) = caller.map_or_else(
                || ("<module>".to_owned(), None),
                |(_, _, name, declaration_path)| (name.clone(), declaration_path.clone()),
            );

            edges.push(LocatedCallEdge {
                edge: CallEdge {
                    caller_file: file_path.clone(),
                    caller_name,
                    caller_declaration_path,
                    callee_name,
                    callee_qualifier: callee_qualifier.clone(),
                    line,
                    resolved_file: None,
                    confidence: 0.0,
                    resolution_strategy: None,
                    canonical_callee_name: None,
                    target_declaration_path: None,
                },
                start_byte: start,
                end_byte: end,
                column: cap.node.start_position().column + 1,
                caller: if is_js_ts {
                    js_callsites.push((edges.len(), cap.node.id()));
                    // Filled in by one tree walk after the matches.
                    LocatedCaller::Module
                } else {
                    LocatedCaller::Unsupported {
                        kind: "non_js_ts_caller",
                    }
                },
                noise_name,
            });
        }
    }

    if !js_callsites.is_empty() {
        let nodes = js_callsites.iter().map(|&(_, id)| id).collect();
        let callers = js_located_callers(tree.root_node(), &nodes);
        for (index, id) in js_callsites {
            if let Some(caller) = callers.get(&id) {
                edges[index].caller = caller.clone();
            }
        }
    }
    edges
}

#[cfg(test)]
mod located_caller_tests {
    use super::*;

    /// The per-call-site parent walk the single tree walk replaced, kept as
    /// the oracle. `Node::parent` re-descends from the root on every step.
    fn caller_by_parent_walk(callsite: Node<'_>) -> LocatedCaller {
        let mut ancestor = callsite.parent();
        let mut unbound_function = false;
        while let Some(node) = ancestor {
            let declaration = match node.kind() {
                "method_definition"
                | "public_field_definition"
                | "field_definition"
                | "class_static_block"
                | "generator_function_declaration" => {
                    return LocatedCaller::Unsupported { kind: node.kind() };
                }
                "function_declaration" => Some(node),
                "arrow_function" | "function_expression" | "generator_function" => {
                    let parent = node.parent();
                    if parent.is_some_and(|parent| parent.kind() == "variable_declarator") {
                        parent
                    } else {
                        unbound_function = true;
                        None
                    }
                }
                "pair" | "assignment_expression" | "variable_declarator" if unbound_function => {
                    return LocatedCaller::Unsupported { kind: node.kind() };
                }
                _ => None,
            };
            if let Some(declaration) = declaration {
                let Some(name) = declaration.child_by_field_name("name") else {
                    return LocatedCaller::Unsupported {
                        kind: declaration.kind(),
                    };
                };
                if name.kind() != "identifier" {
                    return LocatedCaller::Unsupported {
                        kind: declaration.kind(),
                    };
                }
                return LocatedCaller::Declaration {
                    kind: declaration.kind(),
                    start_byte: declaration.start_byte(),
                    end_byte: declaration.end_byte(),
                    name_start_byte: name.start_byte(),
                    name_end_byte: name.end_byte(),
                };
            }
            ancestor = node.parent();
        }
        LocatedCaller::Module
    }

    const SHAPES: &str = r#"
top();
function outer() {
  inner();
  const arrow = () => { inArrow(); };
  items.map((x) => { inCallback(x); });
  const obj = { key: () => { inPair(); }, method() { inMethod(); } };
  handler = function () { inAssigned(); };
  function* gen() { inGenerator(); }
  const fx = function named() { inNamedExpr(); };
}
class Widget {
  field = make();
  static { inStatic(); }
  render() { draw(); }
}
export const exported = async () => { await load(); };
export default function () { inDefault(); }
(function () { inIife(); })();
"#;

    fn assert_matches_parent_walk(path: &str, source: &str) -> usize {
        let calls = extract_located_calls_from_source(Path::new(path), source);
        let config = call_language_for_path(Path::new(path)).unwrap();
        let mut parser = Parser::new();
        parser.set_language(&config.language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        for call in &calls {
            let node = tree
                .root_node()
                .descendant_for_byte_range(call.start_byte, call.end_byte)
                .unwrap();
            assert_eq!(
                call.caller,
                caller_by_parent_walk(node),
                "{path}: `{}` at {}",
                call.edge.callee_name,
                call.start_byte
            );
        }
        calls.len()
    }

    #[test]
    fn located_callers_match_the_parent_walk() {
        for path in ["shapes.js", "shapes.ts", "shapes.tsx"] {
            let count = assert_matches_parent_walk(path, SHAPES);
            assert!(count >= 15, "{path}: only {count} calls extracted");
        }
        let kinds = extract_located_calls_from_source(Path::new("shapes.js"), SHAPES)
            .into_iter()
            .map(|call| match call.caller {
                LocatedCaller::Module => "module",
                LocatedCaller::Declaration { kind, .. } => kind,
                LocatedCaller::Unsupported { kind } => kind,
            })
            .collect::<HashSet<_>>();
        for kind in [
            "module",
            "function_declaration",
            "variable_declarator",
            "method_definition",
            "pair",
            "assignment_expression",
        ] {
            assert!(
                kinds.contains(kind),
                "fixture lost the {kind} case: {kinds:?}"
            );
        }
    }

    /// Bundled code nests callbacks deeply, and `Node::parent` re-descends
    /// from the root, so walking parents from every call site cost
    /// (depth squared) per call: 349 ms of a 545 ms extraction on a 2.4 MB
    /// bundle.
    #[test]
    fn located_callers_do_not_walk_parents_per_call_site() {
        const DEPTH: usize = 250;
        let mut source = String::new();
        for _ in 0..DEPTH {
            source.push_str("f(() => {\n");
        }
        for _ in 0..DEPTH {
            source.push_str("});\n");
        }
        let started = std::time::Instant::now();
        let calls = extract_located_calls_from_source(Path::new("bundle.js"), &source);
        let elapsed = started.elapsed();
        assert_eq!(calls.len(), DEPTH);
        assert!(
            calls
                .iter()
                .all(|call| call.caller == LocatedCaller::Module)
        );
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "{DEPTH} nested call sites took {elapsed:?}"
        );
    }
}
