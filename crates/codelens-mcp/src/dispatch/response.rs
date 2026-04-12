use super::response_support::{
    apply_contextual_guidance, bounded_result_payload, budget_hint, compact_response_payload,
    effective_budget_for_tool, max_result_size_chars_for_tool, routing_hint_for_payload,
    success_jsonrpc_response, text_payload_for_response,
};
use crate::AppState;
use crate::error::CodeLensError;
use crate::mutation_gate::{MutationGateAllowance, MutationGateFailure, is_verifier_source_tool};
use crate::protocol::{JsonRpcResponse, ToolCallResponse, ToolResponseMeta};
use crate::tool_defs::{ToolSurface, tool_definition};
use crate::tools;
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
        state.metrics().record_mutation_with_caution();
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
    }

    if compact {
        compact_response_payload(&mut resp);
    }

    let effort_offset = state.effort_level().compression_threshold_offset();
    let text = text_payload_for_response(&resp, structured_content.as_ref());
    let (text, structured_content, truncated) = bounded_result_payload(
        text,
        structured_content,
        payload_estimate,
        effective_budget,
        effort_offset,
    );

    state.metrics().record_call_with_tokens(
        name,
        elapsed_ms as u64,
        true,
        payload_estimate,
        active_surface,
        truncated,
        harness_phase,
    );
    if emitted_composite_guidance {
        state.metrics().record_composite_guidance_emitted();
    }

    let max_result_size = max_result_size_chars_for_tool(name, truncated);
    success_jsonrpc_response(id, text, structured_content, Some(max_result_size))
}

pub(crate) fn build_error_response(
    name: &str,
    error: CodeLensError,
    gate_failure: Option<MutationGateFailure>,
    active_surface: &str,
    state: &AppState,
    start: std::time::Instant,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let elapsed_ms = start.elapsed().as_millis();

    state.metrics().record_call_with_tokens(
        name,
        elapsed_ms as u64,
        false,
        0,
        active_surface,
        false,
        None,
    );

    if error.is_protocol_error() {
        return JsonRpcResponse::error(id, error.jsonrpc_code(), error.to_string());
    }

    let mut resp = ToolCallResponse::error(error.to_string());
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
    let text = serde_json::to_string(&resp)
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned());
    JsonRpcResponse::result(
        id,
        json!({
            "content": [{ "type": "text", "text": text }],
            "isError": true
        }),
    )
}
