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
        .or_else(|| {
            payload
                .get("section_handles")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            item.get("section")
                                .and_then(|value| value.as_str())
                                .map(ToOwned::to_owned)
                        })
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_default();
    (analysis_id, estimated_sections)
}

#[cfg(test)]
mod tests {
    use super::extract_handle_fields;
    use serde_json::json;

    #[test]
    fn extract_handle_fields_prefers_available_sections() {
        let payload = json!({
            "analysis_id": "analysis-1",
            "available_sections": ["summary", "impact_rows"],
            "section_handles": [
                {"section": "stale", "uri": "codelens://analysis/analysis-1/stale"}
            ],
        });

        let (analysis_id, sections) = extract_handle_fields(&payload);
        assert_eq!(analysis_id.as_deref(), Some("analysis-1"));
        assert_eq!(sections, vec!["summary", "impact_rows"]);
    }

    #[test]
    fn extract_handle_fields_falls_back_to_section_handles() {
        let payload = json!({
            "analysis_id": "analysis-2",
            "section_handles": [
                {"section": "boundary", "uri": "codelens://analysis/analysis-2/boundary"},
                {"section": "tests", "uri": "codelens://analysis/analysis-2/tests"}
            ],
        });

        let (analysis_id, sections) = extract_handle_fields(&payload);
        assert_eq!(analysis_id.as_deref(), Some("analysis-2"));
        assert_eq!(sections, vec!["boundary", "tests"]);
    }
}
