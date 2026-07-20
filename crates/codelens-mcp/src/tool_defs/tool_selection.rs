// Sibling-module imports on purpose: pulling these through `super`
// (mod.rs re-exports) forms a mod.rs ↔ tool_selection.rs import cycle
// in the architecture graph.
use super::build::{tool_definition, tool_tier_label};
use super::presets::{
    ALL_PRESETS, ALL_PROFILES, ToolSurface, is_tool_in_surface, tool_namespace,
    tool_preferred_executor_label,
};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolNameRequest {
    pub(crate) requested_tool: String,
    pub(crate) tool: String,
}

pub(crate) fn normalize_tool_request_name(tool: &str) -> String {
    let trimmed = tool.trim();
    trimmed
        .strip_prefix("mcp__codelens__")
        .unwrap_or(trimmed)
        .to_owned()
}

pub(crate) fn tool_name_requests(
    tool_names: impl IntoIterator<Item = String>,
) -> Vec<ToolNameRequest> {
    let mut seen = HashSet::new();
    tool_names
        .into_iter()
        .filter_map(|requested_tool| {
            let requested_tool = requested_tool.trim().to_owned();
            if requested_tool.is_empty() || !seen.insert(requested_tool.clone()) {
                return None;
            }
            Some(ToolNameRequest {
                tool: normalize_tool_request_name(&requested_tool),
                requested_tool,
            })
        })
        .collect()
}

pub(crate) fn parse_tool_selection_requests(params: Option<&Value>) -> Vec<ToolNameRequest> {
    let Some(params) = params else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for key in ["tool_names", "names"] {
        if let Some(items) = params.get(key).and_then(Value::as_array) {
            names.extend(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned),
            );
        }
    }
    if let Some(value) = params.get("select").and_then(Value::as_str) {
        names.extend(parse_select_expression(value));
    }
    if let Some(value) = params.get("query").and_then(Value::as_str) {
        let trimmed = value.trim();
        if trimmed.starts_with("select:") || trimmed.starts_with("select=") {
            names.extend(parse_select_expression(trimmed));
        }
    }
    tool_name_requests(names)
}

fn parse_select_expression(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let select_body = trimmed
        .strip_prefix("select:")
        .or_else(|| trimmed.strip_prefix("select="))
        .unwrap_or(trimmed);
    select_body
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .map(|part| part.trim_matches(|ch| ch == '\'' || ch == '"' || ch == '`'))
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn surfaces_including_tool(tool: &str) -> Vec<&'static str> {
    ALL_PRESETS
        .iter()
        .map(|preset| ToolSurface::Preset(*preset))
        .chain(
            ALL_PROFILES
                .iter()
                .filter(|profile| !profile.is_deprecated())
                .map(|profile| ToolSurface::Profile(*profile)),
        )
        .filter(|surface| is_tool_in_surface(tool, *surface))
        .map(|surface| surface.as_label())
        .collect()
}

fn recommended_profile_for_tool(tool: &str) -> Option<&'static str> {
    ALL_PROFILES
        .iter()
        .copied()
        .find(|profile| {
            !profile.is_deprecated() && is_tool_in_surface(tool, ToolSurface::Profile(*profile))
        })
        .map(|profile| profile.as_str())
}

fn insert_requested_tool(map: &mut serde_json::Map<String, Value>, request: &ToolNameRequest) {
    if request.requested_tool != request.tool {
        map.insert("requested_tool".to_owned(), json!(request.requested_tool));
    }
}

fn omission_payload(
    request: &ToolNameRequest,
    active_surface: ToolSurface,
    deferred_loading_active: bool,
) -> Value {
    let tool = request.tool.as_str();
    if tool_definition(tool).is_none() {
        let mut omission = serde_json::Map::new();
        omission.insert("tool".to_owned(), json!(tool));
        insert_requested_tool(&mut omission, request);
        omission.insert("reason".to_owned(), json!("unknown_tool"));
        omission.insert(
            "recommended_action".to_owned(),
            json!("fix_preferred_entrypoint"),
        );
        return Value::Object(omission);
    }

    let included_in = surfaces_including_tool(tool);
    let active_surface_contains = is_tool_in_surface(tool, active_surface);
    let hidden_by_deferred_loading = deferred_loading_active && active_surface_contains;
    let mut omission = serde_json::Map::new();
    omission.insert("tool".to_owned(), json!(tool));
    insert_requested_tool(&mut omission, request);
    if hidden_by_deferred_loading {
        let namespace = tool_namespace(tool);
        let tier = tool_tier_label(tool);
        omission.insert("reason".to_owned(), json!("deferred_tool_not_loaded"));
        omission.insert(
            "recommended_action".to_owned(),
            json!("load_deferred_tool_namespace"),
        );
        omission.insert("tool_namespace".to_owned(), json!(namespace));
        omission.insert(
            "tool_loading_request".to_owned(),
            json!({
                "method": "tools/list",
                "params": {
                    "namespace": namespace,
                    "tier": tier,
                },
            }),
        );
    } else {
        omission.insert("reason".to_owned(), json!("not_in_active_surface"));
        omission.insert(
            "recommended_action".to_owned(),
            json!("switch_tool_surface"),
        );
        if let Some(profile) = recommended_profile_for_tool(tool) {
            omission.insert("recommended_profile".to_owned(), json!(profile));
        }
    }
    omission.insert(
        "preferred_executor".to_owned(),
        json!(tool_preferred_executor_label(tool)),
    );
    omission.insert("tool_tier".to_owned(), json!(tool_tier_label(tool)));
    omission.insert("included_in".to_owned(), json!(included_in));
    Value::Object(omission)
}

