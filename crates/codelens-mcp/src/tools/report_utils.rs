use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn strings_from_array(
    value: Option<&Vec<Value>>,
    field: &str,
    limit: usize,
) -> Vec<String> {
    value
        .into_iter()
        .flatten()
        .take(limit)
        .filter_map(|entry| {
            if let Some(text) = entry.as_str() {
                Some(text.to_owned())
            } else if let Some(obj) = entry.as_object() {
                obj.get(field)
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(entry.to_string()))
            } else {
                Some(entry.to_string())
            }
        })
        .collect()
}

pub(super) fn stable_cache_key(
    tool_name: &str,
    arguments: &Value,
    keys: &[&str],
) -> Option<String> {
    let mut fields = BTreeMap::new();
    for key in keys {
        if let Some(value) = arguments.get(*key)
            && !value.is_null()
        {
            fields.insert((*key).to_owned(), value.clone());
        }
    }
    if fields.is_empty() {
        None
    } else {
        Some(
            json!({
                "tool": tool_name,
                "fields": fields,
            })
            .to_string(),
        )
    }
}

pub(super) fn extract_handle_fields(payload: &Value) -> (Option<String>, Vec<String>) {
    let analysis_id = payload
        .get("analysis_id")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let estimated_sections = payload
        .get("available_sections")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (analysis_id, estimated_sections)
}
