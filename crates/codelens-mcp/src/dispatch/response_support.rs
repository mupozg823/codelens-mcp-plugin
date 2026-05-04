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
    let pct = tokens
        .checked_mul(100)
        .and_then(|v| v.checked_div(budget))
        .unwrap_or(100);
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
    let is_async_job = resp
        .data
        .as_ref()
        .and_then(|d| d.get("job_id"))
        .and_then(|v| v.as_str())
        .is_some();
    let is_analysis_handle = resp
        .data
        .as_ref()
        .and_then(|d| d.get("analysis_id"))
        .and_then(|v| v.as_str())
        .is_some();
    if is_cached {
        RoutingHint::Cached
    } else if is_async_job || is_analysis_handle {
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
    if let Some(slim) = slim_text_payload_for_async_job(resp, structured_content) {
        return serde_json::to_string(&slim).unwrap_or_else(|_| {
            "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
        });
    }
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
    out.insert("schema_version".to_owned(), Value::String("1.0".to_owned()));

    // Error message (if present)
    if let Some(ref err) = resp.error {
        out.insert("error".to_owned(), Value::String(err.clone()));
    }

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
    if let Some(ref hint) = resp.routing_hint
        && let Ok(value) = serde_json::to_value(hint)
    {
        out.insert("routing_hint".to_owned(), value);
    }
    if let Some(token_estimate) = resp.token_estimate {
        out.insert("token_estimate".to_owned(), Value::from(token_estimate));
    }
    if let Some(partial) = resp.partial {
        out.insert("partial".to_owned(), Value::Bool(partial));
    }
    if let Some(ref degraded_reason) = resp.degraded_reason {
        out.insert(
            "degraded_reason".to_owned(),
            Value::String(degraded_reason.clone()),
        );
    }
    if let Some(truncated) = resp
        .data
        .as_ref()
        .and_then(|data| data.get("truncated"))
        .and_then(|value| value.as_bool())
    {
        out.insert("truncated".to_owned(), Value::Bool(truncated));
    }

    // Data: summarized mirror of the structured payload for text-only clients.
    // The full machine-readable body remains in `structuredContent`.
    // Opt-in: CODELENS_VERBOSE_TEXT=1 copies the full data (no summarization),
    // so human UIs that only render `content[0].text` see the same payload
    // the agent sees in `structuredContent`.
    if let Some(ref data) = resp.data {
        let data_value =
            if crate::env_compat::env_var_bool("CODELENS_VERBOSE_TEXT").unwrap_or(false) {
                data.clone()
            } else {
                summarize_text_data_for_response(data)
            };
        out.insert("data".to_owned(), data_value);
    }

    // Suggested next tools (preserve original key for compatibility)
    if let Some(ref tools) = resp.suggested_next_tools {
        out.insert(
            "suggested_next_tools".to_owned(),
            serde_json::to_value(tools).unwrap_or(Value::Array(vec![])),
        );
        // Reasons as separate map (agents can read both)
        if let Some(ref reasons) = resp.suggestion_reasons
            && let Ok(v) = serde_json::to_value(reasons)
        {
            out.insert("suggestion_reasons".to_owned(), v);
        }
        // Additive concrete-args companion — skipped when empty.
        if let Some(ref calls) = resp.suggested_next_calls
            && !calls.is_empty()
            && let Ok(v) = serde_json::to_value(calls)
        {
            out.insert("suggested_next_calls".to_owned(), v);
        }
    }

    // CI/batch mode: check if routing hint is Async (analysis handles)
    // or if the response is very small — use compact JSON for efficiency.
    let use_compact = out
        .get("data")
        .and_then(|d| d.as_object())
        .is_some_and(|obj| obj.contains_key("job_id") || obj.contains_key("analysis_id"));

    if use_compact {
        serde_json::to_string(&Value::Object(out))
            .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_owned())
    } else {
        serde_json::to_string_pretty(&Value::Object(out))
            .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_owned())
    }
}

