use super::response_support::{
    apply_contextual_guidance, bounded_result_payload, budget_hint, compact_response_payload,
    effective_budget_for_tool, enrich_recovery_hint_for_signals, max_result_size_chars_for_tool,
    routing_hint_for_payload, success_jsonrpc_response, text_payload_for_response,
};
use crate::AppState;
use crate::error::CodeLensError;
use crate::mutation_gate::{MutationGateAllowance, MutationGateFailure, is_verifier_source_tool};
use crate::protocol::{JsonRpcResponse, SuggestedNextCall, ToolCallResponse, ToolResponseMeta};
use crate::telemetry::CallTelemetryHints;
use crate::tool_defs::{ToolSurface, tool_definition};
use crate::tools;
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static HANDOFF_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) struct SuccessResponseInput<'a> {
    pub name: &'a str,
    pub payload: serde_json::Value,
    pub meta: ToolResponseMeta,
    pub state: &'a AppState,
    pub surface: ToolSurface,
    pub active_surface: &'a str,
    pub arguments: &'a serde_json::Value,
    pub logical_session_id: &'a str,
    pub recent_tools: Vec<String>,
    pub gate_allowance: Option<&'a MutationGateAllowance>,
    pub compact: bool,
    pub harness_phase: Option<&'a str>,
    pub request_budget: usize,
    pub start: std::time::Instant,
    pub id: Option<serde_json::Value>,
    /// Consecutive same-tool+args call count for doom-loop detection.
    pub doom_loop_count: usize,
    /// True when 3+ identical calls happen within 10 seconds (agent retry loop).
    pub doom_loop_rapid: bool,
}