pub(crate) fn tool_request_omissions(
    requests: &[ToolNameRequest],
    visible_tools: &[String],
    active_surface: ToolSurface,
    deferred_loading_active: bool,
) -> Vec<Value> {
    requests
        .iter()
        .filter(|request| !visible_tools.iter().any(|visible| visible == &request.tool))
        .map(|request| omission_payload(request, active_surface, deferred_loading_active))
        .collect()
}

pub(crate) fn tool_selection_diagnostics(
    requests: &[ToolNameRequest],
    listed_tools: &[String],
    visible_tools: &[String],
    active_surface: ToolSurface,
    deferred_loading_active: bool,
) -> Value {
    let mut results = Vec::new();
    let mut found = Vec::new();
    let mut not_found = Vec::new();

    for request in requests {
        let tool = request.tool.as_str();
        if listed_tools.iter().any(|listed| listed == tool) {
            let mut result = serde_json::Map::new();
            result.insert("tool".to_owned(), json!(tool));
            insert_requested_tool(&mut result, request);
            result.insert("status".to_owned(), json!("found"));
            result.insert("tool_namespace".to_owned(), json!(tool_namespace(tool)));
            result.insert("tool_tier".to_owned(), json!(tool_tier_label(tool)));
            let result = Value::Object(result);
            found.push(result.clone());
            results.push(result);
            continue;
        }

        let mut omission = omission_payload(request, active_surface, deferred_loading_active);
        let object = omission.as_object_mut().expect("omission object");
        object.insert("status".to_owned(), json!("not_found"));
        if object
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason == "not_in_active_surface")
            && is_tool_in_surface(tool, active_surface)
            && visible_tools.iter().any(|visible| visible == tool)
        {
            object.insert("reason".to_owned(), json!("not_in_current_listing"));
            object.insert(
                "recommended_action".to_owned(),
                json!("reissue_tools_list_full_or_adjust_filter"),
            );
            object.insert(
                "tool_listing_request".to_owned(),
                json!({
                    "method": "tools/list",
                    "params": { "full": true },
                }),
            );
        }
        not_found.push(omission.clone());
        results.push(omission);
    }

    json!({
        "requested_count": requests.len(),
        "found_count": found.len(),
        "not_found_count": not_found.len(),
        "found": found,
        "not_found": not_found,
        "results": results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_defs::presets::ToolProfile;

    #[test]
    fn parse_select_expression_accepts_toolsearch_style_query() {
        let requests = parse_tool_selection_requests(Some(&json!({
            "query": "select:mcp__codelens__impact_report,mcp__codelens__get_ranked_context"
        })));
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].tool, "impact_report");
        assert_eq!(requests[0].requested_tool, "mcp__codelens__impact_report");
        assert_eq!(requests[1].tool, "get_ranked_context");
    }

    #[test]
    fn parse_tool_selection_ignores_non_select_query() {
        let requests = parse_tool_selection_requests(Some(&json!({
            "query": "impact report"
        })));
        assert!(requests.is_empty());
    }

    #[test]
    fn tool_selection_diagnostics_marks_current_listing_misses() {
        let requests = tool_name_requests(vec!["impact_report".to_owned()]);
        let diagnostics = tool_selection_diagnostics(
            &requests,
            &[],
            &["impact_report".to_owned()],
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            false,
        );
        assert_eq!(diagnostics["not_found_count"], json!(1));
        assert_eq!(
            diagnostics["not_found"][0]["reason"],
            json!("not_in_current_listing")
        );
        assert_eq!(
            diagnostics["not_found"][0]["recommended_action"],
            json!("reissue_tools_list_full_or_adjust_filter")
        );
    }
}
