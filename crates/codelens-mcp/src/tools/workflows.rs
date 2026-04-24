use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_defs::deprecated_workflow_alias;
use crate::tool_runtime::{ToolHandler, ToolResult, success_meta};
use serde_json::{Value, json};

fn attach_workflow_metadata(workflow: &str, delegated_tool: &str, payload: Value) -> Value {
    let deprecation = deprecated_workflow_alias(workflow);
    match payload {
        Value::Object(mut map) => {
            map.insert("workflow".to_owned(), json!(workflow));
            map.insert("delegated_tool".to_owned(), json!(delegated_tool));
            if let Some((replacement_tool, removal_target)) = deprecation {
                map.insert("deprecated".to_owned(), json!(true));
                map.insert("replacement_tool".to_owned(), json!(replacement_tool));
                map.insert("removal_target".to_owned(), json!(removal_target));
            }
            Value::Object(map)
        }
        other => {
            let mut map = serde_json::Map::new();
            map.insert("workflow".to_owned(), json!(workflow));
            map.insert("delegated_tool".to_owned(), json!(delegated_tool));
            if let Some((replacement_tool, removal_target)) = deprecation {
                map.insert("deprecated".to_owned(), json!(true));
                map.insert("replacement_tool".to_owned(), json!(replacement_tool));
                map.insert("removal_target".to_owned(), json!(removal_target));
            }
            map.insert("result".to_owned(), other);
            Value::Object(map)
        }
    }
}

fn delegate_workflow(
    state: &AppState,
    workflow: &str,
    delegated_tool: &str,
    delegated_args: Value,
    handler: ToolHandler,
) -> ToolResult {
    let (payload, meta) = handler(state, &delegated_args)?;
    Ok((
        attach_workflow_metadata(workflow, delegated_tool, payload),
        meta,
    ))
}

fn diagnose_path_scope(state: &AppState, path: &str, max_results: Option<u64>) -> ToolResult {
    let project = state.project();
    let resolved = project.resolve(path)?;
    if resolved.is_file() {
        return delegate_workflow(
            state,
            "diagnose_issues",
            "get_file_diagnostics",
            json!({
                "file_path": path,
                "max_results": max_results,
            }),
            crate::tools::lsp::get_file_diagnostics,
        );
    }
    if !resolved.is_dir() {
        return Err(CodeLensError::NotFound(format!(
            "path does not exist in project scope: {path}"
        )));
    }

    let candidate_files = codelens_engine::find_files(&project, "*", Some(path))?;
    let max_files = 8usize;
    let mut files = Vec::new();
    let mut skipped_files = Vec::new();
    let mut diagnosable_files = 0usize;
    let mut diagnostic_count = 0usize;

    for file_path in candidate_files.into_iter().map(|entry| entry.path) {
        if crate::tools::default_lsp_command_for_path(&file_path).is_none() {
            skipped_files.push(json!({
                "file_path": file_path,
                "reason": "no_default_lsp_mapping",
            }));
            continue;
        }

        diagnosable_files += 1;
        if files.len() >= max_files {
            skipped_files.push(json!({
                "file_path": file_path,
                "reason": "file_limit_reached",
            }));
            continue;
        }

        match crate::tools::lsp::get_file_diagnostics(
            state,
            &json!({
                "file_path": &file_path,
                "max_results": max_results,
            }),
        ) {
            Ok((payload, _meta)) => {
                let count = payload
                    .get("count")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default() as usize;
                diagnostic_count += count;
                files.push(json!({
                    "file_path": file_path,
                    "count": count,
                    "diagnostics": payload.get("diagnostics").cloned().unwrap_or_else(|| json!([])),
                }));
            }
            Err(error) => {
                skipped_files.push(json!({
                    "file_path": file_path,
                    "reason": "diagnostics_error",
                    "error": error.to_string(),
                }));
            }
        }
    }

    Ok((
        attach_workflow_metadata(
            "diagnose_issues",
            "directory_diagnostics",
            json!({
                "path": path,
                "scope": "directory",
                "files": files,
                "returned_file_count": files.len(),
                "diagnosable_file_count": diagnosable_files,
                "diagnostic_count": diagnostic_count,
                "skipped_files": skipped_files,
                "count": diagnostic_count,
            }),
        ),
        success_meta(BackendKind::Lsp, 0.82),
    ))
}