pub(crate) fn build_success_response(input: SuccessResponseInput<'_>) -> JsonRpcResponse {
    let SuccessResponseInput {
        name,
        payload,
        meta,
        state,
        surface,
        active_surface,
        arguments,
        logical_session_id,
        recent_tools,
        gate_allowance,
        compact,
        harness_phase,
        request_budget,
        start,
        id,
        doom_loop_count,
        doom_loop_rapid,
    } = input;

    let elapsed_ms = start.elapsed().as_millis();

    // Apply per-tool hard cap if defined (stricter than global budget)
    let effective_budget = effective_budget_for_tool(name, request_budget);

    if is_verifier_source_tool(name) {
        state.record_recent_preflight_from_payload(
            name,
            active_surface,
            logical_session_id,
            arguments,
            &payload,
        );
    }

    let had_caution = gate_allowance.map(|a| a.caution) == Some(true);
    if had_caution {
        state
            .metrics()
            .record_mutation_with_caution_for_session(Some(logical_session_id));
    }

    // Mutation allowed with caution = no fresh preflight was found
    let missing_preflight = had_caution;

    let has_output_schema = tool_definition(name)
        .and_then(|tool| tool.output_schema.as_ref())
        .is_some();
    let structured_content = has_output_schema.then(|| payload.clone());

    let mut resp = ToolCallResponse::success(payload, meta);

    let payload_estimate = serde_json::to_string(&resp.data)
        .map(|s| tools::estimate_tokens(&s))
        .unwrap_or(0);
    let mut hint = budget_hint(name, payload_estimate, effective_budget);
    if missing_preflight {
        hint = format!("{hint} Tip: run verify_change_readiness before mutations for safer edits.");
    }
    if doom_loop_count >= 3 {
        if doom_loop_rapid {
            hint = format!(
                "{hint} Rapid retry burst detected ({doom_loop_count}x in <10s). \
                 Use start_analysis_job for heavy analysis, or narrow scope with path/max_tokens."
            );
        } else {
            hint = format!(
                "{hint} Repeated low-level chain detected. Prefer verify_change_readiness, \
                 find_minimal_context_for_change, analyze_change_request for compressed context."
            );
        }
    }
    resp.token_estimate = Some(payload_estimate);
    resp.budget_hint = Some(hint);
    resp.elapsed_ms = Some(elapsed_ms as u64);
    resp.routing_hint = Some(routing_hint_for_payload(&resp));
    resp.reasoning_scaffold = tools::reasoning_scaffold::reasoning_scaffold_for(name);

    let emitted_composite_guidance =
        apply_contextual_guidance(&mut resp, name, &recent_tools, harness_phase, surface);

    // Self-evolution: when doom-loop detected, override suggestions with alternative tools
    if doom_loop_count >= 3 {
        if doom_loop_rapid {
            // Rapid burst: suggest async path to break the retry loop
            resp.suggested_next_tools = Some(vec![
                "start_analysis_job".to_owned(),
                "find_minimal_context_for_change".to_owned(),
                "get_ranked_context".to_owned(),
            ]);
        } else {
            resp.suggested_next_tools = Some(vec![
                "verify_change_readiness".to_owned(),
                "find_minimal_context_for_change".to_owned(),
                "analyze_change_request".to_owned(),
            ]);
        }
    }

    if let Some(ref next_tools) = resp.suggested_next_tools {
        resp.suggestion_reasons = Some(tools::suggestion_reasons_for(next_tools, name));
        let mut calls = build_suggested_next_calls(name, arguments, next_tools, resp.data.as_ref());
        let mut next_tools = next_tools.clone();
        inject_delegate_to_codex_builder_hint(
            name,
            arguments,
            resp.data.as_ref(),
            &mut next_tools,
            &mut calls,
            doom_loop_count,
            doom_loop_rapid,
        );
        resp.suggested_next_tools = Some(next_tools);
        if !calls.is_empty() {
            resp.suggested_next_calls = Some(calls);
        }
        resp.suggestion_reasons = resp
            .suggested_next_tools
            .as_ref()
            .map(|tools| tools::suggestion_reasons_for(tools, name));
    }

    if compact {
        compact_response_payload(&mut resp);
    }

    let effort_offset = state.effort_level().compression_threshold_offset();
    let text = text_payload_for_response(&resp, structured_content.as_ref());
    let (text, mut structured_content, truncation_info) = bounded_result_payload(
        text,
        structured_content,
        payload_estimate,
        effective_budget,
        effort_offset,
    );
    // S2: when the response was clipped AND structured signals show the
    // call-graph extractor only emitted unresolved edges, replace the
    // generic budget-narrowing recovery hint with a grep-fallback cue
    // that names the symbol — retrying with smaller max_results would
    // not recover edges the extractor failed to discover.
    let truncation_info = truncation_info
        .map(|info| enrich_recovery_hint_for_signals(info, structured_content.as_ref()));
    let truncated = truncation_info.is_some();
    // Surface the truncation envelope at the top level of structured_content
    // so an agent does not have to reach into the data envelope to discover
    // arrays were clipped. Pre-PR101 dogfood case (Flask `route` callers
    // 287→3) was a recall regression hidden by stage-5 compression.
    if let (Some(info), Some(Value::Object(map))) =
        (truncation_info.as_ref(), structured_content.as_mut())
    {
        map.insert("truncation_warning".to_owned(), info.to_json());
    }
    let suggested_next_tools = resp.suggested_next_tools.as_deref().unwrap_or(&[]);
    let handoff_id = arguments.get("handoff_id").and_then(|value| value.as_str());
    let (delegate_hint_trigger, delegate_target_tool, delegate_handoff_id) =
        delegate_hint_telemetry_fields(&resp);

    let target_paths = state.extract_target_paths(arguments);
    state.metrics().record_call_with_targets_for_session(
        name,
        elapsed_ms as u64,
        true,
        payload_estimate,
        active_surface,
        truncated,
        harness_phase,
        Some(logical_session_id),
        &target_paths,
        CallTelemetryHints {
            suggested_next_tools,
            delegate_hint_trigger,
            delegate_target_tool,
            delegate_handoff_id,
            handoff_id,
        },
    );
    if emitted_composite_guidance
        && !matches!(name, "get_tool_metrics" | "set_profile" | "set_preset")
    {
        state
            .metrics()
            .record_composite_guidance_emitted_for_session(name, Some(logical_session_id));
    }

    let max_result_size = max_result_size_chars_for_tool(name, truncated);
    success_jsonrpc_response(id, name, text, structured_content, Some(max_result_size))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_error_response(
    name: &str,
    error: CodeLensError,
    gate_failure: Option<MutationGateFailure>,
    arguments: &serde_json::Value,
    active_surface: &str,
    logical_session_id: &str,
    state: &AppState,
    start: std::time::Instant,
    id: Option<serde_json::Value>,
    doom_loop_count: usize,
    doom_loop_rapid: bool,
) -> JsonRpcResponse {
    let elapsed_ms = start.elapsed().as_millis();

    let target_paths = state.extract_target_paths(arguments);

    if error.is_protocol_error() {
        state.metrics().record_call_with_targets_for_session(
            name,
            elapsed_ms as u64,
            false,
            0,
            active_surface,
            false,
            None,
            Some(logical_session_id),
            &target_paths,
            CallTelemetryHints::default(),
        );
        return JsonRpcResponse::error(id, error.jsonrpc_code(), error.to_string());
    }

    // Derive the structured recovery hint before consuming the error via `to_string()`.
    let recovery_hint = error.recovery_hint();

    let mut resp = ToolCallResponse::error(error.to_string());
    resp.recovery_hint = recovery_hint;
    if let Some(failure) = gate_failure {
        let analysis_hint = failure
            .analysis_id
            .as_ref()
            .map(|analysis_id| format!(" Last related analysis_id: `{analysis_id}`."))
            .unwrap_or_default();
        resp.error = Some(format!(
            "[{:?}] {}{}",
            failure.kind, failure.message, analysis_hint
        ));
        resp.suggested_next_tools = Some(failure.suggested_next_tools);
        resp.budget_hint = Some(failure.budget_hint);
    }
    let mut next_tools = resp.suggested_next_tools.take().unwrap_or_default();
    let mut next_calls = resp.suggested_next_calls.take().unwrap_or_default();
    inject_delegate_to_codex_builder_hint(
        name,
        arguments,
        None,
        &mut next_tools,
        &mut next_calls,
        doom_loop_count,
        doom_loop_rapid,
    );
    if !next_tools.is_empty() {
        resp.suggested_next_tools = Some(next_tools);
        resp.suggestion_reasons = resp
            .suggested_next_tools
            .as_ref()
            .map(|tools| tools::suggestion_reasons_for(tools, name));
    }
    if !next_calls.is_empty() {
        resp.suggested_next_calls = Some(next_calls);
    }
    let suggested_next_tools = resp.suggested_next_tools.as_deref().unwrap_or(&[]);
    let handoff_id = arguments.get("handoff_id").and_then(|value| value.as_str());
    let (delegate_hint_trigger, delegate_target_tool, delegate_handoff_id) =
        delegate_hint_telemetry_fields(&resp);
    state.metrics().record_call_with_targets_for_session(
        name,
        elapsed_ms as u64,
        false,
        0,
        active_surface,
        false,
        None,
        Some(logical_session_id),
        &target_paths,
        CallTelemetryHints {
            suggested_next_tools,
            delegate_hint_trigger,
            delegate_target_tool,
            delegate_handoff_id,
            handoff_id,
        },
    );
    let text = text_payload_for_response(&resp, None);
    let mut body = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
        "_meta": {
            "codelens/preferredExecutor": crate::tool_defs::tool_preferred_executor_label(name)
        }
    });
    if let Some((since, replacement, removal)) = crate::tool_defs::tool_deprecation(name) {
        body["_meta"]["codelens/deprecatedSince"] = json!(since);
        body["_meta"]["codelens/deprecatedReplacement"] = json!(replacement);
        body["_meta"]["codelens/deprecatedRemovalTarget"] = json!(removal);
    }
    JsonRpcResponse::result(id, body)
}

