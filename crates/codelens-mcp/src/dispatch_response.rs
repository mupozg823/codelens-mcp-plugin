use crate::error::CodeLensError;
use crate::mutation_gate::{is_verifier_source_tool, MutationGateAllowance, MutationGateFailure};
use crate::protocol::{JsonRpcResponse, ToolCallResponse, ToolResponseMeta};
use crate::tool_defs::{tool_definition, ToolSurface};
use crate::tools;
use crate::AppState;
use serde_json::json;

fn summarize_structured_content(value: &serde_json::Value, depth: usize) -> serde_json::Value {
    const MAX_STRING_CHARS: usize = 240;
    const MAX_ARRAY_ITEMS: usize = 3;
    const MAX_OBJECT_DEPTH: usize = 4;

    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            value.clone()
        }
        serde_json::Value::String(text) => {
            if text.chars().count() <= MAX_STRING_CHARS {
                value.clone()
            } else {
                let truncated = text.chars().take(MAX_STRING_CHARS).collect::<String>();
                serde_json::Value::String(format!("{truncated}..."))
            }
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|item| summarize_structured_content(item, depth + 1))
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let max_items = if depth >= MAX_OBJECT_DEPTH {
                MAX_ARRAY_ITEMS
            } else {
                usize::MAX
            };
            let mut summarized = serde_json::Map::with_capacity(map.len().min(max_items));
            for (index, (key, item)) in map.iter().enumerate() {
                if index >= max_items {
                    break;
                }
                summarized.insert(key.clone(), summarize_structured_content(item, depth + 1));
            }
            if map.contains_key("truncated") {
                summarized.insert("truncated".to_owned(), serde_json::Value::Bool(true));
            }
            serde_json::Value::Object(summarized)
        }
    }
}

fn budget_hint(tool_name: &str, tokens: usize, budget: usize) -> String {
    if matches!(
        tool_name,
        "get_project_structure" | "get_symbols_overview" | "get_current_config" | "onboard_project"
    ) {
        return "overview complete — drill into specific files or symbols".to_owned();
    }
    if tokens > budget {
        return format!(
            "response ({tokens} tokens) exceeds budget ({budget}) — narrow with path filter or max_tokens"
        );
    }
    if tokens > budget * 3 / 4 {
        return format!("near budget ({tokens}/{budget} tokens) — consider narrowing scope");
    }
    if tokens > 100 {
        return "context sufficient — proceed to edit or analysis".to_owned();
    }
    if tokens < 50 {
        return "minimal results — try broader query or different tool".to_owned();
    }
    "focused result — ready for next step".to_owned()
}

fn compact_response_payload(resp: &mut ToolCallResponse) {
    if let Some(ref mut data) = resp.data {
        if let Some(obj) = data.as_object_mut() {
            obj.remove("quality_focus");
            obj.remove("recommended_checks");
            obj.remove("performance_watchpoints");
            obj.remove("available_sections");
            obj.remove("evidence_handles");
            obj.remove("schema_version");
            obj.remove("report_kind");
            obj.remove("profile");
            obj.remove("next_actions");
            obj.remove("machine_summary");
            if let Some(checks) = obj.get_mut("verifier_checks") {
                if let Some(arr) = checks.as_array_mut() {
                    for check in arr.iter_mut() {
                        if let Some(check_obj) = check.as_object_mut() {
                            check_obj.remove("summary");
                            check_obj.remove("evidence_section");
                        }
                    }
                }
            }
        }
    }
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

    if is_verifier_source_tool(name) {
        state.record_recent_preflight_from_payload(
            name,
            active_surface,
            logical_session_id,
            arguments,
            &payload,
        );
    }

    if gate_allowance.map(|a| a.caution) == Some(true) {
        state.metrics().record_mutation_with_caution();
    }

    let has_output_schema = tool_definition(name)
        .and_then(|tool| tool.output_schema.as_ref())
        .is_some();
    let mut structured_content = has_output_schema.then(|| payload.clone());

    let mut resp = ToolCallResponse::success(payload, meta);
    resp.suggested_next_tools =
        tools::suggest_next_contextual(name, &state.recent_tools(), harness_phase);

    let payload_estimate = serde_json::to_string(&resp.data)
        .map(|s| tools::estimate_tokens(&s))
        .unwrap_or(0);
    resp.token_estimate = Some(payload_estimate);
    resp.budget_hint = Some(budget_hint(name, payload_estimate, request_budget));
    resp.elapsed_ms = Some(elapsed_ms as u64);

    // Set routing_hint based on response characteristics
    let is_cached = resp
        .data
        .as_ref()
        .and_then(|d| d.get("reused"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_analysis_handle = resp
        .data
        .as_ref()
        .and_then(|d| d.get("analysis_id"))
        .is_some();
    resp.routing_hint = Some(if is_cached {
        "cached".to_owned()
    } else if is_analysis_handle {
        "async".to_owned()
    } else {
        "sync".to_owned()
    });

    let mut emitted_composite_guidance = false;
    if let Some((guided_tools, guidance_hint)) =
        tools::composite_guidance_for_chain(name, &state.recent_tools(), surface)
    {
        emitted_composite_guidance = true;
        let mut suggestions = guided_tools;
        if let Some(existing) = resp.suggested_next_tools.take() {
            for tool in existing {
                if suggestions.len() >= 3 {
                    break;
                }
                if !suggestions.iter().any(|candidate| candidate == &tool) {
                    suggestions.push(tool);
                }
            }
        }
        resp.suggested_next_tools = Some(suggestions);
        resp.budget_hint = Some(match resp.budget_hint.take() {
            Some(existing) => format!("{existing} {guidance_hint}"),
            None => guidance_hint,
        });
    }

    if compact {
        compact_response_payload(&mut resp);
    }

    let mut text = serde_json::to_string(&resp)
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned());

    let max_chars = request_budget * 8;
    let mut truncated = false;
    if text.len() > max_chars {
        truncated = true;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        text = serde_json::to_string(&json!({
            "success": true,
            "truncated": true,
            "error": format!(
                "Response too large ({} tokens, budget {}). Narrow with path, max_tokens, or depth.",
                payload_estimate, request_budget
            ),
            "token_estimate": payload_estimate,
        }))
        .unwrap_or_else(|_| "{\"success\":false,\"truncated\":true}".to_owned());
    }

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

    let mut result = json!({
        "content": [{ "type": "text", "text": text }]
    });
    if let Some(structured_content) = structured_content {
        result["structuredContent"] = structured_content;
    }
    JsonRpcResponse::result(id, result)
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
