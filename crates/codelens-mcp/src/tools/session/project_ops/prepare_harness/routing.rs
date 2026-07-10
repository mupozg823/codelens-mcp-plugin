use crate::resource_context::VisibleToolContext;
use crate::tool_defs::{
    AgentRole, ToolSurface, preferred_bootstrap_tools, tool_name_requests, tool_request_omissions,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) struct PrepareHarnessRoutingInput<'a> {
    pub(super) arguments: &'a Value,
    pub(super) active_surface: ToolSurface,
    pub(super) visible: &'a VisibleToolContext,
    pub(super) agent_role: Option<AgentRole>,
    pub(super) overlay_preferred_entrypoints: &'a [&'static str],
}

pub(super) struct PrepareHarnessRouting {
    pub(super) visible_tool_names: Vec<String>,
    pub(super) default_listed_tool_names: Vec<String>,
    pub(super) default_listed_tool_count: usize,
    pub(super) preferred_entrypoints: Vec<String>,
    pub(super) preferred_entrypoints_source: &'static str,
    pub(super) preferred_entrypoints_visible: Vec<String>,
    pub(super) preferred_entrypoints_omitted: Vec<Value>,
    pub(super) preferred_entrypoints_with_executors: Vec<Value>,
    pub(super) recommended_entrypoint: Option<String>,
    pub(super) recommended_entrypoint_preferred_executor: Option<&'static str>,
    pub(super) visible_executor_counts: BTreeMap<String, usize>,
    pub(super) overlay_agent_role: Option<&'static str>,
}

impl PrepareHarnessRouting {
    pub(super) fn preferred_entrypoints_visible_omitted_count(&self) -> usize {
        self.preferred_entrypoints
            .len()
            .saturating_sub(self.preferred_entrypoints_visible.len())
    }
}

pub(super) fn prepare_harness_routing(
    input: PrepareHarnessRoutingInput<'_>,
) -> PrepareHarnessRouting {
    let visible_tool_names = input
        .visible
        .tools
        .iter()
        .map(|tool| tool.name.to_owned())
        .collect::<Vec<_>>();
    let default_listed_tool_names = crate::tool_defs::default_listed_tool_names()
        .iter()
        .filter(|name| visible_tool_names.iter().any(|visible| visible == **name))
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let default_listed_tool_count = default_listed_tool_names.len();
    let requested_entrypoints = requested_entrypoints(input.arguments);
    let overlay_preferred_entrypoints = to_owned_tools(input.overlay_preferred_entrypoints);
    let preferred_entrypoints_source =
        preferred_entrypoints_source(&requested_entrypoints, &overlay_preferred_entrypoints);
    let preferred_entrypoint_requests = if !requested_entrypoints.is_empty() {
        tool_name_requests(requested_entrypoints)
    } else if !overlay_preferred_entrypoints.is_empty() {
        tool_name_requests(overlay_preferred_entrypoints.clone())
    } else {
        tool_name_requests(
            preferred_bootstrap_tools(input.active_surface)
                .unwrap_or(&[])
                .iter()
                .map(|tool| (*tool).to_owned())
                .collect::<Vec<_>>(),
        )
    };
    let preferred_entrypoints = preferred_entrypoint_requests
        .iter()
        .map(|request| request.tool.clone())
        .collect::<Vec<_>>();
    let preferred_entrypoints_visible = visible_subset(&preferred_entrypoints, &visible_tool_names);
    let preferred_entrypoints_with_executors = preferred_entrypoints_visible
        .iter()
        .map(|tool| {
            json!({
                "tool": tool,
                "preferred_executor": crate::tool_defs::tool_preferred_executor_label(tool),
            })
        })
        .collect::<Vec<_>>();
    let preferred_entrypoints_omitted = tool_request_omissions(
        &preferred_entrypoint_requests,
        &preferred_entrypoints_visible,
        input.active_surface,
        input.visible.deferred_loading_active,
    );
    let recommended_entrypoint = preferred_entrypoints_visible.first().cloned();
    let recommended_entrypoint_preferred_executor = recommended_entrypoint
        .as_deref()
        .map(crate::tool_defs::tool_preferred_executor_label);

    PrepareHarnessRouting {
        visible_tool_names,
        default_listed_tool_names,
        default_listed_tool_count,
        preferred_entrypoints,
        preferred_entrypoints_source,
        preferred_entrypoints_visible,
        preferred_entrypoints_omitted,
        preferred_entrypoints_with_executors,
        recommended_entrypoint,
        recommended_entrypoint_preferred_executor,
        visible_executor_counts: visible_executor_counts(input.visible),
        overlay_agent_role: input.agent_role.map(|value| value.as_str()),
    }
}

fn requested_entrypoints(arguments: &Value) -> Vec<String> {
    arguments
        .get("preferred_entrypoints")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn preferred_entrypoints_source(
    requested_entrypoints: &[String],
    overlay_preferred_entrypoints: &[String],
) -> &'static str {
    if !requested_entrypoints.is_empty() {
        "provided"
    } else if !overlay_preferred_entrypoints.is_empty() {
        "overlay"
    } else {
        "surface_default"
    }
}

fn visible_subset(tools: &[String], visible_tool_names: &[String]) -> Vec<String> {
    tools
        .iter()
        .filter(|tool| visible_tool_names.iter().any(|name| name == *tool))
        .cloned()
        .collect()
}

fn to_owned_tools(tools: &[&'static str]) -> Vec<String> {
    tools.iter().map(|tool| (*tool).to_owned()).collect()
}

fn visible_executor_counts(visible: &VisibleToolContext) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for tool in &visible.tools {
        *counts
            .entry(crate::tool_defs::tool_preferred_executor_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    counts
}
