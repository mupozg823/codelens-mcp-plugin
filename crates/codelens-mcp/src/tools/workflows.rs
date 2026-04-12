use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolHandler, ToolResult, success_meta};
use serde_json::{Value, json};

fn attach_workflow_metadata(workflow: &str, delegated_tool: &str, payload: Value) -> Value {
    match payload {
        Value::Object(mut map) => {
            map.insert("workflow".to_owned(), json!(workflow));
            map.insert("delegated_tool".to_owned(), json!(delegated_tool));
            Value::Object(map)
        }
        other => json!({
            "workflow": workflow,
            "delegated_tool": delegated_tool,
            "result": other,
        }),
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

pub fn audit_security_context(state: &AppState, arguments: &Value) -> ToolResult {
    delegate_workflow(
        state,
        "audit_security_context",
        "semantic_code_review",
        arguments.clone(),
        crate::tools::reports::semantic_code_review,
    )
}

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
    if arguments
        .get("path")
        .or_else(|| arguments.get("file_path"))
        .and_then(|v| v.as_str())
        .is_some()
    {
        return delegate_workflow(
            state,
            "diagnose_issues",
            "get_file_diagnostics",
            json!({
                "file_path": arguments.get("file_path")
                    .or_else(|| arguments.get("path"))
                    .and_then(|v| v.as_str()),
            }),
            crate::tools::lsp::get_file_diagnostics,
        );
    }

    delegate_workflow(
        state,
        "diagnose_issues",
        "unresolved_reference_check",
        json!({
            "file_path": arguments.get("file_path").and_then(|v| v.as_str()),
            "symbol": arguments.get("symbol").and_then(|v| v.as_str()),
            "changed_files": arguments.get("changed_files"),
        }),
        crate::tools::reports::unresolved_reference_check,
    )
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
