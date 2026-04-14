use super::response_support::{
    apply_contextual_guidance, bounded_result_payload, budget_hint, compact_response_payload,
    effective_budget_for_tool, max_result_size_chars_for_tool, routing_hint_for_payload,
    success_jsonrpc_response, text_payload_for_response,
};
use crate::AppState;
use crate::client_profile::ClientProfile;
use crate::error::{CodeLensError, ToolAccessFailure};
use crate::mutation_gate::{MutationGateAllowance, MutationGateFailure, is_verifier_source_tool};
use crate::protocol::{
    JsonRpcResponse, RecoveryAction, RecoveryActionKind, ToolCallResponse, ToolResponseMeta,
};
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, preferred_bootstrap_tools, tool_definition,
};
use crate::tools;
use serde_json::json;
use std::collections::HashMap;

fn bootstrap_follow_up_tools(surface: ToolSurface) -> Vec<String> {
    preferred_bootstrap_tools(surface)
        .unwrap_or(&[])
        .iter()
        .copied()
        .filter(|tool| {
            !matches!(
                *tool,
                "activate_project" | "prepare_harness_session" | "set_profile"
            )
        })
        .take(3)
        .map(str::to_owned)
        .collect()
}

fn response_client_profile(state: &AppState, arguments: &serde_json::Value) -> ClientProfile {
    SessionRequestContext::from_json(arguments)
        .client_name
        .as_deref()
        .map(|name| ClientProfile::detect(Some(name)))
        .unwrap_or_else(|| state.client_profile())
}

fn surface_from_label(label: &str, fallback: ToolSurface) -> ToolSurface {
    if let Some(profile) = ToolProfile::from_str(label) {
        return ToolSurface::Profile(profile);
    }
    if let Some(preset_label) = label.strip_prefix("preset:") {
        return ToolSurface::Preset(ToolPreset::from_str(preset_label));
    }
    fallback
}

fn effective_follow_up_surface(payload: &serde_json::Value, fallback: ToolSurface) -> ToolSurface {
    let Some(label) = payload
        .get("active_surface")
        .and_then(|value| value.as_str())
    else {
        return fallback;
    };
    surface_from_label(label, fallback)
}

fn protocol_error_data(
    state: &AppState,
    surface: ToolSurface,
    active_surface: &str,
    arguments: &serde_json::Value,
    tool_name: &str,
) -> serde_json::Value {
    let response_client = response_client_profile(state, arguments);
    let error_surface = surface_from_label(active_surface, surface);
    let routing_hint = crate::protocol::RoutingHint::Sync;
    let orchestration_contract = crate::harness_host::response_orchestration_contract(
        response_client,
        error_surface,
        tool_name,
        routing_hint,
    );
    let recommended_next_steps = crate::harness_host::response_next_steps(
        response_client,
        tool_name,
        routing_hint,
        &[],
        None,
    );
    json!({
        "error_class": "protocol",
        "tool_name": tool_name,
        "routing_hint": routing_hint,
        "orchestration_contract": orchestration_contract,
        "recommended_next_steps": recommended_next_steps,
    })
}

fn invalid_params_error_data(tool_name: &str) -> serde_json::Value {
    json!({
        "error_class": "validation",
        "tool_name": tool_name,
        "request_stage": "tool_arguments",
    })
}

fn unknown_tool_error_data(tool_name: &str) -> serde_json::Value {
    json!({
        "error_class": "validation",
        "tool_name": tool_name,
        "request_stage": "tool_selection",
    })
}

fn append_budget_hint(hint: &mut Option<String>, extra: impl Into<String>) {
    let extra = extra.into();
    match hint {
        Some(existing) => {
            if !existing.is_empty() {
                existing.push(' ');
            }
            existing.push_str(&extra);
        }
        None => *hint = Some(extra),
    }
}

fn should_emit_followup_guidance(
    name: &str,
    routing_hint: crate::protocol::RoutingHint,
    emitted_composite_guidance: bool,
) -> bool {
    if emitted_composite_guidance || !matches!(routing_hint, crate::protocol::RoutingHint::Sync) {
        return true;
    }

    matches!(
        name,
        "activate_project"
            | "prepare_harness_session"
            | "onboard_project"
            | "explore_codebase"
            | "trace_request_path"
            | "review_architecture"
            | "plan_safe_refactor"
            | "audit_security_context"
            | "analyze_change_impact"
            | "cleanup_duplicate_logic"
            | "review_changes"
            | "assess_change_readiness"
            | "diagnose_issues"
            | "analyze_change_request"
            | "verify_change_readiness"
            | "find_minimal_context_for_change"
            | "summarize_symbol_impact"
            | "module_boundary_report"
            | "safe_rename_report"
            | "unresolved_reference_check"
            | "dead_code_report"
            | "impact_report"
            | "refactor_safety_report"
            | "diff_aware_references"
            | "semantic_code_review"
    )
}