fn delegate_hint_telemetry_fields(
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

/// Build the additive `suggested_next_calls` list for the current response.
///
/// Additive companion to `suggested_next_tools` — never replaces it. Pre-fills
/// `arguments` for follow-up tools the server can unambiguously derive from
/// (a) the current call's own arguments, and (b) the fresh payload (most
/// usefully `analysis_id`). Applies only to bounded workflow/report follow-ups
/// where the server can forward scope without inventing new intent.
fn build_suggested_next_calls(
    current_tool: &str,
    current_args: &serde_json::Value,
    next_tools: &[String],
    payload: Option<&serde_json::Value>,
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
            ("review_architecture", "plan_safe_refactor") => {
                target_path.map(|p| SuggestedNextCall {
                    tool: "plan_safe_refactor".to_owned(),
                    arguments: json!({ "path": p }),
                    reason: "Reuse the reviewed scope as the refactor planning target.".to_owned(),
                })
            }
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
            ("safe_rename_report", "rename_symbol") => {
                match (file_path, symbol) {
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
                }
            }
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

fn inject_delegate_to_codex_builder_hint(
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

    let current_executor = crate::tool_defs::tool_preferred_executor_label(current_tool);
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
        arguments: build_delegate_to_codex_builder_arguments(
            current_tool,
            current_args,
            payload,
            &delegate_tool,
            delegate_arguments,
            trigger,
            doom_loop_count,
            doom_loop_rapid,
        ),
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
        .find(|call| crate::tool_defs::tool_preferred_executor_label(&call.tool) == "codex-builder")
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
        .find(|tool| crate::tool_defs::tool_preferred_executor_label(tool) == "codex-builder")
        .map(|tool| {
            (
                tool.clone(),
                None,
                "preferred_executor_boundary",
                "The next recommended step is builder-heavy. Hand it off with the attached Codex builder scaffold.",
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn build_delegate_to_codex_builder_arguments(
    current_tool: &str,
    current_args: &Value,
    payload: Option<&Value>,
    delegate_tool: &str,
    delegate_arguments: Option<Value>,
    trigger: &str,
    doom_loop_count: usize,
    doom_loop_rapid: bool,
) -> Value {
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
        if let Some(value) = current_args.get(key) {
            carry_forward.insert(key.to_owned(), value.clone());
        }
    }
    carry_forward.insert("handoff_id".to_owned(), json!(handoff_id.clone()));
    if let Some(analysis_id) = payload
        .and_then(|value| value.get("analysis_id"))
        .and_then(|value| value.as_str())
    {
        carry_forward.insert("analysis_id".to_owned(), json!(analysis_id));
    }

    let objective = current_args
        .get("task")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .or_else(|| {
            current_args
                .get("symbol")
                .and_then(|value| value.as_str())
                .map(|symbol| format!("continue work on symbol `{symbol}`"))
        })
        .or_else(|| {
            current_args
                .get("symbol_name")
                .and_then(|value| value.as_str())
                .map(|symbol| format!("continue work on symbol `{symbol}`"))
        })
        .unwrap_or_else(|| format!("continue with `{delegate_tool}`"));

    let why_delegate = match trigger {
        "builder_doom_loop" if doom_loop_rapid => format!(
            "The same builder-heavy step repeated {doom_loop_count} times in a rapid burst. Switch to a Codex builder lane instead of retrying inline."
        ),
        "builder_doom_loop" => format!(
            "The same builder-heavy step repeated {doom_loop_count} times. Switch to a Codex builder lane before continuing."
        ),
        _ => format!(
            "`{delegate_tool}` is tagged `codex-builder`, while `{current_tool}` is not. Keep orchestration here and move execution to a builder session."
        ),
    };

    let mut result = json!({
        "handoff_id": handoff_id.clone(),
        "preferred_executor": "codex-builder",
        "delegate_tool": delegate_tool,
        "source_tool": current_tool,
        "trigger": trigger,
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
    let mut delegate_arguments = delegate_arguments.unwrap_or_else(|| Value::Object(Map::new()));
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
