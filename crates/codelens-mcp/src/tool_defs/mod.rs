//! MCP tool definitions and preset filtering.

mod build;
mod generated;
mod output_schemas;
mod presets;
pub mod tool;

// Re-exports from presets
pub(crate) use presets::{
    ALL_PRESETS, ALL_PROFILES, HostContext, SurfaceCompilerInput, TaskOverlay, ToolPreset,
    ToolProfile, ToolSurface, compile_surface_overlay, default_budget_for_preset,
    default_budget_for_profile, deprecated_workflow_alias, is_tool_callable_in_surface,
    is_tool_in_surface, tool_anthropic_always_load, tool_anthropic_search_hint, tool_deprecation,
    tool_namespace, tool_phase_label, tool_preferred_executor, tool_preferred_executor_label,
};

// Re-exports from build
pub(crate) use build::{tool_definition, tools};

use crate::protocol::ToolTier;

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
            "cleanup_duplicate_logic",
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

/// First consumer of the `phase` tool alias introduced in v1.9.39/40.
/// Each profile declares which phase(s) its visible surface is intended
/// to serve. The mapping is advisory — `tools/list {"phase": ...}` still
/// works as a standalone filter — but it lets hosts that already select
/// a profile (the common case) get phase-scoped ordering guidance
/// without issuing an extra list call per phase.
pub(crate) fn preferred_phase_labels(surface: ToolSurface) -> Vec<&'static str> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => vec!["plan", "review"],
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => vec!["build", "review"],
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => vec!["review", "eval"],
        ToolSurface::Profile(ToolProfile::EvaluatorCompact) => vec!["eval"],
        ToolSurface::Profile(ToolProfile::RefactorFull) => vec!["build", "review"],
        ToolSurface::Profile(ToolProfile::CiAudit) => vec!["review", "eval"],
        // workflow-first is deliberately phase-agnostic — it exposes
        // the short high-value workflow surface to every phase.
        ToolSurface::Profile(ToolProfile::WorkflowFirst) => vec![],
        // Presets are version-legacy concepts; phase shaping is a
        // profile-layer decision.
        ToolSurface::Preset(_) => vec![],
    }
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