fn tool_recovery_action(
    target: impl Into<String>,
    arguments: Option<serde_json::Value>,
    reason: impl Into<String>,
) -> RecoveryAction {
    RecoveryAction {
        kind: RecoveryActionKind::ToolCall,
        target: target.into(),
        arguments,
        reason: reason.into(),
    }
}

fn rpc_recovery_action(
    target: impl Into<String>,
    arguments: Option<serde_json::Value>,
    reason: impl Into<String>,
) -> RecoveryAction {
    RecoveryAction {
        kind: RecoveryActionKind::RpcCall,
        target: target.into(),
        arguments,
        reason: reason.into(),
    }
}

fn access_recovery_actions(failure: &ToolAccessFailure) -> Vec<RecoveryAction> {
    match failure {
        ToolAccessFailure::NotAvailableInActiveSurface { .. } => vec![
            tool_recovery_action(
                "prepare_harness_session",
                None,
                "Re-bootstrap the active surface before retrying the blocked tool.",
            ),
            rpc_recovery_action(
                "tools/list",
                Some(json!({"full": true})),
                "Inspect the full visible surface and choose an allowed alternative.",
            ),
        ],
        ToolAccessFailure::HiddenByDeferredNamespace { namespace, .. } => vec![
            rpc_recovery_action(
                "tools/list",
                Some(json!({"namespace": namespace})),
                "Load the deferred namespace into the current session before retrying.",
            ),
            rpc_recovery_action(
                "tools/list",
                Some(json!({"full": true})),
                "Expand the full tool surface if you need more than one hidden namespace.",
            ),
        ],
        ToolAccessFailure::HiddenByDeferredTier { tier, .. } => vec![
            rpc_recovery_action(
                "tools/list",
                Some(json!({"tier": tier})),
                "Load the deferred tier into the current session before retrying.",
            ),
            rpc_recovery_action(
                "tools/list",
                Some(json!({"full": true})),
                "Expand the full tool surface if you need multiple deferred tiers.",
            ),
        ],
        ToolAccessFailure::TrustedHttpRequired { .. }
        | ToolAccessFailure::DaemonModeBlocked { .. }
        | ToolAccessFailure::ReadOnlySurfaceBlocked { .. } => vec![
            tool_recovery_action(
                "prepare_harness_session",
                None,
                "Re-bootstrap an allowed runtime or surface before retrying the blocked mutation.",
            ),
            rpc_recovery_action(
                "tools/list",
                Some(json!({"full": true})),
                "Inspect which mutation tools are currently exposed for this session.",
            ),
        ],
    }
}

fn follow_up_tool_recovery_actions(
    tools: &[String],
    reasons: Option<&HashMap<String, String>>,
) -> Vec<RecoveryAction> {
    tools
        .iter()
        .take(3)
        .map(|tool| {
            let reason = reasons
                .and_then(|map| map.get(tool))
                .cloned()
                .unwrap_or_else(|| {
                    "Recommended recovery call for the current failure state.".to_owned()
                });
            tool_recovery_action(tool.clone(), None, reason)
        })
        .collect()
}

fn is_access_recovery_error(error: &CodeLensError) -> bool {
    matches!(error, CodeLensError::AccessDenied(_))
}