pub fn explore_codebase(state: &AppState, arguments: &Value) -> ToolResult {
    if let Some(query) = arguments.get("query").and_then(|value| value.as_str()) {
        return delegate_workflow(
            state,
            "explore_codebase",
            "get_ranked_context",
            json!({
                "query": query,
                "path": arguments.get("path").and_then(|value| value.as_str()),
                "max_tokens": arguments.get("max_tokens").and_then(|value| value.as_u64()),
                "include_body": arguments.get("include_body").and_then(|value| value.as_bool()),
                "depth": arguments.get("depth").and_then(|value| value.as_u64()),
                "disable_semantic": arguments.get("disable_semantic").and_then(|value| value.as_bool()),
            }),
            crate::tools::symbols::get_ranked_context,
        );
    }

    delegate_workflow(
        state,
        "explore_codebase",
        "onboard_project",
        json!({}),
        crate::tools::composite::onboard_project,
    )
}

pub fn trace_request_path(state: &AppState, arguments: &Value) -> ToolResult {
    let function_name = arguments
        .get("function_name")
        .or_else(|| arguments.get("symbol"))
        .or_else(|| arguments.get("entrypoint"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("function_name".to_owned()))?;

    delegate_workflow(
        state,
        "trace_request_path",
        "explain_code_flow",
        json!({
            "function_name": function_name,
            "max_depth": arguments.get("max_depth").and_then(|value| value.as_u64()),
            "max_results": arguments.get("max_results").and_then(|value| value.as_u64()),
        }),
        crate::tools::composite::explain_code_flow,
    )
}

pub fn review_architecture(state: &AppState, arguments: &Value) -> ToolResult {
    let include_diagram = arguments
        .get("include_diagram")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if let Some(path) = arguments.get("path").and_then(|value| value.as_str()) {
        if include_diagram {
            return delegate_workflow(
                state,
                "review_architecture",
                "mermaid_module_graph",
                json!({
                    "path": path,
                    "max_nodes": arguments.get("max_nodes").and_then(|value| value.as_u64()),
                }),
                crate::tools::reports::mermaid_module_graph,
            );
        }

        return delegate_workflow(
            state,
            "review_architecture",
            "module_boundary_report",
            json!({ "path": path }),
            crate::tools::reports::module_boundary_report,
        );
    }

    delegate_workflow(
        state,
        "review_architecture",
        "onboard_project",
        json!({}),
        crate::tools::composite::onboard_project,
    )
}

pub fn plan_safe_refactor(state: &AppState, arguments: &Value) -> ToolResult {
    if let (Some(file_path), Some(symbol)) = (
        arguments.get("file_path").and_then(|value| value.as_str()),
        arguments.get("symbol").and_then(|value| value.as_str()),
    ) {
        return delegate_workflow(
            state,
            "plan_safe_refactor",
            "safe_rename_report",
            json!({
                "file_path": file_path,
                "symbol": symbol,
                "new_name": arguments.get("new_name").and_then(|value| value.as_str()),
            }),
            crate::tools::reports::safe_rename_report,
        );
    }

    delegate_workflow(
        state,
        "plan_safe_refactor",
        "refactor_safety_report",
        json!({
            "task": arguments.get("task").and_then(|value| value.as_str()),
            "symbol": arguments.get("symbol").and_then(|value| value.as_str()),
            "path": arguments.get("path").and_then(|value| value.as_str()),
            "file_path": arguments.get("file_path").and_then(|value| value.as_str()),
        }),
        crate::tools::reports::refactor_safety_report,
    )
}

#[deprecated(
    since = "1.12.0",
    note = "Pure delegate. Call `semantic_code_review` directly. Scheduled for removal in v2.0."
)]
pub fn audit_security_context(state: &AppState, arguments: &Value) -> ToolResult {
    delegate_workflow(
        state,
        "audit_security_context",
        "semantic_code_review",
        arguments.clone(),
        crate::tools::reports::semantic_code_review,
    )
}

