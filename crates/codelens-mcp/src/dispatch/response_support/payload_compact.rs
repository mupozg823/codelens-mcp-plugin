use crate::protocol::ToolCallResponse;
use serde_json::{Map, Value, json};

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

pub(super) fn insert_if_present(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), value);
    }
}

pub(super) fn copy_summarized_field(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), summarize_structured_content(value, 0));
    }
}

fn copy_raw_field(target: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

pub(super) fn summarize_text_data_for_response(value: &Value) -> Value {
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
        copy_summarized_field(
            &mut tools_summary,
            visible_tools,
            "tool_names_omitted_count",
        );
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
        copy_summarized_field(
            &mut routing_summary,
            routing,
            "preferred_entrypoints_visible",
        );
        copy_summarized_field(
            &mut routing_summary,
            routing,
            "preferred_entrypoints_visible_omitted_count",
        );
        // Omission records are already compact, machine-readable recovery
        // hints (`reason`, `recommended_action`, surface/profile). Preserve
        // them verbatim for text-only MCP clients so the fallback channel
        // remains operationally equivalent to `structuredContent`.
        copy_raw_field(
            &mut routing_summary,
            routing,
            "preferred_entrypoints_omitted",
        );
        if !routing_summary.is_empty() {
            out.insert("routing".to_owned(), Value::Object(routing_summary));
        }
    }

    Value::Object(out)
}

pub(super) fn summarize_text_object(source: &Map<String, Value>, depth: usize) -> Value {
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

// Keeping `count`/`returned_count` metadata consistent with the visible array
// requires `summarize_text_object` and `summarize_structured_content` to share
// the same cap, so the shrink detector and the shrinker never disagree.
pub(super) const TEXT_CHANNEL_MAX_ARRAY_ITEMS: usize = 3;

pub(super) fn summarize_structured_content(value: &Value, depth: usize) -> Value {
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
                if should_preserve_structured_array(key, item) {
                    summarized.insert(key.clone(), item.clone());
                    continue;
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

fn should_preserve_structured_array(key: &str, value: &Value) -> bool {
    matches!(
        key,
        "preferred_entrypoints"
            | "preferred_entrypoints_visible"
            | "preferred_entrypoints_omitted"
            | "preferred_entrypoints_with_executors"
    ) && value.is_array()
}