fn should_emit_error_guidance(
    error: &CodeLensError,
    gate_failure: Option<&MutationGateFailure>,
) -> bool {
    gate_failure.is_some() || is_access_recovery_error(error)
}

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
        append_budget_hint(
            &mut hint,
            "Tip: run verify_change_readiness before mutations for safer edits.",
        );
    }
    if doom_loop_count >= 3 {
        if doom_loop_rapid {
            append_budget_hint(
                &mut hint,
                format!(
                    "Rapid retry burst detected ({doom_loop_count}x in <10s). \
                     Use start_analysis_job for heavy analysis, or narrow scope with path/max_tokens."
                ),
            );
        } else {
            append_budget_hint(
                &mut hint,
                "Repeated low-level chain detected. Prefer verify_change_readiness, \
                 find_minimal_context_for_change, analyze_change_request for compressed context.",
            );
        }
    }
    resp.token_estimate = Some(payload_estimate);
    resp.budget_hint = hint;
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

    let response_client = response_client_profile(state, arguments);
    let follow_up_surface = effective_follow_up_surface(
        resp.data.as_ref().unwrap_or(&serde_json::Value::Null),
        surface,
    );
    if name == "prepare_harness_session" {
        let follow_up_tools = bootstrap_follow_up_tools(follow_up_surface);
        if !follow_up_tools.is_empty() {
            resp.suggested_next_tools = Some(follow_up_tools);
        }
    }

    if let Some(ref next_tools) = resp.suggested_next_tools {
        resp.suggestion_reasons = Some(tools::suggestion_reasons_for(next_tools, name));
    }
    let routing_hint = resp
        .routing_hint
        .expect("routing_hint should be set before orchestration contract");
    let emit_followup_guidance =
        should_emit_followup_guidance(name, routing_hint, emitted_composite_guidance);
    if emit_followup_guidance {
        resp.orchestration_contract = Some(crate::harness_host::response_orchestration_contract(
            response_client,
            follow_up_surface,
            name,
            routing_hint,
        ));
        resp.recommended_next_steps = Some(crate::harness_host::response_next_steps(
            response_client,
            name,
            routing_hint,
            resp.suggested_next_tools.as_deref().unwrap_or(&[]),
            resp.suggestion_reasons.as_ref(),
        ));
    } else {
        resp.suggested_next_tools = None;
        resp.suggestion_reasons = None;
        resp.orchestration_contract = None;
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

    state.metrics().record_call_with_tokens_for_session(
        name,
        elapsed_ms as u64,
        true,
        payload_estimate,
        active_surface,
        truncated,
        harness_phase,
        Some(logical_session_id),
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
    surface: ToolSurface,
    active_surface: &str,
    arguments: &serde_json::Value,
    logical_session_id: &str,
    state: &AppState,
    start: std::time::Instant,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let elapsed_ms = start.elapsed().as_millis();

    state.metrics().record_call_with_tokens_for_session(
        name,
        elapsed_ms as u64,
        false,
        0,
        active_surface,
        false,
        None,
        Some(logical_session_id),
    );

    if matches!(error, CodeLensError::MissingParam(_)) {
        return JsonRpcResponse::error_with_data(
            id,
            error.jsonrpc_code(),
            error.to_string(),
            invalid_params_error_data(name),
        );
    }

    if matches!(error, CodeLensError::ToolNotFound(_)) {
        return JsonRpcResponse::error_with_data(
            id,
            error.jsonrpc_code(),
            error.to_string(),
            unknown_tool_error_data(name),
        );
    }

    if error.is_protocol_error() {
        return JsonRpcResponse::error_with_data(
            id,
            error.jsonrpc_code(),
            error.to_string(),
            protocol_error_data(state, surface, active_surface, arguments, name),
        );
    }

    let mut resp = ToolCallResponse::error(error.to_string());
    let emit_error_guidance = should_emit_error_guidance(&error, gate_failure.as_ref());
    if let Some(failure) = gate_failure.as_ref() {
        let analysis_hint = failure
            .analysis_id
            .as_ref()
            .map(|analysis_id| format!(" Last related analysis_id: `{analysis_id}`."))
            .unwrap_or_default();
        resp.error = Some(format!(
            "[{:?}] {}{}",
            failure.kind, failure.message, analysis_hint
        ));
        resp.suggested_next_tools = Some(failure.suggested_next_tools.clone());
        resp.budget_hint = Some(failure.budget_hint.clone());
    }
    let routing_hint = crate::protocol::RoutingHint::Sync;
    resp.routing_hint = Some(routing_hint);
    if let Some(ref next_tools) = resp.suggested_next_tools {
        resp.suggestion_reasons = Some(tools::suggestion_reasons_for(next_tools, name));
    }
    if let Some(access_failure) = match &error {
        CodeLensError::AccessDenied(failure) => Some(failure),
        _ => None,
    } {
        let recovery_actions = access_recovery_actions(access_failure);
        if !recovery_actions.is_empty() {
            resp.recovery_actions = Some(recovery_actions);
        }
    } else if gate_failure.is_some() {
        let recovery_actions = follow_up_tool_recovery_actions(
            resp.suggested_next_tools.as_deref().unwrap_or(&[]),
            resp.suggestion_reasons.as_ref(),
        );
        if !recovery_actions.is_empty() {
            resp.recovery_actions = Some(recovery_actions);
        }
    }
    if emit_error_guidance {
        let response_client = response_client_profile(state, arguments);
        let error_surface = surface_from_label(active_surface, surface);
        resp.orchestration_contract = Some(crate::harness_host::response_orchestration_contract(
            response_client,
            error_surface,
            name,
            routing_hint,
        ));
        resp.recommended_next_steps = Some(crate::harness_host::response_next_steps(
            response_client,
            name,
            routing_hint,
            resp.suggested_next_tools.as_deref().unwrap_or(&[]),
            resp.suggestion_reasons.as_ref(),
        ));
    }
    let text = text_payload_for_response(&resp, None);
    JsonRpcResponse::result(
        id,
        json!({
            "content": [{ "type": "text", "text": text }],
            "isError": true
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use codelens_engine::ProjectRoot;

    #[test]
    fn generic_error_response_omits_orchestration_contract() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-error-response-contract-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        let state = AppState::new(project, ToolPreset::Full);

        let response = build_error_response(
            "read_memory",
            CodeLensError::Validation("boom".to_owned()),
            None,
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            "reviewer-graph",
            &json!({
                "_session_client_name": "Claude Code",
            }),
            "local",
            &state,
            std::time::Instant::now(),
            Some(json!(1)),
        );

        let value = serde_json::to_value(&response).unwrap();
        let text = value["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("{}");
        let payload: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        assert_eq!(payload["routing_hint"], json!("sync"));
        assert!(payload.get("orchestration_contract").is_none());
        assert!(payload.get("recommended_next_steps").is_none());
        assert!(payload.get("recovery_actions").is_none());
    }

    #[test]
    fn missing_param_error_stays_lean_without_orchestration_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-missing-param-error-contract-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        let state = AppState::new(project, ToolPreset::Full);

        let response = build_error_response(
            "set_profile",
            CodeLensError::MissingParam("profile".to_owned()),
            None,
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            "reviewer-graph",
            &json!({
                "_session_client_name": "CodexHarness",
            }),
            "local",
            &state,
            std::time::Instant::now(),
            Some(json!(3)),
        );

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["error"]["code"], json!(-32602));
        assert_eq!(value["error"]["data"]["error_class"], json!("validation"));
        assert_eq!(value["error"]["data"]["tool_name"], json!("set_profile"));
        assert_eq!(
            value["error"]["data"]["request_stage"],
            json!("tool_arguments")
        );
        assert!(value["error"]["data"].get("orchestration_contract").is_none());
        assert!(value["error"]["data"].get("recommended_next_steps").is_none());
        assert!(value["error"]["data"].get("recovery_actions").is_none());
    }

    #[test]
    fn access_recovery_error_uses_request_client_profile_and_surface_contract() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-access-error-response-contract-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        let state = AppState::new(project, ToolPreset::Full);

        let response = build_error_response(
            "read_memory",
            CodeLensError::AccessDenied(ToolAccessFailure::NotAvailableInActiveSurface {
                tool_name: "read_memory".to_owned(),
                active_surface: "reviewer-graph".to_owned(),
            }),
            None,
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            "reviewer-graph",
            &json!({
                "_session_client_name": "Claude Code",
            }),
            "local",
            &state,
            std::time::Instant::now(),
            Some(json!(11)),
        );

        let value = serde_json::to_value(&response).unwrap();
        let text = value["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("{}");
        let payload: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        assert_eq!(
            payload["orchestration_contract"]["host_id"],
            json!("claude-code")
        );
        assert_eq!(
            payload["orchestration_contract"]["active_surface"],
            json!("reviewer-graph")
        );
        assert!(
            payload["recovery_actions"]
                .as_array()
                .map(|items| items.iter().any(|item| {
                    item["kind"] == json!("rpc_call")
                        && item["target"] == json!("tools/list")
                        && item["arguments"]["full"] == json!(true)
                }))
                .unwrap_or(false)
        );
        assert!(
            payload["recommended_next_steps"]
                .as_array()
                .map(|items| items
                    .iter()
                    .any(|item| item["target"] == json!("host_orchestrator")))
                .unwrap_or(false)
        );
    }

    #[test]
    fn unknown_tool_error_stays_lean_without_orchestration_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-unknown-tool-error-contract-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        let state = AppState::new(project, ToolPreset::Full);

        let response = build_error_response(
            "missing_tool",
            CodeLensError::ToolNotFound("missing_tool".to_owned()),
            None,
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            "reviewer-graph",
            &json!({
                "_session_client_name": "Claude Code",
            }),
            "local",
            &state,
            std::time::Instant::now(),
            Some(json!(2)),
        );

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["error"]["code"], json!(-32601));
        assert_eq!(value["error"]["data"]["error_class"], json!("validation"));
        assert_eq!(
            value["error"]["data"]["tool_name"],
            json!("missing_tool")
        );
        assert_eq!(
            value["error"]["data"]["request_stage"],
            json!("tool_selection")
        );
        assert!(value["error"]["data"].get("orchestration_contract").is_none());
        assert!(value["error"]["data"].get("recommended_next_steps").is_none());
        assert!(value["error"]["data"].get("recovery_actions").is_none());
    }
}
