use crate::tools::query_analysis::semantic_query_for_retrieval;
use crate::{AppState, error::CodeLensError};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_SCOPE_FILES: usize = 300;
const MAX_SCOPE_ENTRIES: usize = 30;
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".codelens",
    "target",
    "node_modules",
    ".next",
    "dist",
    "build",
    "__pycache__",
];

#[derive(Debug, Clone)]
pub(super) struct ScopeBoundarySummary {
    pub scope: String,
    pub resolved_path: String,
    pub file_count: usize,
    pub truncated: bool,
    pub internal_edge_count: usize,
    pub inbound_external_count: usize,
    pub outbound_external_count: usize,
    pub affected_external_count: usize,
    pub internal_edges: Vec<(String, String)>,
    pub inbound_external: Vec<String>,
    pub outbound_external: Vec<String>,
    pub affected_external: Vec<String>,
    pub top_files: Vec<Value>,
}

fn semantic_status_is_ready(status: &Value) -> bool {
    status
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "ready")
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

pub(super) fn collect_scope_boundary_summary(
    state: &AppState,
    path: &str,
) -> Result<Option<ScopeBoundarySummary>, CodeLensError> {
    let project = state.project();
    let resolved = project.resolve(path)?;
    if !resolved.is_dir() {
        return Ok(None);
    }

    let mut files = Vec::new();
    collect_import_files(&resolved, &mut files)?;
    files.sort();
    let truncated = files.len() > MAX_SCOPE_FILES;
    if truncated {
        files.truncate(MAX_SCOPE_FILES);
    }

    let scope = normalized_scope(&project.to_relative(&resolved));
    let mut internal_edges = BTreeSet::new();
    let mut inbound_external = BTreeSet::new();
    let mut outbound_external = BTreeSet::new();
    let mut affected_external = BTreeSet::new();
    let mut top_files = Vec::new();

    for file in &files {
        let rel = project.to_relative(file);
        let mut internal_out = 0usize;
        let mut external_out = 0usize;

        for module in codelens_engine::extract_imports_for_file(file) {
            if let Some(target) = codelens_engine::resolve_module_for_file(&project, file, &module)
            {
                if is_in_scope(&scope, &target) {
                    internal_edges.insert((rel.clone(), target));
                    internal_out += 1;
                } else {
                    outbound_external.insert(target);
                    external_out += 1;
                }
            }
        }

        let importers = codelens_engine::get_importers(&project, &rel, 50, &state.graph_cache())
            .unwrap_or_default();
        let mut external_importers = 0usize;
        for importer in importers {
            if !is_in_scope(&scope, &importer.file) {
                inbound_external.insert(importer.file);
                external_importers += 1;
            }
        }

        let blast = codelens_engine::get_blast_radius(&project, &rel, 2, &state.graph_cache())
            .unwrap_or_default();
        let mut external_affected = 0usize;
        for entry in blast {
            if !is_in_scope(&scope, &entry.file) {
                affected_external.insert(entry.file);
                external_affected += 1;
            }
        }

        let score = internal_out + external_out + external_importers + external_affected;
        if score > 0 {
            top_files.push(json!({
                "file": rel,
                "internal_outbound": internal_out,
                "external_dependencies": external_out,
                "external_importers": external_importers,
                "external_affected": external_affected,
                "score": score,
            }));
        }
    }

    top_files.sort_by(|left, right| {
        let left_score = left
            .get("score")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let right_score = right
            .get("score")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        right_score
            .cmp(&left_score)
            .then_with(|| json_file(left).cmp(json_file(right)))
    });
    top_files.truncate(MAX_SCOPE_ENTRIES);

    let internal_edge_count = internal_edges.len();
    let inbound_external_count = inbound_external.len();
    let outbound_external_count = outbound_external.len();
    let affected_external_count = affected_external.len();

    Ok(Some(ScopeBoundarySummary {
        scope,
        resolved_path: resolved.to_string_lossy().replace('\\', "/"),
        file_count: files.len(),
        truncated,
        internal_edge_count,
        inbound_external_count,
        outbound_external_count,
        affected_external_count,
        internal_edges: internal_edges.into_iter().take(MAX_SCOPE_ENTRIES).collect(),
        inbound_external: inbound_external
            .into_iter()
            .take(MAX_SCOPE_ENTRIES)
            .collect(),
        outbound_external: outbound_external
            .into_iter()
            .take(MAX_SCOPE_ENTRIES)
            .collect(),
        affected_external: affected_external
            .into_iter()
            .take(MAX_SCOPE_ENTRIES)
            .collect(),
        top_files,
    }))
}

fn collect_import_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name))
            {
                continue;
            }
            collect_import_files(&path, out)?;
        } else if path.is_file() && codelens_engine::supports_import_graph(&path.to_string_lossy())
        {
            out.push(path);
        }
    }
    Ok(())
}

fn normalized_scope(scope: &str) -> String {
    let trimmed = scope.trim_matches('/');
    if trimmed.is_empty() {
        ".".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn is_in_scope(scope: &str, file: &str) -> bool {
    scope == "." || file == scope || file.starts_with(&format!("{scope}/"))
}

fn json_file(value: &Value) -> &str {
    value.get("file").and_then(Value::as_str).unwrap_or("")
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
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(".")
}

/// Returns only the last path component (filename) of a path.
pub(super) fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path)
}
