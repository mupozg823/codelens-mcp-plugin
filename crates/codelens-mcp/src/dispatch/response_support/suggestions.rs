use crate::protocol::{SuggestedNextCall, ToolCallResponse};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Restrict response-level follow-up metadata to an explicitly observed host
/// MCP inventory.
///
/// An omitted inventory keeps the historical suggestion behavior. An explicit
/// array, including an empty array, is authoritative for the current host: only
/// names advertised by the host survive, and foreign `mcp__<server>__` names
/// never become CodeLens suggestions by accident. The helper also cleans up
/// pre-existing calls/reasons so the three response fields remain a consistent
/// set for clients that consume one channel without the others.
pub(crate) fn filter_host_suggestions(response: &mut ToolCallResponse, arguments: &Value) {
    let Some(available_tools) = normalized_host_tool_inventory(arguments) else {
        return;
    };

    let Some(suggestions) = response.suggested_next_tools.take() else {
        response.suggested_next_calls = None;
        response.suggestion_reasons = None;
        return;
    };
    let suggestions = suggestions
        .into_iter()
        .filter(|tool| available_tools.contains(tool))
        .collect::<Vec<_>>();
    if suggestions.is_empty() {
        response.suggested_next_tools = None;
        response.suggested_next_calls = None;
        response.suggestion_reasons = None;
        return;
    }

    let suggestion_names = suggestions.iter().cloned().collect::<HashSet<_>>();
    response.suggested_next_tools = Some(suggestions);
    response.suggested_next_calls = response.suggested_next_calls.take().and_then(|calls| {
        let calls = calls
            .into_iter()
            .filter(|call| {
                available_tools.contains(&call.tool) && suggestion_names.contains(&call.tool)
            })
            .collect::<Vec<_>>();
        (!calls.is_empty()).then_some(calls)
    });
    response.suggestion_reasons = response.suggestion_reasons.take().and_then(|reasons| {
        let reasons = reasons
            .into_iter()
            .filter(|(tool, _)| available_tools.contains(tool) && suggestion_names.contains(tool))
            .collect::<std::collections::HashMap<_, _>>();
        (!reasons.is_empty()).then_some(reasons)
    });
}

/// Return the host's canonical CodeLens tool names when an inventory was
/// explicitly observed. Current session metadata cannot distinguish an omitted
/// inventory from an empty one, so only non-empty legacy session arrays narrow
/// suggestions. An explicit request array, including empty, takes precedence.
fn normalized_host_tool_inventory(arguments: &Value) -> Option<HashSet<String>> {
    let direct_inventory = arguments
        .get("available_mcp_tools")
        .filter(|value| value.is_array());
    let session_inventory = arguments
        .get("_session_available_mcp_tools")
        .filter(|value| value.is_array());

    let inventory = direct_inventory.or_else(|| {
        session_inventory.filter(|value| value.as_array().is_some_and(|items| !items.is_empty()))
    });
    inventory.map(normalize_host_tool_array)
}

fn normalize_host_tool_array(value: &Value) -> HashSet<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|tool| {
            let trimmed = tool.trim();
            if trimmed.is_empty()
                || (trimmed.starts_with("mcp__") && !trimmed.starts_with("mcp__codelens__"))
            {
                return None;
            }
            let canonical = trimmed
                .strip_prefix("mcp__codelens__")
                .unwrap_or(trimmed)
                .trim();
            crate::tool_defs::tool_definition(canonical).map(|_| canonical.to_owned())
        })
        .collect()
}

