use crate::project::ProjectRoot;
use regex::Regex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use super::types::CallEdge;

static JS_IMPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\bimport\s+([^;]+?)\s+from\s+["']([^"']+)["']"#).expect("import regex")
});
static JS_REEXPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\bexport\s+([^;]+?)\s+from\s+["']([^"']+)["']"#).expect("re-export regex")
});

#[derive(Debug)]
pub(crate) struct LocalBindingScope {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) names: HashSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct JSImportBinding {
    pub(crate) imported_name: Option<String>,
    pub(crate) resolved_file: Option<String>,
    pub(crate) external: bool,
}

pub(crate) type JSImportBindingIndex = HashMap<String, HashMap<String, JSImportBinding>>;
pub(crate) fn is_import_sensitive_path(path: &str) -> bool {
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

fn parse_js_reexport_bindings(
    bindings: &mut HashMap<String, JSImportBinding>,
    clause: &str,
    resolved_file: Option<&String>,
    module: &str,
) {
    let clause = clause.trim().trim_start_matches("type ").trim();
    if !clause.starts_with('{') {
        return;
    }
    let Some(end) = clause.find('}') else {
        return;
    };
    let external = is_external_module_specifier(module, resolved_file);

    for item in clause[1..end].split(',') {
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

pub(crate) fn build_js_import_binding_index(
    project: &ProjectRoot,
    files: &[PathBuf],
) -> JSImportBindingIndex {
    let mut index = HashMap::new();
    let mut queue: VecDeque<(PathBuf, usize)> =
        files.iter().cloned().map(|file| (file, 0)).collect();
    let mut seen = HashSet::new();
    while let Some((file, depth)) = queue.pop_front() {
        let relative = project.to_relative(&file);
        if !seen.insert(relative.clone()) {
            continue;
        }
        if !is_import_sensitive_path(&relative) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else {
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
            let resolved_file =
                crate::import_graph::resolve_module_for_file(project, &file, module);
            if depth == 0
                && let Some(resolved_file) = resolved_file.as_ref()
                && let Ok(resolved_path) = project.resolve(resolved_file)
            {
                queue.push_back((resolved_path, 1));
            }
            parse_js_import_bindings(&mut bindings, clause, resolved_file.as_ref(), module);
        }
        for capture in JS_REEXPORT_FROM_RE.captures_iter(&source) {
            let Some(clause) = capture.get(1).map(|value| value.as_str()) else {
                continue;
            };
            let Some(module) = capture.get(2).map(|value| value.as_str()) else {
                continue;
            };
            let resolved_file =
                crate::import_graph::resolve_module_for_file(project, &file, module);
            if depth == 0
                && let Some(resolved_file) = resolved_file.as_ref()
                && let Ok(resolved_path) = project.resolve(resolved_file)
            {
                queue.push_back((resolved_path, 1));
            }
            parse_js_reexport_bindings(&mut bindings, clause, resolved_file.as_ref(), module);
        }
        if !bindings.is_empty() {
            index.insert(relative, bindings);
        }
    }
    index
}

pub(crate) fn filter_external_import_edges(
    edges: &mut Vec<CallEdge>,
    import_bindings: &JSImportBindingIndex,
) {
    edges.retain(|edge| {
        import_bindings
            .get(&edge.caller_file)
            .and_then(|bindings| bindings.get(&edge.callee_name))
            .map(|binding| !binding.external)
            .unwrap_or(true)
    });
}
