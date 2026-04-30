use crate::tools::query_analysis::semantic_query_for_retrieval;
use serde_json::Value;
use std::collections::BTreeMap;

fn semantic_status_is_ready(status: &Value) -> bool {
    status
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "ready")
}

pub(super) fn push_unique(items: &mut Vec<String>, item: impl Into<String>) {
    let item = item.into();
    if !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

pub(super) fn semantic_degraded_note(status: &Value) -> Option<String> {
    if semantic_status_is_ready(status) {
        return None;
    }
    let reason = status
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("semantic enrichment unavailable");
    Some(format!(
        "Semantic enrichment unavailable; report uses structural evidence only. {reason}."
    ))
}

pub(super) fn insert_semantic_status(sections: &mut BTreeMap<String, Value>, status: Value) {
    sections.insert("semantic_status".to_owned(), status);
}

fn path_hint(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".rs")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx")
        .trim_end_matches(".py")
        .trim_end_matches(".go")
        .replace(['_', '-'], " ")
}

pub(super) fn build_module_semantic_query(path: &str, symbol_names: &[String]) -> String {
    let hint = path_hint(path);
    let query = if symbol_names.is_empty() {
        format!("module boundary responsibilities {hint}")
    } else {
        format!(
            "module boundary responsibilities {hint} {}",
            symbol_names.join(" ")
        )
    };
    semantic_query_for_retrieval(&query)
}

pub(super) fn build_dead_code_semantic_query(name: &str, file: Option<&str>) -> String {
    let query = match file {
        Some(file) if !file.is_empty() => {
            format!("similar live code for {name} in {}", path_hint(file))
        }
        _ => format!("similar live code for {name}"),
    };
    semantic_query_for_retrieval(&query)
}

/// Extract a file-path-like string from an impact-analysis entry,
/// tolerating schemas that use `file`, `file_path`, or `path`.
pub(super) fn impact_entry_file(value: &Value) -> Option<&str> {
    value
        .get("file")
        .and_then(Value::as_str)
        .or_else(|| value.get("file_path").and_then(Value::as_str))
        .or_else(|| value.get("path").and_then(Value::as_str))
}

/// Sanitise a label for safe embedding inside a Mermaid `["..."]` node body.
/// Mermaid does not accept unescaped double-quotes inside quoted labels.
pub(super) fn mermaid_escape_label(raw: &str) -> String {
    raw.replace('"', "'")
}

/// Returns the parent directory portion of a path, or `"."` for bare filenames.
pub(super) fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

/// Returns only the last path component (filename) of a path.
pub(super) fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}
