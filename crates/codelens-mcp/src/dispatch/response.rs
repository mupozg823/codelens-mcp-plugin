use super::envelope::DetailLevel;
mod followups;
use super::response_support::{
    apply_contextual_guidance, bounded_result_payload, budget_hint, compact_response_payload,
    effective_budget_for_tool, max_result_size_chars_for_tool, primitive_response_payload,
    routing_hint_for_payload, success_jsonrpc_response, success_jsonrpc_response_with_meta,
    text_payload_for_response, text_payload_for_response_with_shape,
};
use crate::AppState;
use crate::error::CodeLensError;
use crate::mutation::gate::{MutationGateAllowance, MutationGateFailure, is_verifier_source_tool};
use crate::observability::telemetry::CallTelemetryHints;
use crate::protocol::{JsonRpcResponse, ToolCallResponse, ToolResponseMeta};
use crate::tool_defs::{ToolSurface, tool_definition};
use crate::tools;
use followups::{
    build_suggested_next_calls, delegate_hint_telemetry_fields,
    inject_delegate_to_codex_builder_hint,
};
use serde_json::json;

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
    /// Phase P1 response shape tier selected by
    /// [`super::envelope::default_detail_level`] or overridden via
    /// `_detail` / `_compact` argument. `compact` above is the boolean
    /// back-compat view of the same decision.
    pub detail: DetailLevel,
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
        detail,
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

    match detail {
        DetailLevel::Primitive => primitive_response_payload(&mut resp),
        DetailLevel::Core => compact_response_payload(&mut resp),
        DetailLevel::Rich => {
            if compact {
                compact_response_payload(&mut resp);
            }
        }
    }

    let effort_offset = state.effort_level().compression_threshold_offset();
    let text = text_payload_for_response_with_shape(
        &resp,
        structured_content.as_ref(),
        detail.is_primitive(),
    );
    // Phase P1: in primitive mode skip the structuredContent
    // duplication of the text body to stay inside the Serena-class
    // byte envelope. Callers needing typed access explicitly request
    // `_detail="rich"` or `_detail="core"`.
    let structured_content = if detail.is_primitive() {
        None
    } else {
        structured_content
    };
    let (text, structured_content, truncated) = bounded_result_payload(
        text,
        structured_content,
        payload_estimate,
        effective_budget,
        effort_offset,
    );
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
    if detail.is_primitive() {
        success_jsonrpc_response_with_meta(
            id,
            name,
            text,
            structured_content,
            Some(max_result_size),
            /* include_meta = */ false,
        )
    } else {
        success_jsonrpc_response(id, name, text, structured_content, Some(max_result_size))
    }
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
    let body = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
        "_meta": {
            "codelens/preferredExecutor": crate::tool_defs::tool_preferred_executor_label(name)
        }
    });
    JsonRpcResponse::result(id, body)
}
