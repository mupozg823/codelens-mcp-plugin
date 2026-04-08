use crate::protocol::{JsonRpcResponse, RoutingHint, ToolCallResponse};
use crate::tool_defs::{ToolSurface, tool_definition};
use crate::tools;
use serde_json::{Map, Value, json};

pub(crate) fn effective_budget_for_tool(name: &str, request_budget: usize) -> usize {
    tool_definition(name)
        .and_then(|t| t.max_response_tokens)
        .map(|cap| request_budget.min(cap))
        .unwrap_or(request_budget)
}

pub(crate) fn budget_hint(tool_name: &str, tokens: usize, budget: usize) -> String {
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

pub(crate) fn apply_contextual_guidance(
    resp: &mut ToolCallResponse,
    name: &str,
    recent_tools: &[String],
    harness_phase: Option<&str>,
    surface: ToolSurface,
) -> bool {
    resp.suggested_next_tools = tools::suggest_next_contextual(name, recent_tools, harness_phase);

    let mut emitted_composite_guidance = false;
    if let Some((guided_tools, guidance_hint)) =
        tools::composite_guidance_for_chain(name, recent_tools, surface)
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
    emitted_composite_guidance
}

pub(crate) fn routing_hint_for_payload(resp: &ToolCallResponse) -> RoutingHint {
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
    if is_cached {
        RoutingHint::Cached
    } else if is_analysis_handle {
        RoutingHint::Async
    } else {
        RoutingHint::Sync
    }
}

pub(crate) fn compact_response_payload(resp: &mut ToolCallResponse) {
    if let Some(ref mut data) = resp.data
        && let Some(obj) = data.as_object_mut()
    {
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
        if let Some(checks) = obj.get_mut("verifier_checks")
            && let Some(arr) = checks.as_array_mut()
        {
            for check in arr.iter_mut() {
                if let Some(check_obj) = check.as_object_mut() {
                    check_obj.remove("summary");
                    check_obj.remove("evidence_section");
                }
            }
        }
    }
}

pub(crate) fn text_payload_for_response(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> String {
    let payload = slim_text_payload_for_async_handle(resp, structured_content)
        .unwrap_or_else(|| serde_json::to_value(resp).unwrap_or_else(|_| json!({})));
    serde_json::to_string(&payload)
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned())
}

fn slim_text_payload_for_async_handle(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> Option<Value> {
    let data = structured_content?.as_object()?;
    data.get("analysis_id")?;

    let mut payload = Map::new();
    payload.insert("success".to_owned(), Value::Bool(resp.success));
    insert_if_present(&mut payload, "backend_used", resp.backend_used.clone().map(Value::String));
    insert_if_present(&mut payload, "confidence", resp.confidence.map(Value::from));
    insert_if_present(
        &mut payload,
        "degraded_reason",
        resp.degraded_reason.clone().map(Value::String),
    );
    insert_if_present(
        &mut payload,
        "source",
        resp.source
            .as_ref()
            .and_then(|source| serde_json::to_value(source).ok()),
    );
    insert_if_present(&mut payload, "partial", resp.partial.map(Value::Bool));
    insert_if_present(
        &mut payload,
        "freshness",
        resp.freshness
            .as_ref()
            .and_then(|freshness| serde_json::to_value(freshness).ok()),
    );
    insert_if_present(&mut payload, "staleness_ms", resp.staleness_ms.map(Value::from));
    insert_if_present(
        &mut payload,
        "token_estimate",
        resp.token_estimate.map(Value::from),
    );
    insert_if_present(
        &mut payload,
        "suggested_next_tools",
        resp.suggested_next_tools
            .as_ref()
            .and_then(|tools| serde_json::to_value(tools).ok()),
    );
    insert_if_present(
        &mut payload,
        "budget_hint",
        resp.budget_hint.clone().map(Value::String),
    );
    insert_if_present(
        &mut payload,
        "routing_hint",
        resp.routing_hint
            .as_ref()
            .and_then(|hint| serde_json::to_value(hint).ok()),
    );
    insert_if_present(&mut payload, "elapsed_ms", resp.elapsed_ms.map(Value::from));

    let mut text_data = Map::new();
    copy_summarized_field(&mut text_data, data, "analysis_id");
    copy_summarized_field(&mut text_data, data, "summary");
    copy_summarized_field(&mut text_data, data, "readiness");
    copy_summarized_field(&mut text_data, data, "readiness_score");
    copy_summarized_field(&mut text_data, data, "risk_level");
    copy_summarized_field(&mut text_data, data, "blocker_count");
    copy_summarized_field(&mut text_data, data, "reused");
    copy_summarized_field(&mut text_data, data, "available_sections");
    copy_summarized_field(&mut text_data, data, "next_actions");
    if !text_data.is_empty() {
        payload.insert("data".to_owned(), Value::Object(text_data));
    }

    Some(Value::Object(payload))
}

fn insert_if_present(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), value);
    }
}

