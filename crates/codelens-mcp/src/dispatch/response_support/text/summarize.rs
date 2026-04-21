use serde_json::{Map, Value};

pub(super) const TEXT_CHANNEL_MAX_ARRAY_ITEMS: usize = 3;

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
    for (index, (key, value)) in source.iter().enumerate() {
        if index >= MAX_OBJECT_ITEMS {
            summarized.insert("truncated".to_owned(), Value::Bool(true));
            break;
        }
        if let Value::Array(items) = value
            && items.len() > TEXT_CHANNEL_MAX_ARRAY_ITEMS
        {
            array_shrunk = true;
        }
        summarized.insert(key.clone(), summarize_text_value(value, depth + 1));
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

pub(in crate::dispatch::response_support) fn summarize_structured_content(
    value: &Value,
    depth: usize,
) -> Value {
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

fn copy_summarized_field(target: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), summarize_structured_content(value, 0));
    }
}