/// Build the additive `suggested_next_calls` list for the current response.
///
/// Additive companion to `suggested_next_tools` — never replaces it. Pre-fills
/// `arguments` for follow-up tools the server can unambiguously derive from
/// (a) the current call's own arguments, and (b) the fresh payload (most
/// usefully `analysis_id`). Applies only to bounded workflow/report follow-ups
/// where the server can forward scope without inventing new intent.
pub(crate) fn build_suggested_next_calls(
    current_tool: &str,
    current_args: &Value,
    next_tools: &[String],
    payload: Option<&Value>,
) -> Vec<SuggestedNextCall> {
    let analysis_id = payload
        .and_then(|p| p.get("analysis_id"))
        .and_then(|v| v.as_str());
    let task = current_args.get("task").and_then(|v| v.as_str());
    let changed_files = current_args.get("changed_files").cloned();
    let path = current_args.get("path").and_then(|v| v.as_str());
    let file_path = current_args
        .get("file_path")
        .and_then(|v| v.as_str())
        .or_else(|| current_args.get("relative_path").and_then(|v| v.as_str()));
    let symbol = current_args
        .get("symbol")
        .and_then(|v| v.as_str())
        .or_else(|| current_args.get("symbol_name").and_then(|v| v.as_str()))
        .or_else(|| current_args.get("name").and_then(|v| v.as_str()))
        .or_else(|| current_args.get("function_name").and_then(|v| v.as_str()))
        .or_else(|| current_args.get("entrypoint").and_then(|v| v.as_str()));
    let new_name = current_args.get("new_name").and_then(|v| v.as_str());
    let target_path = file_path.or(path);
    let single_changed_file = current_args
        .get("changed_files")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            if items.len() == 1 {
                items.first().and_then(|value| value.as_str())
            } else {
                None
            }
        });
    let diagnostic_path = target_path.or(single_changed_file);

    let mut calls = Vec::new();
    for next in next_tools.iter().take(3) {
        let call = match (current_tool, next.as_str()) {
            ("explore_codebase", "review_architecture") => target_path.map(|p| {
                SuggestedNextCall {
                    tool: "review_architecture".to_owned(),
                    arguments: json!({ "path": p }),
                    reason: "Escalate the same scoped exploration into an architecture review instead of starting over.".to_owned(),
                }
            }),
            ("explore_codebase", "analyze_change_impact" | "impact_report") => {
                target_path.map(|p| SuggestedNextCall {
                    tool: next.clone(),
                    arguments: json!({ "path": p }),
                    reason: "Carry the same scoped path into an impact pass without re-entering the target.".to_owned(),
                })
            }
            ("trace_request_path", "plan_safe_refactor") => symbol.map(|sym| SuggestedNextCall {
                tool: "plan_safe_refactor".to_owned(),
                arguments: json!({ "symbol": sym }),
                reason: "Use the traced entrypoint as the symbol anchor for refactor planning.".to_owned(),
            }),
            ("review_architecture", "plan_safe_refactor") => target_path.map(|p| {
                SuggestedNextCall {
                    tool: "plan_safe_refactor".to_owned(),
                    arguments: json!({ "path": p }),
                    reason: "Reuse the reviewed scope as the refactor planning target.".to_owned(),
                }
            }),
            ("review_architecture", "analyze_change_impact" | "impact_report") => {
                target_path.map(|p| SuggestedNextCall {
                    tool: next.clone(),
                    arguments: json!({ "path": p }),
                    reason: "Take the same architecture scope into an impact report instead of re-specifying it.".to_owned(),
                })
            }
            ("plan_safe_refactor", "trace_request_path") => symbol.map(|sym| SuggestedNextCall {
                tool: "trace_request_path".to_owned(),
                arguments: json!({ "symbol": sym }),
                reason: "Trace the same symbol before editing to confirm the execution path.".to_owned(),
            }),
            ("plan_safe_refactor", "analyze_change_impact" | "impact_report") => {
                target_path.map(|p| SuggestedNextCall {
                    tool: next.clone(),
                    arguments: json!({ "path": p }),
                    reason: "Assess blast radius for the same refactor scope without restating the target.".to_owned(),
                })
            }
            ("verify_change_readiness", "get_analysis_section") => analysis_id.map(|aid| {
                SuggestedNextCall {
                    tool: "get_analysis_section".to_owned(),
                    arguments: json!({
                        "analysis_id": aid,
                        "section": "verifier_diagnostics",
                    }),
                    reason: "Expand the diagnostics section of this readiness report instead of re-running verifier work.".to_owned(),
                }
            }),
            ("analyze_change_request", "verify_change_readiness") => task.map(|t| {
                let mut args = json!({ "task": t });
                if let Some(cf) = changed_files.clone() {
                    args["changed_files"] = cf;
                }
                SuggestedNextCall {
                    tool: "verify_change_readiness".to_owned(),
                    arguments: args,
                    reason: "Gate the same task through the verifier before any edit.".to_owned(),
                }
            }),
            ("impact_report", "diff_aware_references") => {
                changed_files.clone().map(|cf| SuggestedNextCall {
                    tool: "diff_aware_references".to_owned(),
                    arguments: json!({ "changed_files": cf }),
                    reason: "Drill into classified references for the same file set without re-running impact.".to_owned(),
                })
            }
            ("impact_report", "verify_change_readiness") => task.map(|t| {
                let mut args = json!({ "task": t });
                if let Some(cf) = changed_files.clone() {
                    args["changed_files"] = cf;
                }
                SuggestedNextCall {
                    tool: "verify_change_readiness".to_owned(),
                    arguments: args,
                    reason: "Promote the impact evidence into a readiness verdict for the same scope.".to_owned(),
                }
            }),
            ("review_changes", "impact_report") => {
                let mut args = json!({});
                if let Some(cf) = changed_files.clone() {
                    args["changed_files"] = cf;
                    Some(SuggestedNextCall {
                        tool: "impact_report".to_owned(),
                        arguments: args,
                        reason: "Re-run the broader impact view over the same changed files."
                            .to_owned(),
                    })
                } else if let Some(p) = target_path {
                    args["path"] = json!(p);
                    Some(SuggestedNextCall {
                        tool: "impact_report".to_owned(),
                        arguments: args,
                        reason: "Expand this review into a broader impact report for the same scope."
                            .to_owned(),
                    })
                } else {
                    None
                }
            }
            ("review_changes", "diagnose_issues") => diagnostic_path.map(|p| SuggestedNextCall {
                tool: "diagnose_issues".to_owned(),
                arguments: json!({ "path": p }),
                reason: "Drill into diagnostics for the same reviewed file while the scope is still warm.".to_owned(),
            }),
            ("diagnose_issues", "review_changes") => diagnostic_path.map(|p| SuggestedNextCall {
                tool: "review_changes".to_owned(),
                arguments: json!({ "path": p }),
                reason: "Promote the same file-level issue into a broader change review.".to_owned(),
            }),
            ("diagnose_issues", "find_symbol") => symbol.map(|sym| {
                let mut args = json!({ "name": sym });
                if let Some(p) = target_path {
                    args["file_path"] = json!(p);
                }
                SuggestedNextCall {
                    tool: "find_symbol".to_owned(),
                    arguments: args,
                    reason: "Jump directly to the implicated symbol instead of searching from scratch."
                        .to_owned(),
                }
            }),
            ("safe_rename_report", "rename_symbol") => match (file_path, symbol) {
                (Some(fp), Some(sym)) => {
                    let mut args = json!({
                        "file_path": fp,
                        "symbol_name": sym,
                        "dry_run": true,
                    });
                    if let Some(nn) = new_name {
                        args["new_name"] = json!(nn);
                    }
                    Some(SuggestedNextCall {
                        tool: "rename_symbol".to_owned(),
                        arguments: args,
                        reason: "Execute the rename previewed here; keep `dry_run=true` until diagnostics pass.".to_owned(),
                    })
                }
                _ => None,
            },
            // Generic fallback: any workflow tool that hands out an analysis_id
            // can be followed by get_analysis_section with the correct handle.
            (_, "get_analysis_section") => analysis_id.map(|aid| SuggestedNextCall {
                tool: "get_analysis_section".to_owned(),
                arguments: json!({ "analysis_id": aid, "section": "summary" }),
                reason: "Pull the summary section of this handle instead of re-running the workflow."
                    .to_owned(),
            }),
            _ => None,
        };
        if let Some(c) = call {
            calls.push(c);
        }
    }
    calls
}