fn copy_summarized_field(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), summarize_structured_content(value, 0));
    }
}

/// Adaptive compression based on OpenDev 5-stage strategy (arxiv:2603.05344).
/// Stage 1: <75% budget → pass through
/// Stage 2: 75-85% → summarize structured content (depth=1)
/// Stage 3: 85-95% → aggressive summarize (depth=0)
/// Stage 4: 95-100% → drop structured content entirely
/// Stage 5: >100% → hard truncation to error payload
pub(crate) fn bounded_result_payload(
    mut text: String,
    mut structured_content: Option<Value>,
    payload_estimate: usize,
    effective_budget: usize,
) -> (String, Option<Value>, bool) {
    let usage_pct = if effective_budget > 0 {
        payload_estimate * 100 / effective_budget
    } else {
        100
    };
    let max_chars = effective_budget * 8;
    let mut truncated = false;

    if usage_pct <= 75 {
        // Stage 1: pass through
    } else if usage_pct <= 85 {
        // Stage 2: light summarization
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 1));
        }
    } else if usage_pct <= 95 {
        // Stage 3: aggressive summarization
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
    } else if usage_pct <= 100 {
        // Stage 4: aggressive summarize structured + truncate text if needed
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        if text.len() > max_chars {
            text = format!("{}...[truncated]", text.chars().take(max_chars).collect::<String>());
        }
        truncated = true;
    } else {
        // Stage 5: hard truncation — summarize structured to minimal skeleton
        truncated = true;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        text = serde_json::to_string(&json!({
            "success": true,
            "truncated": true,
            "compression_stage": 5,
            "error": format!(
                "Response too large ({} tokens, budget {}). Narrow with path, max_tokens, or depth.",
                payload_estimate, effective_budget
            ),
            "token_estimate": payload_estimate,
        }))
        .unwrap_or_else(|_| "{\"success\":false,\"truncated\":true}".to_owned());
    }
    (text, structured_content, truncated)
}

pub(crate) fn success_jsonrpc_response(
    id: Option<Value>,
    text: String,
    structured_content: Option<Value>,
) -> JsonRpcResponse {
    let mut result = json!({
        "content": [{ "type": "text", "text": text }]
    });
    if let Some(structured_content) = structured_content {
        result["structuredContent"] = structured_content;
    }
    JsonRpcResponse::result(id, result)
}

fn summarize_structured_content(value: &Value, depth: usize) -> Value {
    const MAX_STRING_CHARS: usize = 240;
    const MAX_ARRAY_ITEMS: usize = 3;
    const MAX_OBJECT_DEPTH: usize = 4;

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => {
            if text.chars().count() <= MAX_STRING_CHARS {
                value.clone()
            } else {
                let truncated = text.chars().take(MAX_STRING_CHARS).collect::<String>();
                Value::String(format!("{truncated}..."))
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|item| summarize_structured_content(item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => {
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
                summarized.insert("truncated".to_owned(), Value::Bool(true));
            }
            Value::Object(summarized)
        }
    }
}
