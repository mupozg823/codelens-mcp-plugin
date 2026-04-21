use crate::AppState;
use crate::protocol::Tool;
use crate::resources::context::{ResourceRequestContext, build_visible_tool_context};
use crate::tool_defs::{preferred_phase_labels, tool_preferred_executor_label, visible_tools};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

pub(crate) struct ToolListPayload {
    pub(crate) active_surface_label: &'static str,
    pub(crate) payload: Value,
    pub(crate) token_estimate: usize,
    pub(crate) deferred_expansion_requested: bool,
}

pub(crate) fn build_tools_list_payload(
    state: &AppState,
    params: Option<&Value>,
) -> ToolListPayload {
    let request = ResourceRequestContext::from_request("codelens://tools/list", params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let all_tools = visible_tools(surface);
    let (all_namespaces, all_tiers) = visible_axes_from_tools(&all_tools);
    let include_output_schema =
        list_param_bool(params, "includeOutputSchema", "include_output_schema")
            .unwrap_or(!(context.deferred_loading_active || request.lean_tool_contract()));
    let include_annotations = list_param_bool(params, "includeAnnotations", "include_annotations")
        .unwrap_or(!request.lean_tool_contract());

    let response_tools = context
        .tools
        .iter()
        .map(|tool| {
            let mut tool = (*tool).clone();
            let mut meta = json!({
                "codelens/preferredExecutor": tool_preferred_executor_label(tool.name),
            });
            if let Some(search_hint) = crate::tool_defs::tool_anthropic_search_hint(tool.name) {
                meta["anthropic/searchHint"] = json!(search_hint);
            }
            if crate::tool_defs::tool_anthropic_always_load(tool.name) {
                meta["anthropic/alwaysLoad"] = Value::Bool(true);
            }
            tool.meta = Some(meta);
            if !include_output_schema {
                tool.output_schema = None;
            }
            if !include_annotations {
                tool.annotations = None;
            }
            tool
        })
        .collect::<Vec<_>>();

    let token_estimate = if include_output_schema {
        context.tools.iter().map(|tool| tool.estimated_tokens).sum()
    } else {
        serde_json::to_string(&response_tools)
            .map(|body| body.len() / 4)
            .unwrap_or(0)
    };

    let mut payload = Map::new();
    payload.insert(
        "client_profile".to_owned(),
        Value::String(request.client_profile.as_str().to_owned()),
    );
    payload.insert(
        "active_surface".to_owned(),
        Value::String(surface.as_label().to_owned()),
    );
    payload.insert(
        "preferred_namespaces".to_owned(),
        json!(context.preferred_namespaces),
    );
    payload.insert("preferred_tiers".to_owned(), json!(context.preferred_tiers));
    payload.insert(
        "preferred_phases".to_owned(),
        json!(preferred_phase_labels(surface)),
    );
    payload.insert(
        "loaded_namespaces".to_owned(),
        json!(context.loaded_namespaces),
    );
    payload.insert("loaded_tiers".to_owned(), json!(context.loaded_tiers));
    payload.insert(
        "effective_namespaces".to_owned(),
        json!(context.effective_namespaces),
    );
    payload.insert("effective_tiers".to_owned(), json!(context.effective_tiers));
    payload.insert(
        "deferred_loading_active".to_owned(),
        Value::Bool(context.deferred_loading_active),
    );
    payload.insert(
        "include_output_schema".to_owned(),
        Value::Bool(include_output_schema),
    );
    payload.insert(
        "include_annotations".to_owned(),
        Value::Bool(include_annotations),
    );
    payload.insert(
        "default_contract_mode".to_owned(),
        Value::String(request.tool_contract_mode().to_owned()),
    );
    payload.insert("tool_count".to_owned(), json!(response_tools.len()));
    payload.insert("tool_count_total".to_owned(), json!(all_tools.len()));
    payload.insert("tools".to_owned(), json!(response_tools));

    if !request.lean_tool_contract() {
        payload.insert("visible_namespaces".to_owned(), json!(all_namespaces));
        payload.insert("visible_tiers".to_owned(), json!(all_tiers));
        payload.insert("full_listing".to_owned(), Value::Bool(request.full_listing));
        payload.insert(
            "full_tool_exposure".to_owned(),
            Value::Bool(request.full_tool_exposure),
        );
    }
    if let Some(namespace) = context.selected_namespace {
        payload.insert("selected_namespace".to_owned(), Value::String(namespace));
    }
    if let Some(tier) = context.selected_tier {
        payload.insert("selected_tier".to_owned(), Value::String(tier));
    }

    ToolListPayload {
        active_surface_label: surface.as_label(),
        payload: Value::Object(payload),
        token_estimate,
        deferred_expansion_requested: request.deferred_loading_requested
            && (request.requested_namespace.is_some() || request.requested_tier.is_some()),
    }
}

fn list_param_bool(params: Option<&Value>, camel: &str, snake: &str) -> Option<bool> {
    params
        .and_then(|params| params.get(camel).or_else(|| params.get(snake)))
        .and_then(Value::as_bool)
}

fn visible_axes_from_tools(tools: &[&'static Tool]) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut namespaces = BTreeSet::new();
    let mut tiers = BTreeSet::new();
    for tool in tools {
        namespaces.insert(crate::tool_defs::tool_namespace(tool.name));
        tiers.insert(crate::tool_defs::tool_tier_label(tool.name));
    }
    (
        namespaces.into_iter().collect(),
        tiers.into_iter().collect(),
    )
}
