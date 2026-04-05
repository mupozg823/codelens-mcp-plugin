use crate::AppState;
use crate::resource_context::{ResourceRequestContext, build_visible_tool_context};
use crate::tool_defs::{tool_namespace, tool_tier_label};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) fn static_resource_entries(project_name: &str) -> Vec<Value> {
    vec![
        json!({
            "uri": "codelens://project/overview",
            "name": format!("Project: {project_name}"),
            "description": "Compressed project overview with active surface and index status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://project/architecture",
            "name": "Project Architecture",
            "description": "High-level architecture summary for harness planning",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list",
            "name": "Visible Tool Surface",
            "description": "Compressed role-aware tool surface summary",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list/full",
            "name": "Visible Tool Surface (Full)",
            "description": "Expanded role-aware tool surface with descriptions",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://stats/token-efficiency",
            "name": "Token Efficiency Stats",
            "description": "Session-level token, chain, and handle reuse metrics",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://session/http",
            "name": "HTTP Session Runtime",
            "description": "Shared daemon session counts, timeout, and resume support",
            "mimeType": "application/json"
        }),
    ]
}

pub(crate) fn visible_tool_summary(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let surface = *state.surface();
    let request = ResourceRequestContext::from_request(uri, params);
    let context = build_visible_tool_context(state, &request);
    let mut namespace_counts = BTreeMap::new();
    let mut tier_counts = BTreeMap::new();
    for tool in &context.tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *tier_counts
            .entry(tool_tier_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    let prioritized = context
        .tools
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "tier": tool_tier_label(tool.name)
            })
        })
        .collect::<Vec<_>>();
    json!({
        "client_profile": context_request_client_profile(&request),
        "active_surface": surface.as_label(),
        "default_contract_mode": request.client_profile.default_tool_contract_mode(),
        "tool_count": context.tools.len(),
        "tool_count_total": context.total_tool_count,
        "visible_namespaces": namespace_counts,
        "visible_tiers": tier_counts,
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
        "recommended_tools": prioritized,
        "note": "Read `codelens://tools/list/full` only when summary is insufficient."
    })
}

pub(crate) fn visible_tool_details(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let surface = *state.surface();
    let request = ResourceRequestContext::from_request(uri, params);
    let context = build_visible_tool_context(state, &request);
    let tools = context
        .tools
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "description": tool.description,
                "tier": tool_tier_label(tool.name)
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