#[cfg(test)]
mod host_inventory_tests {
    use super::*;

    fn response_with_suggestions() -> ToolCallResponse {
        let mut response = ToolCallResponse::error("test");
        response.suggested_next_tools = Some(vec!["search".to_owned(), "review".to_owned()]);
        response.suggested_next_calls = Some(vec![SuggestedNextCall {
            tool: "search".to_owned(),
            arguments: json!({}),
            reason: "search".to_owned(),
        }]);
        response.suggestion_reasons = Some(
            [("search".to_owned(), "search".to_owned())]
                .into_iter()
                .collect(),
        );
        response
    }

    #[test]
    fn host_inventory_keeps_codelens_facades_and_ignores_foreign_servers() {
        let mut response = response_with_suggestions();

        filter_host_suggestions(
            &mut response,
            &json!({
                "available_mcp_tools": [
                    " mcp__codelens__search ",
                    "mcp__github__review",
                    "search"
                ]
            }),
        );

        assert_eq!(
            response.suggested_next_tools,
            Some(vec!["search".to_owned()])
        );
        assert_eq!(
            response.suggested_next_calls.as_ref().map(|calls| calls
                .iter()
                .map(|call| call.tool.as_str())
                .collect::<Vec<_>>()),
            Some(vec!["search"])
        );
        assert_eq!(
            response
                .suggestion_reasons
                .as_ref()
                .map(|reasons| reasons.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["search"])
        );
    }

    #[test]
    fn host_inventory_preserves_payload_and_legacy_session_semantics() {
        let mut response = response_with_suggestions();
        response.data = Some(json!({
            "warnings": [{"code": "partial_index_coverage"}],
            "readiness": {"mutation_ready": "blocked"}
        }));
        let data = response.data.clone();
        filter_host_suggestions(&mut response, &json!({"_session_available_mcp_tools": []}));
        assert_eq!(response.suggested_next_tools.as_ref().unwrap().len(), 2);
        filter_host_suggestions(
            &mut response,
            &json!({
                "_session_available_mcp_tools": ["mcp__codelens__review"]
            }),
        );
        assert_eq!(
            response.suggested_next_tools,
            Some(vec!["review".to_owned()])
        );
        filter_host_suggestions(
            &mut response,
            &json!({
                "available_mcp_tools": [],
                "_session_available_mcp_tools": ["mcp__codelens__review"]
            }),
        );
        assert!(response.suggested_next_tools.is_none());
        assert!(response.suggested_next_calls.is_none());
        assert!(response.suggestion_reasons.is_none());
        assert_eq!(response.data, data);
    }
}
