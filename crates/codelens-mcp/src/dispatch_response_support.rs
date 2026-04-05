use crate::protocol::{JsonRpcResponse, RoutingHint, ToolCallResponse};
use crate::tool_defs::{ToolSurface, tool_definition};
use crate::tools;
use serde_json::{Value, json};

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

pub(crate) fn bounded_result_payload(
    mut text: String,
    mut structured_content: Option<Value>,
    payload_estimate: usize,
    effective_budget: usize,
) -> (String, Option<Value>, bool) {
    let max_chars = effective_budget * 8;
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
