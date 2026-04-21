use crate::protocol::{SuggestedNextCall, ToolCallResponse};
use crate::tool_defs;
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static HANDOFF_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(super) fn delegate_hint_telemetry_fields(
    resp: &ToolCallResponse,
) -> (Option<&str>, Option<&str>, Option<&str>) {
    resp.suggested_next_calls
        .as_ref()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.tool == "delegate_to_codex_builder"
                    && call
                        .arguments
                        .get("trigger")
                        .and_then(|value| value.as_str())
                        .is_some()
            })
        })
        .map(|call| {
            (
                call.arguments
                    .get("trigger")
                    .and_then(|value| value.as_str()),
                call.arguments
                    .get("delegate_tool")
                    .and_then(|value| value.as_str()),
                call.arguments
                    .get("handoff_id")
                    .and_then(|value| value.as_str()),
            )
        })
        .unwrap_or((None, None, None))
}

pub(super) fn build_suggested_next_calls(
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
            ("explore_codebase", "review_architecture") => {
                target_path.map(|p| SuggestedNextCall {
                    tool: "review_architecture".to_owned(),
                    arguments: json!({ "path": p }),
                    reason: "Escalate the same scoped exploration into an architecture review instead of starting over.".to_owned(),
                })
            }
            ("explore_codebase", "impact_report") => {
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
            ("review_architecture", "plan_safe_refactor") => {
                target_path.map(|p| SuggestedNextCall {
                    tool: "plan_safe_refactor".to_owned(),
                    arguments: json!({ "path": p }),
                    reason: "Reuse the reviewed scope as the refactor planning target.".to_owned(),
                })
            }
            ("review_architecture", "impact_report") => {
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
            ("plan_safe_refactor", "impact_report") => {
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
            ("impact_report", "diff_aware_references") => changed_files.clone().map(|cf| {
                SuggestedNextCall {
                    tool: "diff_aware_references".to_owned(),
                    arguments: json!({ "changed_files": cf }),
                    reason: "Drill into classified references for the same file set without re-running impact.".to_owned(),
                }
            }),
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
                        reason: "Re-run the broader impact view over the same changed files.".to_owned(),
                    })
                } else if let Some(p) = target_path {
                    args["path"] = json!(p);
                    Some(SuggestedNextCall {
                        tool: "impact_report".to_owned(),
                        arguments: args,
                        reason: "Expand this review into a broader impact report for the same scope.".to_owned(),
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
                    reason: "Jump directly to the implicated symbol instead of searching from scratch.".to_owned(),
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

pub(super) fn inject_delegate_to_codex_builder_hint(
    current_tool: &str,
    current_args: &Value,
    payload: Option<&Value>,
    next_tools: &mut Vec<String>,
    next_calls: &mut Vec<SuggestedNextCall>,
    doom_loop_count: usize,
    doom_loop_rapid: bool,
) {
    const DELEGATE_TOOL: &str = "delegate_to_codex_builder";

    if next_tools.iter().any(|tool| tool == DELEGATE_TOOL) {
        return;
    }

    let current_executor = tool_defs::tool_preferred_executor_label(current_tool);
    let candidate = if current_executor == "codex-builder" && doom_loop_count >= 3 {
        Some((
            current_tool.to_owned(),
            Some(current_args.clone()),
            "builder_doom_loop",
            if doom_loop_rapid {
                "Repeated rapid builder retries detected. Move this step to a Codex-class builder session."
            } else {
                "Repeated builder-heavy retries detected. Move this step to a Codex-class builder session."
            },
        ))
    } else {
        codex_builder_candidate_from_suggestions(current_executor, next_tools, next_calls)
    };

    let Some((delegate_tool, delegate_arguments, trigger, reason)) = candidate else {
        return;
    };

    let delegate_call = SuggestedNextCall {
        tool: DELEGATE_TOOL.to_owned(),
        arguments: build_delegate_to_codex_builder_arguments(CodexBuilderDelegateInput {
            current_tool,
            current_args,
            payload,
            delegate_tool: &delegate_tool,
            delegate_arguments,
            trigger,
            doom_loop_count,
            doom_loop_rapid,
        }),
        reason: reason.to_owned(),
    };

    next_tools.insert(0, DELEGATE_TOOL.to_owned());
    if next_tools.len() > 4 {
        next_tools.truncate(4);
    }
    next_calls.insert(0, delegate_call);
    if next_calls.len() > 4 {
        next_calls.truncate(4);
    }
}

fn codex_builder_candidate_from_suggestions(
    current_executor: &str,
    next_tools: &[String],
    next_calls: &[SuggestedNextCall],
) -> Option<(String, Option<Value>, &'static str, &'static str)> {
    if current_executor == "codex-builder" {
        return None;
    }

    if let Some(call) = next_calls
        .iter()
        .find(|call| tool_defs::tool_preferred_executor_label(&call.tool) == "codex-builder")
    {
        return Some((
            call.tool.clone(),
            Some(call.arguments.clone()),
            "preferred_executor_boundary",
            "The next recommended step is builder-heavy. Hand it off with the attached Codex builder scaffold.",
        ));
    }

    next_tools
        .iter()
        .find(|tool| tool_defs::tool_preferred_executor_label(tool) == "codex-builder")
        .map(|tool| {
            (
                tool.clone(),
                None,
                "preferred_executor_boundary",
                "The next recommended step is builder-heavy. Hand it off with the attached Codex builder scaffold.",
            )
        })
}

struct CodexBuilderDelegateInput<'a> {
    current_tool: &'a str,
    current_args: &'a Value,
    payload: Option<&'a Value>,
    delegate_tool: &'a str,
    delegate_arguments: Option<Value>,
    trigger: &'a str,
    doom_loop_count: usize,
    doom_loop_rapid: bool,
}

fn build_delegate_to_codex_builder_arguments(input: CodexBuilderDelegateInput<'_>) -> Value {
    let handoff_id = generate_delegate_handoff_id();
    let mut carry_forward = Map::new();
    for key in [
        "task",
        "changed_files",
        "path",
        "file_path",
        "relative_path",
        "symbol",
        "symbol_name",
        "name",
        "new_name",
    ] {
        if let Some(value) = input.current_args.get(key) {
            carry_forward.insert(key.to_owned(), value.clone());
        }
    }
    carry_forward.insert("handoff_id".to_owned(), json!(handoff_id.clone()));
    if let Some(analysis_id) = input
        .payload
        .and_then(|value| value.get("analysis_id"))
        .and_then(|value| value.as_str())
    {
        carry_forward.insert("analysis_id".to_owned(), json!(analysis_id));
    }

    let objective = input
        .current_args
        .get("task")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .or_else(|| {
            input
                .current_args
                .get("symbol")
                .and_then(|value| value.as_str())
                .map(|symbol| format!("continue work on symbol `{symbol}`"))
        })
        .or_else(|| {
            input
                .current_args
                .get("symbol_name")
                .and_then(|value| value.as_str())
                .map(|symbol| format!("continue work on symbol `{symbol}`"))
        })
        .unwrap_or_else(|| format!("continue with `{}`", input.delegate_tool));

    let why_delegate = match input.trigger {
        "builder_doom_loop" if input.doom_loop_rapid => format!(
            "The same builder-heavy step repeated {} times in a rapid burst. Switch to a Codex builder lane instead of retrying inline.",
            input.doom_loop_count
        ),
        "builder_doom_loop" => format!(
            "The same builder-heavy step repeated {} times. Switch to a Codex builder lane before continuing.",
            input.doom_loop_count
        ),
        _ => format!(
            "`{}` is tagged `codex-builder`, while `{}` is not. Keep orchestration here and move execution to a builder session.",
            input.delegate_tool, input.current_tool
        ),
    };

    let mut result = json!({
        "handoff_id": handoff_id.clone(),
        "preferred_executor": "codex-builder",
        "delegate_tool": input.delegate_tool,
        "source_tool": input.current_tool,
        "trigger": input.trigger,
        "briefing": {
            "objective": objective,
            "why_delegate": why_delegate,
            "completion_contract": [
                "Execute only the delegated builder step and any immediately required diagnostics.",
                "After mutation, run get_file_diagnostics before returning control.",
                "Return changed files, unresolved blockers, and the next safe planner-facing action."
            ]
        }
    });

    if !carry_forward.is_empty() {
        result["carry_forward"] = Value::Object(carry_forward);
    }
    let mut delegate_arguments = input
        .delegate_arguments
        .unwrap_or_else(|| Value::Object(Map::new()));
    if let Value::Object(arguments) = &mut delegate_arguments {
        arguments.insert("handoff_id".to_owned(), json!(handoff_id));
    }
    result["delegate_arguments"] = delegate_arguments;

    result
}

fn generate_delegate_handoff_id() -> String {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = HANDOFF_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("codelens-handoff-{timestamp_ms:x}-{sequence:x}")
}