fn slim_text_payload_for_async_job(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> Option<Value> {
    let data = structured_content?.as_object()?;
    data.get("job_id")?.as_str()?;

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
    insert_if_present(
        &mut payload,
        "freshness",
        resp.freshness
            .as_ref()
            .and_then(|freshness| serde_json::to_value(freshness).ok()),
    );
    insert_if_present(&mut payload, "partial", resp.partial.map(Value::Bool));
    insert_if_present(
        &mut payload,
        "token_estimate",
        resp.token_estimate.map(Value::from),
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
        "suggested_next_tools",
        resp.suggested_next_tools
            .as_ref()
            .and_then(|tools| serde_json::to_value(tools).ok()),
    );
    insert_if_present(
        &mut payload,
        "suggestion_reasons",
        resp.suggestion_reasons
            .as_ref()
            .and_then(|reasons| serde_json::to_value(reasons).ok()),
    );
    insert_if_present(
        &mut payload,
        "suggested_next_calls",
        resp.suggested_next_calls
            .as_ref()
            .filter(|calls| !calls.is_empty())
            .and_then(|calls| serde_json::to_value(calls).ok()),
    );

    let mut text_data = Map::new();
    copy_summarized_field(&mut text_data, data, "job_id");
    copy_summarized_field(&mut text_data, data, "kind");
    copy_summarized_field(&mut text_data, data, "status");
    copy_summarized_field(&mut text_data, data, "progress");
    copy_summarized_field(&mut text_data, data, "current_step");
    copy_summarized_field(&mut text_data, data, "profile_hint");
    copy_summarized_field(&mut text_data, data, "analysis_id");
    copy_summarized_field(&mut text_data, data, "estimated_sections");
    copy_summarized_field(&mut text_data, data, "summary_resource");
    copy_summarized_field(&mut text_data, data, "section_handles");
    copy_summarized_field(&mut text_data, data, "error");
    if !text_data.is_empty() {
        payload.insert("data".to_owned(), Value::Object(text_data));
    }

    Some(Value::Object(payload))
}

fn slim_text_payload_for_async_handle(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> Option<Value> {
    let data = structured_content?.as_object()?;
    data.get("analysis_id")?.as_str()?;

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
    insert_if_present(
        &mut payload,
        "suggested_next_calls",
        resp.suggested_next_calls
            .as_ref()
            .filter(|calls| !calls.is_empty())
            .and_then(|calls| serde_json::to_value(calls).ok()),
    );

    let mut text_data = Map::new();
    copy_summarized_field(&mut text_data, data, "analysis_id");
    copy_summarized_field(&mut text_data, data, "summary");
    copy_summarized_field(&mut text_data, data, "readiness");
    copy_summarized_field(&mut text_data, data, "readiness_score");
    copy_summarized_field(&mut text_data, data, "overlapping_claims");
    copy_summarized_field(&mut text_data, data, "risk_level");
    copy_summarized_field(&mut text_data, data, "blocker_count");
    copy_summarized_field(&mut text_data, data, "reused");
    copy_summarized_field(&mut text_data, data, "summary_resource");
    copy_summarized_field(&mut text_data, data, "section_handles");
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

fn summarize_text_data_for_response(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return summarize_structured_content(value, 0);
    };

    if object.contains_key("capabilities")
        && object.contains_key("visible_tools")
        && object.contains_key("routing")
    {
        return summarize_bootstrap_text_data(object);
    }

    summarize_text_object(object, 0)
}

fn summarize_bootstrap_text_data(source: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    copy_summarized_field(&mut out, source, "active_surface");
    copy_summarized_field(&mut out, source, "token_budget");
    copy_summarized_field(&mut out, source, "index_recovery");
    copy_summarized_field(&mut out, source, "health_summary");
    copy_summarized_field(&mut out, source, "warnings");

    if let Some(project) = source.get("project").and_then(|value| value.as_object()) {
        let mut project_summary = Map::new();
        copy_summarized_field(&mut project_summary, project, "project_name");
        copy_summarized_field(&mut project_summary, project, "backend_id");
        copy_summarized_field(&mut project_summary, project, "indexed_files");
        if !project_summary.is_empty() {
            out.insert("project".to_owned(), Value::Object(project_summary));
        }
    }

    if let Some(capabilities) = source
        .get("capabilities")
        .and_then(|value| value.as_object())
    {
        let mut capability_summary = Map::new();
        copy_summarized_field(&mut capability_summary, capabilities, "index_fresh");
        copy_summarized_field(&mut capability_summary, capabilities, "indexed_files");
        copy_summarized_field(&mut capability_summary, capabilities, "supported_files");
        copy_summarized_field(&mut capability_summary, capabilities, "stale_files");
        copy_summarized_field(
            &mut capability_summary,
            capabilities,
            "semantic_search_status",
        );
        copy_summarized_field(&mut capability_summary, capabilities, "lsp_attached");
        copy_summarized_field(&mut capability_summary, capabilities, "health_summary");
        if !capability_summary.is_empty() {
            out.insert("capabilities".to_owned(), Value::Object(capability_summary));
        }
    }

    if let Some(visible_tools) = source
        .get("visible_tools")
        .and_then(|value| value.as_object())
    {
        let mut tools_summary = Map::new();
        copy_summarized_field(&mut tools_summary, visible_tools, "tool_count");
        copy_summarized_field(&mut tools_summary, visible_tools, "tool_names");
        copy_summarized_field(&mut tools_summary, visible_tools, "effective_namespaces");
        copy_summarized_field(&mut tools_summary, visible_tools, "effective_tiers");
        if !tools_summary.is_empty() {
            out.insert("visible_tools".to_owned(), Value::Object(tools_summary));
        }
    }

    if let Some(routing) = source.get("routing").and_then(|value| value.as_object()) {
        let mut routing_summary = Map::new();
        copy_summarized_field(&mut routing_summary, routing, "recommended_entrypoint");
        copy_summarized_field(&mut routing_summary, routing, "preferred_entrypoints");
        if !routing_summary.is_empty() {
            out.insert("routing".to_owned(), Value::Object(routing_summary));
        }
    }

    Value::Object(out)
}

fn summarize_text_object(source: &Map<String, Value>, depth: usize) -> Value {
    const MAX_OBJECT_ITEMS: usize = 8;

    let mut summarized = Map::new();
    let mut array_shrunk = false;
    // #211: previously the loop broke on the first cap hit and only
    // marked `truncated: true`, leaving callers with no way to know
    // which keys had been dropped. Collect every dropped key so the
    // response carries a `_omitted_keys` list and downstream agents
    // can request the missing fields explicitly.
    let mut omitted_keys: Vec<String> = Vec::new();
    for (index, (key, value)) in source.iter().enumerate() {
        if index >= MAX_OBJECT_ITEMS {
            omitted_keys.push(key.clone());
            continue;
        }
        if let Value::Array(items) = value
            && items.len() > TEXT_CHANNEL_MAX_ARRAY_ITEMS
        {
            array_shrunk = true;
        }
        summarized.insert(key.clone(), summarize_text_value(value, depth + 1));
    }
    if !omitted_keys.is_empty() {
        summarized.insert("truncated".to_owned(), Value::Bool(true));
        summarized.insert("_omitted_keys".to_owned(), json!(omitted_keys));
    }
    if source.contains_key("truncated") || array_shrunk {
        summarized.insert("truncated".to_owned(), Value::Bool(true));
    }
    Value::Object(summarized)
}

fn summarize_text_value(value: &Value, depth: usize) -> Value {
    match value {
        Value::Object(map) => summarize_text_object(map, depth),
        other => summarize_structured_content(other, depth),
    }
}

/// Adaptive compression based on OpenDev 5-stage strategy (arxiv:2603.05344).
/// Thresholds are adjusted by effort level offset (Low=-10, Medium=0, High=+10).
/// Stage 1: <75% budget → pass through
/// Stage 2: 75-85% → summarize structured content (depth=1)
/// Stage 3: 85-95% → aggressive summarize (depth=0)
/// Stage 4: 95-100% → drop structured content entirely
/// Stage 5: >100% → hard truncation to error payload
///
/// Returns (text, structured_content, truncation_info). When the payload
/// passes through (stage 1), `truncation_info` is `None`. When any
/// summarization or truncation happens, `truncation_info` carries the
/// stage, original payload size estimate, effective budget, and a
/// human-readable recovery hint so callers can surface the loss to
/// agents at the top level instead of burying `truncated: true` in the
/// data envelope.
pub(crate) fn bounded_result_payload(
    mut text: String,
    mut structured_content: Option<Value>,
    payload_estimate: usize,
    effective_budget: usize,
    effort_offset: i32,
) -> (String, Option<Value>, Option<TruncationInfo>) {
    let usage_pct = payload_estimate
        .checked_mul(100)
        .and_then(|v| v.checked_div(effective_budget))
        .unwrap_or(100);
    // Apply effort offset to thresholds (High effort delays compression)
    let t1 = (75i32 + effort_offset).clamp(50, 90) as usize;
    let t2 = (85i32 + effort_offset).clamp(60, 95) as usize;
    let t3 = (95i32 + effort_offset).clamp(70, 100) as usize;
    let t4 = (100i32 + effort_offset).clamp(80, 110) as usize;

    let max_chars = effective_budget * 8;
    let stage: u8;

    if usage_pct <= t1 {
        // Stage 1: pass through
        stage = 1;
    } else if usage_pct <= t2 {
        // Stage 2: light summarization
        stage = 2;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 1));
        }
    } else if usage_pct <= t3 {
        // Stage 3: aggressive summarization
        stage = 3;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
    } else if usage_pct <= t4 {
        // Stage 4: aggressive summarize structured + truncate text if needed
        stage = 4;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        if text.len() > max_chars {
            let byte_idx = text
                .char_indices()
                .nth(max_chars)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            text.truncate(byte_idx);
            text.push_str("...[truncated]");
        }
    } else {
        // Stage 5: hard truncation — summarize structured to minimal skeleton
        stage = 5;
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

    let info = if stage >= 2 {
        Some(TruncationInfo {
            stage,
            original_payload_estimate: payload_estimate,
            effective_budget,
            recovery_hint: recovery_hint_for_stage(stage, payload_estimate, effective_budget),
        })
    } else {
        None
    };
    (text, structured_content, info)
}

