//! MCP tool definitions and preset filtering.

mod build;
mod output_schemas;
mod presets;

// Re-exports from presets
pub(crate) use presets::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
    is_tool_in_surface, tool_namespace,
};

// Re-exports from build
pub(crate) use build::{tool_definition, tools};

use crate::protocol::ToolTier;

pub(crate) fn visible_tools(surface: ToolSurface) -> Vec<&'static crate::protocol::Tool> {
    tools()
        .iter()
        .filter(|tool| is_tool_in_surface(tool.name, surface))
        .collect()
}

pub(crate) fn visible_namespaces(surface: ToolSurface) -> Vec<&'static str> {
    let mut namespaces = visible_tools(surface)
        .into_iter()
        .map(|tool| tool_namespace(tool.name))
        .collect::<Vec<_>>();
    namespaces.sort_unstable();
    namespaces.dedup();
    namespaces
}

pub(crate) fn is_deferred_control_tool(name: &str) -> bool {
    matches!(
        name,
        "activate_project"
            | "get_current_config"
            | "get_capabilities"
            | "set_profile"
            | "set_preset"
    )
}

pub(crate) fn preferred_namespaces(surface: ToolSurface) -> Vec<&'static str> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => {
            vec!["reports", "symbols", "graph", "session"]
        }
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => {
            vec!["symbols", "filesystem", "session"]
        }
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => {
            vec!["reports", "graph", "symbols", "session"]
        }
        ToolSurface::Profile(ToolProfile::RefactorFull) => {
            vec!["reports", "session"]
        }
        ToolSurface::Profile(ToolProfile::CiAudit) => {
            vec!["reports", "graph", "session"]
        }
        ToolSurface::Profile(ToolProfile::EvaluatorCompact) => {
            vec!["reports", "symbols", "lsp", "session"]
        }
        ToolSurface::Preset(ToolPreset::Minimal) => vec!["symbols", "filesystem", "mutation"],
        ToolSurface::Preset(ToolPreset::Balanced) => {
            vec!["reports", "symbols", "graph", "filesystem", "session"]
        }
        ToolSurface::Preset(ToolPreset::Full) => visible_namespaces(surface),
    }
}

pub(crate) fn preferred_bootstrap_tools(surface: ToolSurface) -> Option<&'static [&'static str]> {
    match surface {
        // Keep refactor bootstrap preview-first. Mutation and broader report tools
        // are still reachable after an explicit expansion or follow-up step.
        ToolSurface::Profile(ToolProfile::RefactorFull) => Some(&[
            "verify_change_readiness",
            "safe_rename_report",
            "refactor_safety_report",
            "start_analysis_job",
        ]),
        _ => None,
    }
}

pub(crate) fn preferred_tiers(surface: ToolSurface) -> Vec<ToolTier> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly)
        | ToolSurface::Profile(ToolProfile::ReviewerGraph)
        | ToolSurface::Profile(ToolProfile::RefactorFull)
        | ToolSurface::Profile(ToolProfile::CiAudit) => vec![ToolTier::Workflow],
        ToolSurface::Profile(ToolProfile::EvaluatorCompact) => {
            vec![ToolTier::Primitive, ToolTier::Analysis]
        }
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => {
            vec![ToolTier::Analysis, ToolTier::Primitive]
        }
        ToolSurface::Preset(ToolPreset::Minimal) => vec![ToolTier::Primitive, ToolTier::Analysis],
        ToolSurface::Preset(ToolPreset::Balanced) => vec![ToolTier::Workflow, ToolTier::Analysis],
        ToolSurface::Preset(ToolPreset::Full) => {
            vec![ToolTier::Workflow, ToolTier::Analysis, ToolTier::Primitive]
        }
    }
}

pub(crate) fn preferred_tier_labels(surface: ToolSurface) -> Vec<&'static str> {
    preferred_tiers(surface)
        .into_iter()
        .map(|tier| match tier {
            ToolTier::Primitive => "primitive",
            ToolTier::Analysis => "analysis",
            ToolTier::Workflow => "workflow",
        })
        .collect()
}

pub(crate) fn visible_tiers(surface: ToolSurface) -> Vec<&'static str> {
    let mut tiers = visible_tools(surface)
        .into_iter()
        .map(|tool| tool_tier_label(tool.name))
        .collect::<Vec<_>>();
    tiers.sort_unstable();
    tiers.dedup();
    tiers
}

pub(crate) fn tool_tier(name: &str) -> ToolTier {
    tool_definition(name)
        .and_then(|tool| tool.annotations.as_ref())
        .and_then(|annotations| annotations.tier)
        .unwrap_or(ToolTier::Primitive)
}

pub(crate) fn tool_tier_label(name: &str) -> &'static str {
    match tool_tier(name) {
        ToolTier::Primitive => "primitive",
        ToolTier::Analysis => "analysis",
        ToolTier::Workflow => "workflow",
    }
}

#[cfg(feature = "http")]
pub(crate) fn parse_tier_label(value: &str) -> Option<ToolTier> {
    match value {
        "primitive" => Some(ToolTier::Primitive),
        "analysis" => Some(ToolTier::Analysis),
        "workflow" => Some(ToolTier::Workflow),
        _ => None,
    }
}

pub(crate) fn is_read_only_surface(surface: ToolSurface) -> bool {
    matches!(
        surface,
        ToolSurface::Profile(ToolProfile::PlannerReadonly)
            | ToolSurface::Profile(ToolProfile::ReviewerGraph)
            | ToolSurface::Profile(ToolProfile::CiAudit)
            | ToolSurface::Profile(ToolProfile::EvaluatorCompact)
    )
}

pub(crate) fn is_content_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "replace_symbol_body"
            | "delete_lines"
            | "insert_at_line"
            | "insert_before_symbol"
            | "insert_after_symbol"
            | "insert_content"
            | "replace_content"
            | "replace_lines"
            | "replace"
            | "rename_symbol"
            | "create_text_file"
            | "add_import"
            | "write_memory"
            | "delete_memory"
            | "rename_memory"
            | "add_queryable_project"
            | "remove_queryable_project"
            | "refactor_extract_function"
            | "refactor_inline_function"
            | "refactor_move_to_file"
            | "refactor_change_signature"
    )
}
