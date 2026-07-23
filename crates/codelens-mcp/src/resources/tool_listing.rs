use crate::AppState;
use crate::resource_context::{
    ResourceRequestContext, build_visible_tool_context, filter_default_listed_tools,
    filter_listed_tools,
};
use crate::tool_defs::{tool_execution_policy, tool_namespace, tool_tier_label};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) fn visible_tool_summary(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let lean_contract = request.lean_tool_contract();
    let listed_tools = filter_default_listed_tools(
        filter_listed_tools(context.tools.clone(), None, false),
        &request,
        None,
        surface,
    );
    let mut namespace_counts = BTreeMap::new();
    let mut tier_counts = BTreeMap::new();
    let mut execution_class_counts = BTreeMap::new();
    for tool in &listed_tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *tier_counts
            .entry(tool_tier_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
        if let Some(policy) = tool_execution_policy(tool.name) {
            *execution_class_counts
                .entry(policy.execution_class.to_owned())
                .or_insert(0usize) += 1;
        }
    }
    let prioritized = listed_tools
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "tier": tool_tier_label(tool.name),
                "execution_policy": tool_execution_policy(tool.name)
            })
        })
        .collect::<Vec<_>>();
    let mut payload = serde_json::Map::new();
    payload.insert(
        "client_profile".to_owned(),
        json!(context_request_client_profile(&request)),
    );
    payload.insert("active_surface".to_owned(), json!(surface.as_label()));
    payload.insert(
        "default_contract_mode".to_owned(),
        json!(request.tool_contract_mode()),
    );
    payload.insert("tool_count".to_owned(), json!(listed_tools.len()));
    payload.insert(
        "tool_count_total".to_owned(),
        json!(context.total_tool_count),
    );
    payload.insert(
        "preferred_namespaces".to_owned(),
        json!(context.preferred_namespaces),
    );
    payload.insert("preferred_tiers".to_owned(), json!(context.preferred_tiers));
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
        json!(context.deferred_loading_active),
    );
    payload.insert(
        "execution_classes".to_owned(),
        json!(execution_class_counts),
    );
    payload.insert("recommended_tools".to_owned(), json!(prioritized));
    payload.insert(
        "note".to_owned(),
        json!("Read `codelens://tools/list/full` only when summary is insufficient."),
    );
    if !lean_contract {
        payload.insert("visible_namespaces".to_owned(), json!(namespace_counts));
        payload.insert("visible_tiers".to_owned(), json!(tier_counts));
        payload.insert("all_namespaces".to_owned(), json!(context.all_namespaces));
        payload.insert("all_tiers".to_owned(), json!(context.all_tiers));
        payload.insert(
            "full_tool_exposure".to_owned(),
            json!(context.full_tool_exposure),
        );
    }
    if let Some(namespace) = context.selected_namespace {
        payload.insert("selected_namespace".to_owned(), json!(namespace));
    }
    if let Some(tier) = context.selected_tier {
        payload.insert("selected_tier".to_owned(), json!(tier));
    }
    Value::Object(payload)
}

pub(crate) fn visible_tool_details(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let tools = filter_default_listed_tools(
        filter_listed_tools(context.tools, None, false),
        &request,
        None,
        surface,
    )
    .into_iter()
    .map(|tool| {
        json!({
            "name": tool.name,
            "namespace": tool_namespace(tool.name),
            "description": tool.description,
            "tier": tool_tier_label(tool.name),
            "execution_policy": tool_execution_policy(tool.name)
        })
    })
    .collect::<Vec<_>>();
    json!({
        "client_profile": context_request_client_profile(&request),
        "active_surface": surface.as_label(),
        "default_contract_mode": request.client_profile.default_tool_contract_mode(),
        "tool_count": tools.len(),
        "tool_count_total": context.total_tool_count,
        "all_namespaces": context.all_namespaces,
        "all_tiers": context.all_tiers,
        "preferred_namespaces": context.preferred_namespaces,
        "preferred_tiers": context.preferred_tiers,
        "loaded_namespaces": context.loaded_namespaces,
        "loaded_tiers": context.loaded_tiers,
        "effective_namespaces": context.effective_namespaces,
        "effective_tiers": context.effective_tiers,
        "selected_namespace": context.selected_namespace,
        "selected_tier": context.selected_tier,
        "deferred_loading_active": context.deferred_loading_active,
        "full_tool_exposure": context.full_tool_exposure,
        "tools": tools
    })
}

fn context_request_client_profile(
    request: &crate::resource_context::ResourceRequestContext,
) -> &'static str {
    request.client_profile.as_str()
}