/// Top-level metadata describing what the adaptive compressor did.
///
/// This is surfaced into the structured response so that an agent can
/// detect "I asked for X but only got a summarized view" without having
/// to reach into the data envelope. Stage 1 (pass-through) returns
/// `None` from `bounded_result_payload`; stages 2–5 emit one of these.
#[derive(Debug, Clone)]
pub(crate) struct TruncationInfo {
    pub stage: u8,
    pub original_payload_estimate: usize,
    pub effective_budget: usize,
    pub recovery_hint: String,
}

impl TruncationInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "compression_stage": self.stage,
            "original_payload_estimate": self.original_payload_estimate,
            "effective_budget": self.effective_budget,
            "recovery_hint": self.recovery_hint,
        })
    }
}

/// When the call-graph extractor produced only `unresolved` edges and the
/// response was clipped, the default `recovery_hint` ("narrow with file_path
/// or smaller max_results") is misleading — the issue is extractor coverage,
/// not budget. Append a tool-aware grep cue that names the actual symbol so
/// an agent can fall back to direct text search instead of retrying the same
/// low-confidence call-graph query with smaller bounds.
///
/// No-op for stages < 4, missing structured content, or non-`unresolved_only`
/// confidence bases — the existing hint is correct in those cases.
pub(crate) fn enrich_recovery_hint_for_signals(
    info: TruncationInfo,
    structured_content: Option<&Value>,
) -> TruncationInfo {
    if info.stage < 4 {
        return info;
    }
    let Some(content) = structured_content.and_then(|v| v.as_object()) else {
        return info;
    };
    let basis = content
        .get("confidence_basis")
        .and_then(Value::as_str)
        .unwrap_or("");
    if basis != "unresolved_only" {
        return info;
    }
    let symbol = content
        .get("function")
        .or_else(|| content.get("symbol_name"))
        .or_else(|| content.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("the symbol");
    let mut recovery_hint = info.recovery_hint;
    recovery_hint.push_str(&format!(
        " Call graph returned only `unresolved` edges and the result was clipped — `Grep '{symbol}'` directly may surface raw matches the tree-sitter call query missed."
    ));
    TruncationInfo {
        recovery_hint,
        ..info
    }
}

fn recovery_hint_for_stage(stage: u8, estimate: usize, budget: usize) -> String {
    match stage {
        2 => format!(
            "Light summarization applied ({} of {} budget). Drill into a specific file or symbol for full detail.",
            estimate, budget
        ),
        3 => format!(
            "Aggressive summarization applied ({} of {} budget). Arrays clipped to 3 items each — use file_path / max_results to narrow scope.",
            estimate, budget
        ),
        4 => format!(
            "Near-budget summarization applied ({} of {} budget). Result arrays clipped — narrow scope with file_path or smaller max_results to recover the full set.",
            estimate, budget
        ),
        5 => format!(
            "Response over budget ({} tokens vs {}); structured arrays clipped to 3 items each. Use file_path / smaller max_results / get_analysis_section to recover items.",
            estimate, budget
        ),
        _ => "Compression applied".to_owned(),
    }
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
    tool_name: &str,
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
    result["_meta"] = json!({
        "codelens/preferredExecutor": crate::tool_defs::tool_preferred_executor_label(tool_name)
    });
    if let Some(max_chars) = max_result_size_chars {
        result["_meta"]["anthropic/maxResultSizeChars"] = json!(max_chars);
    }
    if let Some((since, replacement, removal)) = crate::tool_defs::tool_deprecation(tool_name) {
        result["_meta"]["codelens/deprecatedSince"] = json!(since);
        result["_meta"]["codelens/deprecatedReplacement"] = json!(replacement);
        result["_meta"]["codelens/deprecatedRemovalTarget"] = json!(removal);
    }
    JsonRpcResponse::result(id, result)
}

// Keeping `count`/`returned_count` metadata consistent with the visible array
// requires `summarize_text_object` and `summarize_structured_content` to share
// the same cap, so the shrink detector and the shrinker never disagree.
const TEXT_CHANNEL_MAX_ARRAY_ITEMS: usize = 3;

fn summarize_structured_content(value: &Value, depth: usize) -> Value {
    const MAX_STRING_CHARS: usize = 240;
    const MAX_ARRAY_ITEMS: usize = TEXT_CHANNEL_MAX_ARRAY_ITEMS;
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
            // Track sibling `<key>_omitted_count` markers so an agent
            // sees both the top-level `truncation_warning` AND the
            // per-array original size at the call site (e.g. callers
            // array clipped from 287 → 3 records `callers_omitted_count: 284`).
            let mut omitted_markers: Vec<(String, usize)> = Vec::new();
            for (index, (key, item)) in map.iter().enumerate() {
                if index >= max_items {
                    break;
                }
                if let Value::Array(items) = item
                    && items.len() > MAX_ARRAY_ITEMS
                {
                    omitted_markers.push((
                        format!("{key}_omitted_count"),
                        items.len() - MAX_ARRAY_ITEMS,
                    ));
                }
                summarized.insert(key.clone(), summarize_structured_content(item, depth + 1));
            }
            for (marker_key, omitted) in omitted_markers {
                if !summarized.contains_key(&marker_key) {
                    summarized.insert(marker_key, json!(omitted));
                }
            }
            if map.contains_key("truncated") {
                summarized.insert("truncated".to_owned(), Value::Bool(true));
            }
            Value::Object(summarized)
        }
    }
}

