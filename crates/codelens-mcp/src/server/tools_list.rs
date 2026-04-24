use crate::AppState;
use crate::protocol::JsonRpcRequest;
use crate::resource_context::{
    ResourceRequestContext, build_visible_tool_context, filter_default_listed_tools,
    filter_listed_tools,
};
use crate::tool_defs::{preferred_phase_labels, tool_preferred_executor_label};
use serde_json::{Map, Value, json};

fn list_param_bool(request: &JsonRpcRequest, camel: &str, snake: &str) -> Option<bool> {
    request
        .params
        .as_ref()
        .and_then(|params| params.get(camel).or_else(|| params.get(snake)))
        .and_then(|value| value.as_bool())
}

pub(crate) fn build_tools_list_response(
    state: &AppState,
    request: &JsonRpcRequest,
    connector_safe: bool,
) -> Value {
    let request_context =
        ResourceRequestContext::from_request("codelens://tools/list", request.params.as_ref());
    let surface = state.execution_surface(&request_context.session);
    let visible_context = build_visible_tool_context(state, &request_context);
    let requested_phase = request
        .params
        .as_ref()
        .and_then(|params| params.get("phase"))
        .and_then(|value| value.as_str())
        .and_then(crate::protocol::ToolPhase::from_label);
    let requested_phase_param = request
        .params
        .as_ref()
        .and_then(|params| params.get("phase"))
        .is_some();
    let full_listing = request_context.full_listing;
    let lean_contract = request_context.lean_tool_contract() || connector_safe;
    let include_output_schema =
        list_param_bool(request, "includeOutputSchema", "include_output_schema")
            .unwrap_or(!(visible_context.deferred_loading_active || lean_contract));
    let include_annotations = list_param_bool(request, "includeAnnotations", "include_annotations")
        .unwrap_or(!lean_contract);
    let include_deprecated =
        list_param_bool(request, "includeDeprecated", "include_deprecated").unwrap_or(false);
    let listed = filter_listed_tools(
        visible_context.tools.clone(),
        requested_phase,
        include_deprecated,
    );
    let filtered = if requested_phase_param {
        listed
    } else {
        filter_default_listed_tools(listed, &request_context, requested_phase, surface)
    };
    let response_tools = filtered
        .iter()
        .map(|tool| {
            let mut tool = (*tool).clone();
            tool.title = tool
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.title.clone());
            let mut meta = json!({
                "codelens/preferredExecutor": tool_preferred_executor_label(tool.name),
            });
            if let Some(search_hint) = crate::tool_defs::tool_anthropic_search_hint(tool.name) {
                meta["anthropic/searchHint"] = json!(search_hint);
            }
            if crate::tool_defs::tool_anthropic_always_load(tool.name) {
                meta["anthropic/alwaysLoad"] = Value::Bool(true);
            }
            if let Some((since, replacement, removal)) =
                crate::tool_defs::tool_deprecation(tool.name)
            {
                meta["codelens/deprecatedSince"] = json!(since);
                meta["codelens/deprecatedReplacement"] = json!(replacement);
                meta["codelens/deprecatedRemovalTarget"] = json!(removal);
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
        filtered.iter().map(|tool| tool.estimated_tokens).sum()
    } else {
        serde_json::to_string(&response_tools)
            .map(|body| body.len() / 4)
            .unwrap_or(0)
    };
    if request_context.deferred_loading_requested
        && (request_context.requested_namespace.is_some()
            || request_context.requested_tier.is_some())
    {
        state.metrics().record_deferred_namespace_expansion();
    }
    state.metrics().record_call_with_tokens(
        "tools/list",
        0,
        true,
        token_estimate,
        surface.as_label(),
        false,
        None,
    );
    let mut payload = Map::new();
    payload.insert(
        "client_profile".to_owned(),
        Value::String(request_context.client_profile.as_str().to_owned()),
    );
    payload.insert(
        "active_surface".to_owned(),
        Value::String(surface.as_label().to_owned()),
    );
    payload.insert(
        "preferred_namespaces".to_owned(),
        json!(visible_context.preferred_namespaces),
    );
    payload.insert(
        "preferred_tiers".to_owned(),
        json!(visible_context.preferred_tiers),
    );
    payload.insert(
        "preferred_phases".to_owned(),
        json!(preferred_phase_labels(surface)),
    );
    payload.insert(
        "loaded_namespaces".to_owned(),
        json!(visible_context.loaded_namespaces),
    );
    payload.insert(
        "loaded_tiers".to_owned(),
        json!(visible_context.loaded_tiers),
    );
    payload.insert(
        "effective_namespaces".to_owned(),
        json!(visible_context.effective_namespaces),
    );
    payload.insert(
        "effective_tiers".to_owned(),
        json!(visible_context.effective_tiers),
    );
    payload.insert(
        "deferred_loading_active".to_owned(),
        Value::Bool(visible_context.deferred_loading_active),
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
        Value::String(
            request_context
                .client_profile
                .default_tool_contract_mode()
                .to_owned(),
        ),
    );
    payload.insert("tool_count".to_owned(), json!(response_tools.len()));
    payload.insert(
        "tool_count_total".to_owned(),
        json!(visible_context.total_tool_count),
    );
    let tools = json!(response_tools);
    payload.insert("tools".to_owned(), tools.clone());

    if connector_safe {
        let mut connector_payload = Map::new();
        connector_payload.insert("tools".to_owned(), tools);
        return Value::Object(connector_payload);
    }

    if !lean_contract {
        payload.insert(
            "visible_namespaces".to_owned(),
            json!(visible_context.all_namespaces),
        );
        payload.insert("visible_tiers".to_owned(), json!(visible_context.all_tiers));
        payload.insert("full_listing".to_owned(), Value::Bool(full_listing));
        payload.insert(
            "full_tool_exposure".to_owned(),
            Value::Bool(visible_context.full_tool_exposure),
        );
    }
    if let Some(namespace) = visible_context.selected_namespace.as_deref() {
        payload.insert(
            "selected_namespace".to_owned(),
            Value::String(namespace.to_owned()),
        );
    }
    if let Some(tier) = visible_context.selected_tier.as_deref() {
        payload.insert("selected_tier".to_owned(), Value::String(tier.to_owned()));
    }

    Value::Object(payload)
}
