use crate::AppState;
use crate::protocol::{Tool, ToolPhase};
use crate::tool_defs::{
    ToolSurface, is_deferred_control_tool, preferred_bootstrap_tools, preferred_namespaces,
    preferred_tier_labels, tool_deprecation, tool_namespace, tool_phase_label, tool_tier_label,
    visible_namespaces, visible_tiers, visible_tools,
};

use super::ResourceRequestContext;

pub(crate) struct VisibleToolContext {
    pub(crate) tools: Vec<&'static Tool>,
    pub(crate) total_tool_count: usize,
    pub(crate) all_namespaces: Vec<&'static str>,
    pub(crate) all_tiers: Vec<&'static str>,
    pub(crate) preferred_namespaces: Vec<&'static str>,
    pub(crate) preferred_tiers: Vec<&'static str>,
    pub(crate) loaded_namespaces: Vec<String>,
    pub(crate) loaded_tiers: Vec<String>,
    pub(crate) effective_namespaces: Vec<String>,
    pub(crate) effective_tiers: Vec<String>,
    pub(crate) selected_namespace: Option<String>,
    pub(crate) selected_tier: Option<String>,
    pub(crate) deferred_loading_active: bool,
    pub(crate) full_tool_exposure: bool,
}

pub(crate) fn filter_listed_tools(
    tools: Vec<&'static Tool>,
    requested_phase: Option<ToolPhase>,
    include_deprecated: bool,
) -> Vec<&'static Tool> {
    tools
        .into_iter()
        .filter(|tool| include_deprecated || tool_deprecation(tool.name).is_none())
        .filter(|tool| match requested_phase {
            Some(phase) => {
                is_deferred_control_tool(tool.name)
                    || match tool_phase_label(tool.name) {
                        Some(label) => label == phase.as_label(),
                        None => true,
                    }
            }
            None => true,
        })
        .collect()
}

pub(crate) fn default_listed_tool_names(_surface: ToolSurface) -> &'static [&'static str] {
    crate::tool_defs::default_listed_tool_names()
}

pub(crate) fn filter_default_listed_tools(
    tools: Vec<&'static Tool>,
    request: &ResourceRequestContext,
    requested_phase: Option<ToolPhase>,
    surface: ToolSurface,
) -> Vec<&'static Tool> {
    if !request.default_listing_requested() || requested_phase.is_some() {
        return tools;
    }
    let default_names = default_listed_tool_names(surface);
    default_names
        .iter()
        .filter_map(|name| tools.iter().copied().find(|tool| tool.name == *name))
        .collect()
}

pub(crate) fn build_visible_tool_context(
    state: &AppState,
    request: &ResourceRequestContext,
) -> VisibleToolContext {
    let surface = state.execution_surface(&request.session);
    let all_tools = visible_tools(surface);
    let preferred = preferred_namespaces(surface);
    let preferred_bootstrap = preferred_bootstrap_tools(surface);
    let preferred_tiers = preferred_tier_labels(surface);
    let has_loaded_expansions =
        !request.session.loaded_namespaces.is_empty() || !request.session.loaded_tiers.is_empty();
    let mut tools = all_tools
        .iter()
        .copied()
        .filter(|tool| match request.requested_namespace.as_deref() {
            _ if request.deferred_loading_active() && is_deferred_control_tool(tool.name) => true,
            Some(namespace) => tool_namespace(tool.name) == namespace,
            None if request.deferred_loading_active() => {
                if is_deferred_control_tool(tool.name) {
                    return true;
                }
                let namespace = tool_namespace(tool.name);
                let tier = tool_tier_label(tool.name);
                preferred.contains(&namespace)
                    || request
                        .session
                        .loaded_tiers
                        .iter()
                        .any(|value| value == tier)
                    || request
                        .session
                        .loaded_namespaces
                        .iter()
                        .any(|value| value == namespace)
            }
            None => true,
        })
        .filter(|tool| match request.requested_tier.as_deref() {
            _ if request.deferred_loading_active() && is_deferred_control_tool(tool.name) => true,
            Some(tier) => tool_tier_label(tool.name) == tier,
            None if request.deferred_loading_active() => {
                if is_deferred_control_tool(tool.name) {
                    return true;
                }
                let namespace = tool_namespace(tool.name);
                let tier = tool_tier_label(tool.name);
                preferred_tiers.contains(&tier)
                    || request
                        .session
                        .loaded_namespaces
                        .iter()
                        .any(|value| value == namespace)
                    || request
                        .session
                        .loaded_tiers
                        .iter()
                        .any(|value| value == tier)
            }
            None => true,
        })
        .filter(|tool| match preferred_bootstrap {
            _ if request.deferred_loading_active() && is_deferred_control_tool(tool.name) => true,
            Some(tool_names) if request.deferred_loading_active() && !has_loaded_expansions => {
                tool_names.contains(&tool.name)
            }
            _ => true,
        })
        .collect::<Vec<_>>();
    if request.deferred_loading_active() && has_loaded_expansions {
        tools.sort_by_key(|tool| {
            let namespace = tool_namespace(tool.name);
            let tier = tool_tier_label(tool.name);
            let namespace_rank = if request
                .session
                .loaded_namespaces
                .iter()
                .any(|value| value == namespace)
            {
                0usize
            } else {
                1
            };
            let tier_rank = if request
                .session
                .loaded_tiers
                .iter()
                .any(|value| value == tier)
            {
                0usize
            } else {
                1
            };
            let control_rank = if is_deferred_control_tool(tool.name) {
                2usize
            } else {
                0
            };
            (namespace_rank, tier_rank, control_rank)
        });
    }

    let mut effective_namespaces = preferred
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    for namespace in &request.session.loaded_namespaces {
        if !effective_namespaces.iter().any(|value| value == namespace) {
            effective_namespaces.push(namespace.clone());
        }
    }
    effective_namespaces.sort();

    let mut effective_tiers = preferred_tiers
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    for tier in &request.session.loaded_tiers {
        if !effective_tiers.iter().any(|value| value == tier) {
            effective_tiers.push(tier.clone());
        }
    }
    effective_tiers.sort();

    VisibleToolContext {
        tools,
        total_tool_count: all_tools.len(),
        all_namespaces: visible_namespaces(surface),
        all_tiers: visible_tiers(surface),
        preferred_namespaces: preferred,
        preferred_tiers,
        loaded_namespaces: request.session.loaded_namespaces.clone(),
        loaded_tiers: request.session.loaded_tiers.clone(),
        effective_namespaces,
        effective_tiers,
        selected_namespace: request.requested_namespace.clone(),
        selected_tier: request.requested_tier.clone(),
        deferred_loading_active: request.deferred_loading_active(),
        full_tool_exposure: request.session.full_tool_exposure,
    }
}
