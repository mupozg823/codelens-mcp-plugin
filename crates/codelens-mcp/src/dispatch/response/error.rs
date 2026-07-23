use crate::AppState;
use crate::dispatch::response_support::text_payload_for_response;
use crate::error::CodeLensError;
use crate::mutation_gate::MutationGateFailure;
use crate::operation::ResolvedOperation;
use crate::protocol::{JsonRpcError, JsonRpcResponse, ToolCallResponse};
use crate::telemetry::{CallTelemetryHints, ToolCallEvent};
use crate::tools;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_error_response<'a>(
    name: &'a str,
    error: CodeLensError,
    gate_failure: Option<MutationGateFailure>,
    arguments: &'a serde_json::Value,
    active_surface: &str,
    logical_session_id: &str,
    state: &AppState,
    start: std::time::Instant,
    id: Option<serde_json::Value>,
    _doom_loop_count: usize,
    _doom_loop_rapid: bool,
    operation: Option<ResolvedOperation<'a>>,
) -> JsonRpcResponse {
    let elapsed_ms = start.elapsed().as_millis();
    let operation = operation.unwrap_or_else(|| ResolvedOperation::from_request(name, arguments));

    let target_paths = state.extract_target_paths(arguments);

    if error.is_protocol_error() {
        state.metrics().record_event(ToolCallEvent {
            tool: name,
            operation,
            elapsed_ms: elapsed_ms as u64,
            tokens: 0,
            success: false,
            surface: active_surface,
            truncated: false,
            phase: None,
            logical_session_id: Some(logical_session_id),
            client_name: arguments
                .get("_session_client_name")
                .and_then(|value| value.as_str()),
            target_paths: &target_paths,
            hints: CallTelemetryHints::default(),
        });
        // Protocol errors used to terminate as a bare JSON-RPC string. Carry
        // the structured recovery hint (RequireField / did-you-mean +
        // get_capabilities fallback) in `error.data.recovery_hint` so agents
        // can self-correct without re-parsing the message.
        let known_tools: Vec<&str> = crate::dispatch::table::DISPATCH_TABLE
            .keys()
            .copied()
            .collect();
        let (message, data) = error.protocol_error_data(name, &known_tools);
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: error.jsonrpc_code(),
                message,
                data,
            }),
        };
    }

    // Derive structured recovery metadata before consuming the error via
    // `to_string()`.
    let recovery_hint = error.recovery_hint();
    let retryable = error.is_retryable();

    let mut resp = ToolCallResponse::error(error.to_string());
    resp.recovery_hint = recovery_hint;
    resp.retryable = retryable.then_some(true);
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
    if resp.suggested_next_tools.is_some() {
        resp.suggestion_reasons = resp
            .suggested_next_tools
            .as_ref()
            .map(|tools| tools::suggestion_reasons_for(tools, name));
    }
    if crate::host_capabilities::HostCapabilities::for_request(state, arguments, logical_session_id)
        .is_some_and(|capabilities| capabilities.native_tool_search)
    {
        resp.suggested_next_tools = None;
        resp.suggested_next_calls = None;
        resp.suggestion_reasons = None;
    }
    let suggested_next_tools = resp.suggested_next_tools.as_deref().unwrap_or(&[]);
    let handoff_id = arguments.get("handoff_id").and_then(|value| value.as_str());
    state.metrics().record_event(ToolCallEvent {
        tool: name,
        operation,
        elapsed_ms: elapsed_ms as u64,
        tokens: 0,
        success: false,
        surface: active_surface,
        truncated: false,
        phase: None,
        logical_session_id: Some(logical_session_id),
        client_name: arguments
            .get("_session_client_name")
            .and_then(|value| value.as_str()),
        target_paths: &target_paths,
        hints: CallTelemetryHints {
            suggested_next_tools,
            delegate_hint_trigger: None,
            delegate_target_tool: None,
            delegate_handoff_id: None,
            handoff_id,
        },
    });
    let text = text_payload_for_response(&resp, None, false);
    let mut body = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
        "_meta": {}
    });
    if let Some(policy) = crate::tool_defs::tool_execution_policy_payload(name) {
        body["_meta"]["codelens/executionPolicy"] = policy;
    }
    crate::tool_defs::apply_tool_deprecation_meta(&mut body["_meta"], name);
    JsonRpcResponse::result(id, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationWorkClass;

    #[test]
    fn pre_dispatch_facade_error_recovers_resolved_operation_without_global_state() {
        let project = crate::tests::project_root();
        let state = crate::tests::make_state(&project);
        let arguments = json!({
            "mode": "symbol",
            "name": "facade_metric_target",
        });
        let session_id = "facade-pre-dispatch-error";

        let response = build_error_response(
            "search",
            CodeLensError::Validation("blocked before dispatch".to_owned()),
            None,
            &arguments,
            "preset:full",
            session_id,
            &state,
            std::time::Instant::now(),
            Some(json!(1)),
            0,
            false,
            None,
        );

        assert!(response.error.is_none());
        let metrics = state.metrics().session_snapshot_for(session_id);
        assert_eq!(metrics.call_type.low_level_calls, 1);
        assert_eq!(metrics.call_type.composite_calls, 0);
        let invocation = &metrics.timeline[0];
        assert_eq!(invocation.tool, "search");
        assert_eq!(invocation.resolved_target.as_deref(), Some("find_symbol"));
        assert_eq!(invocation.mode.as_deref(), Some("symbol"));
        assert_eq!(invocation.work_class, OperationWorkClass::Primitive);
        assert_eq!(invocation.downstream_call_count, 0);
        assert!(!invocation.success);
    }
}