#[deprecated(
    since = "1.12.0",
    note = "Pure delegate. Call `impact_report` directly. Scheduled for removal in v2.0."
)]
pub fn analyze_change_impact(state: &AppState, arguments: &Value) -> ToolResult {
    delegate_workflow(
        state,
        "analyze_change_impact",
        "impact_report",
        arguments.clone(),
        crate::tools::reports::impact_report,
    )
}

pub fn review_changes(state: &AppState, arguments: &Value) -> ToolResult {
    if arguments
        .get("changed_files")
        .and_then(|v| v.as_array())
        .is_some()
    {
        return delegate_workflow(
            state,
            "review_changes",
            "diff_aware_references",
            arguments.clone(),
            crate::tools::reports::diff_aware_references,
        );
    }

    delegate_workflow(
        state,
        "review_changes",
        "impact_report",
        arguments.clone(),
        crate::tools::reports::impact_report,
    )
}

#[deprecated(
    since = "1.12.0",
    note = "Pure delegate. Call `verify_change_readiness` directly. Scheduled for removal in v2.0."
)]
pub fn assess_change_readiness(state: &AppState, arguments: &Value) -> ToolResult {
    delegate_workflow(
        state,
        "assess_change_readiness",
        "verify_change_readiness",
        arguments.clone(),
        crate::tools::reports::verify_change_readiness,
    )
}

pub fn diagnose_issues(state: &AppState, arguments: &Value) -> ToolResult {
    let max_results = arguments
        .get("max_results")
        .and_then(|value| value.as_u64());

    if let Some(symbol) = arguments.get("symbol").and_then(|v| v.as_str()) {
        let file_path = arguments
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CodeLensError::MissingParam("file_path".to_owned()))?;
        let resolved = state.project().resolve(file_path)?;
        if !resolved.is_file() {
            return Err(CodeLensError::Validation(
                "diagnose_issues with `symbol` requires a file target, not a directory".to_owned(),
            ));
        }
        return delegate_workflow(
            state,
            "diagnose_issues",
            "unresolved_reference_check",
            json!({
                "file_path": file_path,
                "symbol": symbol,
                "changed_files": arguments.get("changed_files"),
            }),
            crate::tools::reports::unresolved_reference_check,
        );
    }

    if let Some(file_path) = arguments.get("file_path").and_then(|v| v.as_str()) {
        return delegate_workflow(
            state,
            "diagnose_issues",
            "get_file_diagnostics",
            json!({
                "file_path": file_path,
                "max_results": max_results,
            }),
            crate::tools::lsp::get_file_diagnostics,
        );
    }

    if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
        return diagnose_path_scope(state, path, max_results);
    }

    Err(CodeLensError::MissingParam(
        "file_path, path, or symbol".to_owned(),
    ))
}

pub fn cleanup_duplicate_logic(state: &AppState, arguments: &Value) -> ToolResult {
    #[cfg(feature = "semantic")]
    {
        let threshold = arguments
            .get("threshold")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.85);
        let max_pairs = arguments
            .get("max_pairs")
            .and_then(|value| value.as_u64())
            .unwrap_or(20) as usize;
        let guard = state.embedding_engine();
        if let Some(engine) = guard.as_ref() {
            let pairs = engine.find_duplicates(threshold, max_pairs)?;
            let payload = json!({
                "threshold": threshold,
                "duplicates": pairs,
                "count": pairs.len(),
            });
            return Ok((
                attach_workflow_metadata(
                    "cleanup_duplicate_logic",
                    "find_code_duplicates",
                    payload,
                ),
                success_meta(BackendKind::Semantic, 0.80),
            ));
        }
    }

    delegate_workflow(
        state,
        "cleanup_duplicate_logic",
        "dead_code_report",
        json!({
            "scope": arguments.get("scope").and_then(|value| value.as_str()),
            "max_results": arguments.get("max_results").and_then(|value| value.as_u64()),
        }),
        crate::tools::reports::dead_code_report,
    )
}
