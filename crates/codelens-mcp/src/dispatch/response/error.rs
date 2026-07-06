use crate::AppState;
use crate::dispatch::response_support::{
    delegate_hint_telemetry_fields, inject_delegate_to_codex_builder_hint,
    text_payload_for_response,
};
use crate::error::CodeLensError;
use crate::mutation_gate::MutationGateFailure;
use crate::protocol::{JsonRpcResponse, ToolCallResponse};
use crate::telemetry::CallTelemetryHints;
use crate::tools;
use serde_json::json;

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
    let text = text_payload_for_response(&resp, None, false);
    let mut body = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
        "_meta": {
            "codelens/preferredExecutor": crate::tool_defs::tool_preferred_executor_label(name)
        }
    });
    crate::tool_defs::apply_tool_deprecation_meta(&mut body["_meta"], name);
    JsonRpcResponse::result(id, body)
}
