use crate::protocol::SuggestedNextCall;
use serde_json::{Value, json};

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