#[cfg(test)]
mod text_channel_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarize_text_object_records_omitted_keys_when_capped() {
        // #211: a 12-key payload with the cap at 8 used to silently lose
        // the trailing 4 keys with only `truncated: true` as a signal.
        // The new contract surfaces each dropped key by name so the
        // caller can either widen the request (detail=full) or request
        // the missing fields directly.
        let mut map = Map::new();
        for i in 0..12u32 {
            map.insert(format!("key{i:02}"), json!(i));
        }
        let result = summarize_text_object(&map, 0);
        let obj = result.as_object().expect("object");
        assert_eq!(
            obj.get("truncated").and_then(Value::as_bool),
            Some(true),
            "cap-driven drop must mark truncated=true"
        );
        let omitted = obj
            .get("_omitted_keys")
            .and_then(Value::as_array)
            .expect("_omitted_keys array present after cap drop");
        assert_eq!(
            omitted.len(),
            4,
            "12 keys - 8 cap = 4 dropped, got {omitted:?}"
        );
        let omitted_names: Vec<&str> = omitted.iter().filter_map(Value::as_str).collect();
        assert_eq!(
            omitted_names,
            vec!["key08", "key09", "key10", "key11"],
            "omitted_keys must list the trailing keys in iteration order"
        );
    }

    #[test]
    fn summarize_text_object_no_omitted_keys_when_under_cap() {
        // Stage 1 / under-cap behavior must stay clean — no `truncated`
        // flag and no `_omitted_keys` entry when nothing was dropped.
        let mut map = Map::new();
        for i in 0..5u32 {
            map.insert(format!("key{i}"), json!(i));
        }
        let result = summarize_text_object(&map, 0);
        let obj = result.as_object().expect("object");
        assert!(
            obj.get("_omitted_keys").is_none(),
            "no _omitted_keys when under cap, got {obj:?}"
        );
        assert!(
            obj.get("truncated").is_none(),
            "no truncated when under cap"
        );
    }

    #[test]
    fn shrinking_array_child_flags_parent_truncated() {
        let payload = json!({
            "references": [1, 2, 3, 4, 5],
            "count": 5,
            "returned_count": 5,
            "sampled": false,
        });
        let summarized = summarize_text_data_for_response(&payload);
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("truncated").and_then(Value::as_bool),
            Some(true),
            "parent must be flagged when an array child was shrunk"
        );
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(TEXT_CHANNEL_MAX_ARRAY_ITEMS)
        );
        assert_eq!(obj.get("count").and_then(Value::as_i64), Some(5));
        assert_eq!(obj.get("returned_count").and_then(Value::as_i64), Some(5));
    }

    #[test]
    fn short_array_leaves_parent_untruncated() {
        let payload = json!({
            "references": [1, 2],
            "count": 2,
            "returned_count": 2,
        });
        let summarized = summarize_text_data_for_response(&payload);
        let obj = summarized.as_object().expect("object");
        assert!(obj.get("truncated").is_none());
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn nested_object_array_shrink_flags_inner_parent() {
        let payload = json!({
            "outer": {
                "items": [1, 2, 3, 4],
                "total": 4,
            }
        });
        let summarized = summarize_text_data_for_response(&payload);
        let inner = summarized
            .get("outer")
            .and_then(Value::as_object)
            .expect("outer object");
        assert_eq!(inner.get("truncated").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn bounded_payload_passthrough_returns_no_truncation_info() {
        // Stage 1 passthrough: payload well below budget should report
        // no compression so callers can keep the fast path.
        let (text, structured, info) = bounded_result_payload(
            r#"{"ok":true}"#.to_owned(),
            Some(json!({"ok": true})),
            10,   // payload tokens
            1000, // budget
            0,    // medium effort
        );
        assert_eq!(text, r#"{"ok":true}"#);
        assert!(info.is_none(), "stage 1 must not surface truncation_info");
        assert!(structured.is_some());
    }

    #[test]
    fn bounded_payload_stage_5_emits_truncation_info_with_recovery_hint() {
        // Stage 5 (over-budget) is the dogfood case: Flask `route` extracted
        // 287 callers but the response showed 3, with `truncated: true`
        // buried in the data envelope. The new contract surfaces a
        // top-level TruncationInfo with stage + estimate + budget +
        // human-readable recovery_hint.
        let (_text, _structured, info) = bounded_result_payload(
            "x".repeat(50_000),
            Some(json!({
                "callers": (0..300).map(|n| json!({"name": n})).collect::<Vec<_>>(),
                "count": 300,
            })),
            12_000, // payload tokens (over budget)
            4_000,  // budget
            10,     // high effort
        );
        let info = info.expect("stage 5 must emit truncation info");
        assert_eq!(info.stage, 5);
        assert_eq!(info.original_payload_estimate, 12_000);
        assert_eq!(info.effective_budget, 4_000);
        assert!(
            info.recovery_hint.contains("12000")
                && info.recovery_hint.contains("4000")
                && (info.recovery_hint.contains("file_path")
                    || info.recovery_hint.contains("max_results")),
            "recovery_hint should include both estimate, budget, and a recovery cue: {}",
            info.recovery_hint
        );
        let info_json = info.to_json();
        assert_eq!(info_json["compression_stage"], json!(5));
        assert_eq!(info_json["original_payload_estimate"], json!(12_000));
        assert_eq!(info_json["effective_budget"], json!(4_000));
        assert!(info_json["recovery_hint"].is_string());
    }

    #[test]
    fn array_clipping_records_omitted_count_marker() {
        // When an array of length N gets clipped to MAX_ARRAY_ITEMS=3,
        // the parent object should carry `<key>_omitted_count` so the
        // call site (e.g. `data.callers`) shows how much was dropped.
        let payload = json!({
            "callers": (0..287).map(|n| json!({"name": n})).collect::<Vec<_>>(),
            "count": 287,
        });
        let summarized = summarize_structured_content(&payload, 0);
        let obj = summarized.as_object().expect("object");
        let arr = obj
            .get("callers")
            .and_then(Value::as_array)
            .expect("callers array");
        assert_eq!(arr.len(), TEXT_CHANNEL_MAX_ARRAY_ITEMS);
        assert_eq!(
            obj.get("callers_omitted_count").and_then(Value::as_i64),
            Some((287 - TEXT_CHANNEL_MAX_ARRAY_ITEMS) as i64),
            "expected callers_omitted_count to record dropped items"
        );
    }

    #[test]
    fn unresolved_only_truncation_appends_grep_fallback_hint() {
        // S2: when call-graph extractor reports `confidence_basis:
        // "unresolved_only"` and the response was clipped at stage 4-5,
        // the recovery hint should name the symbol and suggest a direct
        // grep rather than telling the agent to retry with smaller bounds
        // (which won't help — the extractor missed the edges).
        let info = TruncationInfo {
            stage: 5,
            original_payload_estimate: 12_000,
            effective_budget: 4_000,
            recovery_hint: recovery_hint_for_stage(5, 12_000, 4_000),
        };
        let structured = json!({
            "function": "register_route",
            "callers": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
            "confidence_basis": "unresolved_only",
        });
        let enriched = enrich_recovery_hint_for_signals(info, Some(&structured));
        assert!(
            enriched.recovery_hint.contains("Grep")
                && enriched.recovery_hint.contains("register_route"),
            "expected grep-fallback cue with symbol name, got: {}",
            enriched.recovery_hint
        );
        assert!(
            enriched.recovery_hint.contains("file_path")
                || enriched.recovery_hint.contains("max_results"),
            "base recovery_hint must remain (got: {})",
            enriched.recovery_hint
        );
    }

    #[test]
    fn mixed_resolution_truncation_keeps_default_hint() {
        // Negative case: when the call graph has real evidence
        // (import_evidence / mixed), the budget-narrowing hint is the
        // correct guidance — do not append the grep-fallback cue.
        let base = recovery_hint_for_stage(5, 12_000, 4_000);
        let info = TruncationInfo {
            stage: 5,
            original_payload_estimate: 12_000,
            effective_budget: 4_000,
            recovery_hint: base.clone(),
        };
        let structured = json!({
            "function": "register_route",
            "callers": [{"name": "a"}],
            "confidence_basis": "import_evidence",
        });
        let enriched = enrich_recovery_hint_for_signals(info, Some(&structured));
        assert_eq!(enriched.recovery_hint, base);
    }

    #[test]
    fn enrichment_skipped_for_stage_below_four() {
        // Stage 2-3 already get tool-agnostic guidance and leave the
        // structured payload mostly intact, so the unresolved_only label
        // here is informational, not a wall to recovery. Don't enrich.
        let base = recovery_hint_for_stage(3, 9_000, 10_000);
        let info = TruncationInfo {
            stage: 3,
            original_payload_estimate: 9_000,
            effective_budget: 10_000,
            recovery_hint: base.clone(),
        };
        let structured = json!({
            "function": "register_route",
            "confidence_basis": "unresolved_only",
        });
        let enriched = enrich_recovery_hint_for_signals(info, Some(&structured));
        assert_eq!(enriched.recovery_hint, base);
    }

    #[test]
    fn short_arrays_get_no_omitted_marker() {
        // Backward-compat: arrays at or below MAX_ARRAY_ITEMS keep the
        // current shape — no extra `_omitted_count` marker is added.
        let payload = json!({
            "callers": [{"name": "a"}, {"name": "b"}],
            "count": 2,
        });
        let summarized = summarize_structured_content(&payload, 0);
        let obj = summarized.as_object().expect("object");
        assert!(obj.get("callers_omitted_count").is_none());
    }
}
