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
    let pct = if budget > 0 {
        tokens * 100 / budget
    } else {
        100
    };
    let base = format!("{tokens} tokens ({pct}% of {budget} budget)");

    if pct > 95 {
        format!(
            "{base}. Response near limit — use get_analysis_section to expand specific parts instead of full reports."
        )
    } else if pct > 75 {
        format!("{base}. Consider narrowing scope with path or max_tokens parameter.")
    } else {
        base
    }
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

/// Recursively strip empty arrays, null values, and empty strings from a JSON Value.
pub(crate) fn strip_empty_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, v| {
                strip_empty_fields(v);
                !is_empty_value(v)
            });
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_empty_fields(item);
            }
        }
        _ => {}
    }
}

fn is_empty_value(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

pub(crate) fn compact_response_payload(resp: &mut ToolCallResponse) {
    if let Some(ref mut data) = resp.data {
        strip_empty_fields(data);
    }
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
    // Async handles get a slim payload that cherry-picks essential fields.
    if let Some(slim) = slim_text_payload_for_async_handle(resp, structured_content) {
        return serde_json::to_string(&slim).unwrap_or_else(|_| {
            "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
        });
    }
    // Pretty-print the full response as structured JSON with readable formatting.
    // Agents get valid JSON (parseability preserved) but with indentation and
    // newlines instead of a single flat line.
    format_structured_response(resp)
}

/// Format a ToolCallResponse as pretty-printed JSON.
/// Preserves JSON validity for parsing while being much more readable than
/// a single-line blob. Strips redundant metadata fields that waste tokens.
fn format_structured_response(resp: &ToolCallResponse) -> String {
    // Build a clean output object with only the fields agents need.
    let mut out = serde_json::Map::new();

    out.insert("success".to_owned(), Value::Bool(resp.success));

    // Header: compact metadata on key fields only
    if let Some(ref backend) = resp.backend_used {
        out.insert("backend_used".to_owned(), Value::String(backend.clone()));
    }
    if let Some(c) = resp.confidence {
        out.insert("confidence".to_owned(), Value::from(c));
    }
    if let Some(ms) = resp.elapsed_ms {
        out.insert("elapsed_ms".to_owned(), Value::from(ms));
    }
    if let Some(ref hint) = resp.budget_hint {
        out.insert("budget_hint".to_owned(), Value::String(hint.clone()));
    }

    // Data: the core payload
    if let Some(ref data) = resp.data {
        out.insert("data".to_owned(), data.clone());
    }

    // Suggested next tools (preserve original key for compatibility)
    if let Some(ref tools) = resp.suggested_next_tools {
        out.insert(
            "suggested_next_tools".to_owned(),
            serde_json::to_value(tools).unwrap_or(Value::Array(vec![])),
        );
        // Reasons as separate map (agents can read both)
        if let Some(ref reasons) = resp.suggestion_reasons {
            if let Ok(v) = serde_json::to_value(reasons) {
                out.insert("suggestion_reasons".to_owned(), v);
            }
        }
    }

    serde_json::to_string_pretty(&Value::Object(out))
        .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_owned())
}

fn slim_text_payload_for_async_handle(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> Option<Value> {
    let data = structured_content?.as_object()?;
    data.get("analysis_id")?;

    let mut payload = Map::new();
    payload.insert("success".to_owned(), Value::Bool(resp.success));
    insert_if_present(
        &mut payload,
        "backend_used",
        resp.backend_used.clone().map(Value::String),
    );
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
    insert_if_present(
        &mut payload,
        "staleness_ms",
        resp.staleness_ms.map(Value::from),
    );
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
    insert_if_present(
        &mut payload,
        "suggestion_reasons",
        resp.suggestion_reasons
            .as_ref()
            .and_then(|reasons| serde_json::to_value(reasons).ok()),
    );

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

fn copy_summarized_field(target: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), summarize_structured_content(value, 0));
    }
}

/// Adaptive compression based on OpenDev 5-stage strategy (arxiv:2603.05344).
/// Thresholds are adjusted by effort level offset (Low=-10, Medium=0, High=+10).
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
    effort_offset: i32,
) -> (String, Option<Value>, bool) {
    let usage_pct = if effective_budget > 0 {
        payload_estimate * 100 / effective_budget
    } else {
        100
    };
    // Apply effort offset to thresholds (High effort delays compression)
    let t1 = (75i32 + effort_offset).clamp(50, 90) as usize;
    let t2 = (85i32 + effort_offset).clamp(60, 95) as usize;
    let t3 = (95i32 + effort_offset).clamp(70, 100) as usize;
    let t4 = (100i32 + effort_offset).clamp(80, 110) as usize;

    let max_chars = effective_budget * 8;
    let mut truncated = false;

    if usage_pct <= t1 {
        // Stage 1: pass through
    } else if usage_pct <= t2 {
        // Stage 2: light summarization
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 1));
        }
    } else if usage_pct <= t3 {
        // Stage 3: aggressive summarization
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
    } else if usage_pct <= t4 {
        // Stage 4: aggressive summarize structured + truncate text if needed
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        if text.len() > max_chars {
            // In-place truncation: find the char boundary at max_chars,
            // truncate the existing allocation, and append the marker —
            // avoids the two intermediate String allocations that
            // `chars().take().collect::<String>()` + `format!()` paid.
            let byte_idx = text
                .char_indices()
                .nth(max_chars)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            text.truncate(byte_idx);
            text.push_str("...[truncated]");
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

/// Determine `_meta["anthropic/maxResultSizeChars"]` based on tool tier.
/// Claude Code v2.1.91+ respects this annotation to keep up to 500K chars.
pub(crate) fn max_result_size_chars_for_tool(name: &str, truncated: bool) -> usize {
    use crate::protocol::ToolTier;
    use crate::tool_defs::tool_tier;

    if truncated {
        return 25_000;
    }

    match tool_tier(name) {
        ToolTier::Workflow => 200_000,
        ToolTier::Analysis => 100_000,
        ToolTier::Primitive => 50_000,
    }
}

pub(crate) fn success_jsonrpc_response(
    id: Option<Value>,
    text: String,
    structured_content: Option<Value>,
    max_result_size_chars: Option<usize>,
) -> JsonRpcResponse {
    let mut result = json!({
        "content": [{ "type": "text", "text": text }]
    });
    if let Some(structured_content) = structured_content {
        result["structuredContent"] = structured_content;
    }
    if let Some(max_chars) = max_result_size_chars {
        result["_meta"] = json!({
            "anthropic/maxResultSizeChars": max_chars
        });
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
