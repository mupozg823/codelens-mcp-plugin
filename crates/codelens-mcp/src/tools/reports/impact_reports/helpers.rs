use crate::error::CodeLensError;
use crate::tools::query_analysis::semantic_query_for_retrieval;
use codelens_engine::ProjectRoot;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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

pub(super) fn analysis_completeness_section(
    impact: &Value,
    cycle_limit_hit: bool,
    coupling_limit_hit: bool,
) -> Value {
    let in_scope_file_limit_hit = impact
        .get("in_scope_file_limit_hit")
        .and_then(Value::as_bool);
    let direct_importer_limit_hit = impact
        .get("direct_importer_limit_hit")
        .and_then(Value::as_bool);
    let has_scope_evidence = impact.get("scope_kind").and_then(Value::as_str).is_some()
        && impact
            .get("in_scope_file_count")
            .and_then(Value::as_u64)
            .is_some()
        && in_scope_file_limit_hit.is_some()
        && direct_importer_limit_hit.is_some();
    let any_limit_hit = in_scope_file_limit_hit == Some(true)
        || direct_importer_limit_hit == Some(true)
        || cycle_limit_hit
        || coupling_limit_hit;
    let status = match (any_limit_hit, has_scope_evidence) {
        (true, true) => "partial",
        (false, true) => "complete",
        _ => "unavailable",
    };
    let reason = match status {
        "partial" => {
            "At least one architecture evidence limit was reached; omitted files, importers, cycles, or coupling entries may change the result."
        }
        "complete" => {
            "The requested scope was analyzed without hitting a file, importer, cycle, or coupling evidence limit."
        }
        _ => "The analyzer did not return enough scope evidence to claim completeness.",
    };

    json!({
        "status": status,
        "reason": reason,
        "scope_kind": impact.get("scope_kind").cloned().unwrap_or(Value::Null),
        "in_scope_file_count": impact.get("in_scope_file_count").cloned().unwrap_or(Value::Null),
        "in_scope_file_limit_hit": in_scope_file_limit_hit,
        "direct_importer_limit_hit": direct_importer_limit_hit,
        "cycle_limit_hit": cycle_limit_hit,
        "coupling_limit_hit": coupling_limit_hit,
    })
}

pub(super) fn validate_architecture_scope(
    project: &ProjectRoot,
    path: &str,
) -> Result<(), CodeLensError> {
    let resolved = project
        .resolve(path)
        .map_err(|error| CodeLensError::Validation(error.to_string()))?;
    if !resolved.exists() {
        return Err(CodeLensError::NotFound(format!(
            "architecture scope `{path}` does not exist"
        )));
    }
    if resolved.is_file() && !codelens_engine::supports_import_graph(path) {
        let language = resolved
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("unknown")
            .to_owned();
        return Err(CodeLensError::LanguageUnsupported {
            language,
            feature: "architecture analysis does not support this file type".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn verifier_files_for_path(project: &ProjectRoot, path: &str) -> Vec<String> {
    project
        .resolve(path)
        .ok()
        .filter(|resolved| resolved.is_file())
        .map(|_| vec![path.to_owned()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_impact_evidence_is_unavailable_not_complete() {
        let completeness = analysis_completeness_section(&json!({}), false, false);

        assert_eq!(completeness["status"], json!("unavailable"));
        assert_eq!(completeness["in_scope_file_limit_hit"], Value::Null);
    }

    #[test]
    fn every_bounded_architecture_lane_can_make_analysis_partial() {
        let complete_impact = json!({
            "scope_kind": "file",
            "in_scope_file_count": 1,
            "in_scope_file_limit_hit": false,
            "direct_importer_limit_hit": false,
        });
        assert_eq!(
            analysis_completeness_section(&complete_impact, false, false)["status"],
            json!("complete")
        );

        let importer_capped = json!({
            "scope_kind": "file",
            "in_scope_file_count": 1,
            "in_scope_file_limit_hit": false,
            "direct_importer_limit_hit": true,
        });
        assert_eq!(
            analysis_completeness_section(&importer_capped, false, false)["status"],
            json!("partial")
        );
        assert_eq!(
            analysis_completeness_section(&complete_impact, true, false)["status"],
            json!("partial")
        );
        assert_eq!(
            analysis_completeness_section(&complete_impact, false, true)["status"],
            json!("partial")
        );
    }
}
