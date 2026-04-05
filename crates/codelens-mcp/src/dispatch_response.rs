use crate::dispatch_response_support::{
    apply_contextual_guidance, bounded_result_payload, budget_hint, compact_response_payload,
    effective_budget_for_tool, routing_hint_for_payload, success_jsonrpc_response,
};
use crate::error::CodeLensError;
use crate::mutation_gate::{is_verifier_source_tool, MutationGateAllowance, MutationGateFailure};
use crate::protocol::{JsonRpcResponse, ToolCallResponse, ToolResponseMeta};
use crate::tool_defs::{tool_definition, ToolSurface};
use crate::tools;
use crate::AppState;
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
    pub gate_allowance: Option<&'a MutationGateAllowance>,
    pub compact: bool,
    pub harness_phase: Option<&'a str>,
    pub request_budget: usize,
    pub start: std::time::Instant,
    pub id: Option<serde_json::Value>,
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
        gate_allowance,
        compact,
        harness_phase,
        request_budget,
        start,
        id,
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
    resp.token_estimate = Some(payload_estimate);
    resp.budget_hint = Some(hint);
    resp.elapsed_ms = Some(elapsed_ms as u64);
    resp.routing_hint = Some(routing_hint_for_payload(&resp));

    let emitted_composite_guidance = apply_contextual_guidance(
        &mut resp,
        name,
        &state.recent_tools(),
        harness_phase,
        surface,
    );

    if compact {
        compact_response_payload(&mut resp);
    }

    let text = serde_json::to_string(&resp)
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned());
    let (text, structured_content, truncated) =
        bounded_result_payload(text, structured_content, payload_estimate, effective_budget);

    state.metrics().record_call_with_tokens(
        name,
        elapsed_ms as u64,
        true,
        payload_estimate,
        active_surface,
        truncated,
    );
    if emitted_composite_guidance {
        state.metrics().record_composite_guidance_emitted();
    }

    success_jsonrpc_response(id, text, structured_content)
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
        resp.error = Some(format!("{}{}", failure.message, analysis_hint));
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
