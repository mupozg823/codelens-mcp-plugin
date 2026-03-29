use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Cached compiled tree-sitter Query for call graph extraction.
/// Key: (language pointer as usize, query string pointer as usize)
static CALL_QUERY_CACHE: LazyLock<Mutex<HashMap<usize, Arc<Query>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_call_query(language: &Language, query_str: &'static str) -> Option<Arc<Query>> {
    let key = query_str.as_ptr() as usize;
    let mut cache = CALL_QUERY_CACHE.lock().unwrap();
    if let Some(q) = cache.get(&key) {
        return Some(Arc::clone(q));
    }
    let q = Query::new(language, query_str).ok()?;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct CallerEntry {
    pub file: String,
    pub function: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalleeEntry {
    pub name: String,
    pub line: usize,
}

struct CallLanguageConfig {
    language: Language,
    /// Query to find function definitions: captures @func.name
    func_query: &'static str,
    /// Query to find call sites: captures @callee
    call_query: &'static str,
}

/// Resolve call graph config via the unified language registry.
/// Only a subset of languages have call graph queries defined.
fn call_language_for_path(path: &Path) -> Option<CallLanguageConfig> {
    let lang_config = crate::lang_config::language_for_path(path)?;
    // Map canonical extension to call graph queries (not all languages support this)
    let (func_query, call_query) = match lang_config.extension {
        "py" => (PYTHON_FUNC_QUERY, PYTHON_CALL_QUERY),
        "js" => (JS_FUNC_QUERY, JS_CALL_QUERY),
        "ts" | "tsx" => (JS_FUNC_QUERY, JS_CALL_QUERY),
        "go" => (GO_FUNC_QUERY, GO_CALL_QUERY),
        "java" => (JAVA_FUNC_QUERY, JAVA_CALL_QUERY),
        "kt" => (KOTLIN_FUNC_QUERY, JAVA_CALL_QUERY),
        "rs" => (RUST_FUNC_QUERY, RUST_CALL_QUERY),
        _ => return None,
    };
    Some(CallLanguageConfig {
        language: lang_config.language,
        func_query,
        call_query,
    })
}

fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| call_language_for_path(path).is_some())
}

/// Parse a file and extract all call edges within each function.
pub fn extract_calls(path: &Path) -> Vec<CallEdge> {
    let Some(config) = call_language_for_path(path) else {
        return Vec::new();
    };
    let Ok(source) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut parser = Parser::new();
    if parser.set_language(&config.language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(&source, None) else {
        return Vec::new();
    };
    let source_bytes = source.as_bytes();

    // Build a map: byte_range_start -> caller_name for each function definition.
    // We'll use this to find which function contains each call site.
    let Some(func_query) = cached_call_query(&config.language, config.func_query) else {
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
        if let (Some((s, e)), Some(name)) = (def_range, func_name) {
            if !name.is_empty() {
                func_ranges.push((s, e, name));
            }
        }
    }

    // Parse call sites
    let Some(call_query) = cached_call_query(&config.language, config.call_query) else {
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
            if callee_name.is_empty() {
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
            });
        }
    }

    edges
}

/// Find all functions that call `function_name` across the project.
pub fn get_callers(
    project: &ProjectRoot,
    function_name: &str,
    max_results: usize,
) -> Result<Vec<CallerEntry>> {
    let files = collect_candidate_files(project.as_path())?;
    let mut results = Vec::new();

    // Deduplicate by (file, function, line)
    let mut seen = std::collections::HashSet::new();

    'outer: for file in &files {
        let edges = extract_calls(file);
        for edge in edges {
            if edge.callee_name == function_name {
                let key = (
                    edge.caller_file.clone(),
                    edge.caller_name.clone(),
                    edge.line,
                );
                if seen.insert(key) {
                    let relative = project.to_relative(file);
                    results.push(CallerEntry {
                        file: relative,
                        function: edge.caller_name,
                        line: edge.line,
                    });
                    if max_results > 0 && results.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Find all functions called by `function_name` (optionally restricted to a file).
pub fn get_callees(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
) -> Result<Vec<CalleeEntry>> {
    let files: Vec<PathBuf> = if let Some(fp) = file_path {
        let resolved = project.resolve(fp)?;
        vec![resolved]
    } else {
        collect_candidate_files(project.as_path())?
    };

    // Collect all call edges from functions named `function_name`
    let mut seen: HashMap<(String, usize), ()> = HashMap::new();
    let mut results = Vec::new();

    'outer: for file in &files {
        let edges = extract_calls(file);
        for edge in edges {
            if edge.caller_name == function_name {
                let key = (edge.callee_name.clone(), edge.line);
                if seen.insert(key, ()).is_none() {
                    results.push(CalleeEntry {
                        name: edge.callee_name,
                        line: edge.line,
                    });
                    if max_results > 0 && results.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }
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
"#;

const JS_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
(method_definition name: (property_identifier) @func.name) @func.def
(function (identifier) @func.name) @func.def
"#;

const JS_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (member_expression property: (property_identifier) @callee))
"#;

const GO_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
(method_declaration name: (field_identifier) @func.name) @func.def
"#;

const GO_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (selector_expression field: (field_identifier) @callee))
"#;

const JAVA_FUNC_QUERY: &str = r#"
(method_declaration name: (identifier) @func.name) @func.def
(constructor_declaration name: (identifier) @func.name) @func.def
"#;

const JAVA_CALL_QUERY: &str = r#"
(method_invocation name: (identifier) @callee)
"#;

const KOTLIN_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
"#;

const RUST_FUNC_QUERY: &str = r#"
(function_item name: (identifier) @func.name) @func.def
"#;

const RUST_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (field_expression field: (field_identifier) @callee))
"#;

#[cfg(test)]
mod tests {
    use super::{extract_calls, get_callees, get_callers};
    use crate::ProjectRoot;
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

    #[test]
    fn get_callers_finds_callers() {
        let dir = temp_dir("callers");
        fs::write(dir.join("a.py"), "def foo():\n    bar()\n    baz()\n").expect("write a");
        fs::write(dir.join("b.py"), "def qux():\n    bar()\n").expect("write b");
        fs::write(dir.join("c.py"), "def bar():\n    pass\n").expect("write c");

        let project = ProjectRoot::new(&dir).expect("project");
        let callers = get_callers(&project, "bar", 50).expect("callers");
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
        let callees = get_callees(&project, "main", None, 50).expect("callees");
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
    fn get_callees_scoped_to_file() {
        let dir = temp_dir("callees-file");
        fs::write(dir.join("a.py"), "def process():\n    helper()\n").expect("write a");
        fs::write(dir.join("b.py"), "def process():\n    other()\n").expect("write b");

        let project = ProjectRoot::new(&dir).expect("project");
        let callees = get_callees(&project, "process", Some("a.py"), 50).expect("callees");
        let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"helper"), "expected helper, got {names:?}");
        assert!(!names.contains(&"other"), "should not have other from b.py");
    }
}
