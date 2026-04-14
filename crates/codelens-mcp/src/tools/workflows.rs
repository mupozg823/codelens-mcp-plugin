use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_defs::{DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET, deprecated_workflow_replacement};
use crate::tool_runtime::success_meta;
use crate::tool_runtime::{ToolHandler, ToolResult};
use serde_json::{Value, json};

fn attach_workflow_metadata(workflow: &str, delegated_tool: &str, payload: Value) -> Value {
    match payload {
        Value::Object(mut map) => {
            map.insert("workflow".to_owned(), json!(workflow));
            map.insert("delegated_tool".to_owned(), json!(delegated_tool));
            map.insert("deprecated".to_owned(), json!(false));
            if let Some(replacement_tool) = deprecated_workflow_replacement(workflow) {
                map.insert("deprecated".to_owned(), json!(true));
                map.insert("replacement_tool".to_owned(), json!(replacement_tool));
                map.insert(
                    "removal_target".to_owned(),
                    json!(DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET),
                );
            }
            Value::Object(map)
        }
        other => json!({
            "workflow": workflow,
            "delegated_tool": delegated_tool,
            "deprecated": deprecated_workflow_replacement(workflow).is_some(),
            "replacement_tool": deprecated_workflow_replacement(workflow),
            "removal_target": deprecated_workflow_replacement(workflow)
                .map(|_| DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET),
            "result": other,
        }),
    }
}

/// Compatibility shim table for deprecated workflow aliases.
/// Each entry names the alias, the canonical replacement, and the release
/// in which the alias will be removed. Extend with care — a canonical
/// workflow MUST NOT appear here.
#[derive(Clone, Copy)]
struct DeprecatedAliasSpec {
    alias: &'static str,
    replacement_tool: &'static str,
    removal_target: &'static str,
    handler: ToolHandler,
}

const DEPRECATED_ALIASES: &[DeprecatedAliasSpec] = &[
    DeprecatedAliasSpec {
        alias: "audit_security_context",
        replacement_tool: "semantic_code_review",
        removal_target: DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET,
        handler: crate::tools::reports::semantic_code_review,
    },
    DeprecatedAliasSpec {
        alias: "analyze_change_impact",
        replacement_tool: "impact_report",
        removal_target: DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET,
        handler: crate::tools::reports::impact_report,
    },
    DeprecatedAliasSpec {
        alias: "assess_change_readiness",
        replacement_tool: "verify_change_readiness",
        removal_target: DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET,
        handler: crate::tools::reports::verify_change_readiness,
    },
];

fn attach_alias_metadata(spec: &DeprecatedAliasSpec, payload: Value) -> Value {
    let deprecation = json!({
        "deprecated": true,
        "replacement_tool": spec.replacement_tool,
        "removal_target": spec.removal_target,
    });
    match payload {
        Value::Object(mut map) => {
            map.insert("workflow".to_owned(), json!(spec.alias));
            map.insert("delegated_tool".to_owned(), json!(spec.replacement_tool));
            map.insert("deprecated".to_owned(), json!(true));
            map.insert("replacement_tool".to_owned(), json!(spec.replacement_tool));
            map.insert("removal_target".to_owned(), json!(spec.removal_target));
            map.insert("deprecation".to_owned(), deprecation);
            Value::Object(map)
        }
        other => json!({
            "workflow": spec.alias,
            "delegated_tool": spec.replacement_tool,
            "deprecated": true,
            "replacement_tool": spec.replacement_tool,
            "removal_target": spec.removal_target,
            "deprecation": deprecation,
            "result": other,
        }),
    }
}

fn dispatch_deprecated_alias(
    state: &AppState,
    alias: &'static str,
    arguments: &Value,
) -> ToolResult {
    let spec = DEPRECATED_ALIASES
        .iter()
        .find(|entry| entry.alias == alias)
        .ok_or_else(|| {
            CodeLensError::Validation(format!("unknown deprecated workflow alias: {alias}"))
        })?;
    let (payload, meta) = (spec.handler)(state, arguments)?;
    Ok((attach_alias_metadata(spec, payload), meta))
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

fn diagnose_path_scope(state: &AppState, path: &str) -> ToolResult {
    let project = state.project();
    let resolved = project.resolve(path)?;
    if resolved.is_file() {
        return Err(CodeLensError::Validation(
            "diagnose_issues with `path` requires a directory; use `file_path` for file diagnostics"
                .to_owned(),
        ));
    }
    let (scope, candidate_files) = if resolved.is_dir() {
        (
            "directory",
            codelens_engine::find_files(&project, "*", Some(path))?
                .into_iter()
                .map(|entry| entry.path)
                .collect::<Vec<_>>(),
        )
    } else {
        return Err(CodeLensError::NotFound(format!(
            "path does not exist in project scope: {path}"
        )));
    };

    let max_files = 8usize;
    let mut files = Vec::new();
    let mut skipped_files = Vec::new();
    let mut diagnosable_files = 0usize;
    let mut diagnostic_count = 0usize;

    for file_path in candidate_files {
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
                "max_results": 20,
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

    let payload = json!({
        "path": path,
        "scope": scope,
        "files": files,
        "returned_file_count": files.len(),
        "diagnosable_file_count": diagnosable_files,
        "diagnostic_count": diagnostic_count,
        "skipped_files": skipped_files,
        "count": diagnostic_count,
    });

    Ok((
        attach_workflow_metadata("diagnose_issues", "directory_diagnostics", payload),
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

pub fn audit_security_context(state: &AppState, arguments: &Value) -> ToolResult {
    dispatch_deprecated_alias(state, "audit_security_context", arguments)
}

pub fn analyze_change_impact(state: &AppState, arguments: &Value) -> ToolResult {
    dispatch_deprecated_alias(state, "analyze_change_impact", arguments)
}

pub fn review_changes(state: &AppState, arguments: &Value) -> ToolResult {
    let has_changed_files = arguments
        .get("changed_files")
        .and_then(|v| v.as_array())
        .is_some();
    let has_path = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .is_some_and(|value| !value.is_empty());

    if has_changed_files {
        return delegate_workflow(
            state,
            "review_changes",
            "diff_aware_references",
            arguments.clone(),
            crate::tools::reports::diff_aware_references,
        );
    }

    if has_path {
        return delegate_workflow(
            state,
            "review_changes",
            "impact_report",
            arguments.clone(),
            crate::tools::reports::impact_report,
        );
    }

    Err(CodeLensError::Validation(
        "review_changes requires `changed_files` or `path`".to_owned(),
    ))
}

pub fn assess_change_readiness(state: &AppState, arguments: &Value) -> ToolResult {
    dispatch_deprecated_alias(state, "assess_change_readiness", arguments)
}

pub fn diagnose_issues(state: &AppState, arguments: &Value) -> ToolResult {
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
                "max_results": arguments.get("max_results").and_then(|v| v.as_u64()),
            }),
            crate::tools::lsp::get_file_diagnostics,
        );
    }

    if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
        return diagnose_path_scope(state, path);
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
