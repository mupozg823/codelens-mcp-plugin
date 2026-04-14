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

pub(crate) const DEPRECATED_WORKFLOW_ALIAS_REMOVAL_TARGET: &str = "v1.10.0";

pub(crate) fn deprecated_workflow_replacement(name: &str) -> Option<&'static str> {
    match name {
        "audit_security_context" => Some("semantic_code_review"),
        "analyze_change_impact" => Some("impact_report"),
        "assess_change_readiness" => Some("verify_change_readiness"),
        _ => None,
    }
}

pub(crate) fn canonical_tool_name(name: &str) -> &str {
    deprecated_workflow_replacement(name).unwrap_or(name)
}

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

pub(crate) fn is_deferred_control_tool(name: &str) -> bool {
    matches!(
        name,
        "activate_project"
            | "prepare_harness_session"
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
            vec!["reports", "symbols", "filesystem", "session"]
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
        ToolSurface::Profile(ToolProfile::WorkflowFirst) => vec!["workflow", "session"],
        ToolSurface::Preset(ToolPreset::Minimal) => vec!["symbols", "filesystem", "mutation"],
        ToolSurface::Preset(ToolPreset::Balanced) => {
            vec!["reports", "symbols", "graph", "filesystem", "session"]
        }
        ToolSurface::Preset(ToolPreset::Full) => raw_visible_namespaces(surface),
    }
}

pub(crate) fn preferred_bootstrap_tools(surface: ToolSurface) -> Option<&'static [&'static str]> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => Some(&[
            "explore_codebase",
            "review_architecture",
            "review_changes",
            "prepare_harness_session",
        ]),
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => Some(&[
            "explore_codebase",
            "trace_request_path",
            "plan_safe_refactor",
            "prepare_harness_session",
        ]),
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => Some(&[
            "review_architecture",
            "review_changes",
            "cleanup_duplicate_logic",
            "prepare_harness_session",
        ]),
        // Keep refactor bootstrap preview-first. Mutation and broader report tools
        // are still reachable after an explicit expansion or follow-up step.
        ToolSurface::Profile(ToolProfile::RefactorFull) => Some(&[
            "plan_safe_refactor",
            "review_changes",
            "trace_request_path",
            "prepare_harness_session",
        ]),
        ToolSurface::Profile(ToolProfile::CiAudit) => Some(&[
            "review_changes",
            "semantic_code_review",
            "review_architecture",
            "prepare_harness_session",
        ]),
        ToolSurface::Preset(ToolPreset::Balanced) => Some(&[
            "explore_codebase",
            "review_architecture",
            "review_changes",
            "prepare_harness_session",
        ]),
        ToolSurface::Preset(ToolPreset::Full) => Some(&[
            "explore_codebase",
            "review_architecture",
            "plan_safe_refactor",
            "prepare_harness_session",
        ]),
        _ => None,
    }
}

pub(crate) fn preferred_tiers(surface: ToolSurface) -> Vec<ToolTier> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly)
        | ToolSurface::Profile(ToolProfile::ReviewerGraph)
        | ToolSurface::Profile(ToolProfile::RefactorFull)
        | ToolSurface::Profile(ToolProfile::CiAudit)
        | ToolSurface::Profile(ToolProfile::WorkflowFirst) => vec![ToolTier::Workflow],
        ToolSurface::Profile(ToolProfile::EvaluatorCompact) => {
            vec![ToolTier::Primitive, ToolTier::Analysis]
        }
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => {
            vec![ToolTier::Workflow, ToolTier::Analysis, ToolTier::Primitive]
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
    let mut tiers = raw_visible_tools(surface)
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
