use super::{
    ToolSurface, is_tool_in_surface, preferred_bootstrap_tools, preferred_namespaces,
    preferred_tiers, tool_namespace, tool_tier, tools,
};

fn raw_visible_tool_entries(surface: ToolSurface) -> Vec<(usize, &'static crate::protocol::Tool)> {
    tools()
        .iter()
        .enumerate()
        .filter(|(_, tool)| is_tool_in_surface(tool.name, surface))
        .collect::<Vec<_>>()
}

fn raw_visible_tools(surface: ToolSurface) -> Vec<&'static crate::protocol::Tool> {
    raw_visible_tool_entries(surface)
        .into_iter()
        .map(|(_, tool)| tool)
        .collect()
}

fn raw_visible_namespaces(surface: ToolSurface) -> Vec<&'static str> {
    let mut namespaces = raw_visible_tools(surface)
        .into_iter()
        .map(|tool| tool_namespace(tool.name))
        .collect::<Vec<_>>();
    namespaces.sort_unstable();
    namespaces.dedup();
    namespaces
}

pub(crate) fn visible_tools(surface: ToolSurface) -> Vec<&'static crate::protocol::Tool> {
    let preferred_bootstrap = preferred_bootstrap_tools(surface);
    let preferred_tiers = preferred_tiers(surface);
    let preferred_namespaces = preferred_namespaces(surface);
    let mut visible = raw_visible_tool_entries(surface);
    visible.sort_by_key(|(index, tool)| {
        let bootstrap_rank = preferred_bootstrap
            .and_then(|tool_names| tool_names.iter().position(|name| *name == tool.name))
            .unwrap_or(usize::MAX);
        let tier_rank = preferred_tiers
            .iter()
            .position(|tier| *tier == tool_tier(tool.name))
            .unwrap_or(usize::MAX);
        let namespace_rank = preferred_namespaces
            .iter()
            .position(|namespace| *namespace == tool_namespace(tool.name))
            .unwrap_or(usize::MAX);
        (bootstrap_rank, tier_rank, namespace_rank, *index)
    });
    visible.into_iter().map(|(_, tool)| tool).collect()
}

pub(crate) fn visible_namespaces(surface: ToolSurface) -> Vec<&'static str> {
    raw_visible_namespaces(surface)
}
