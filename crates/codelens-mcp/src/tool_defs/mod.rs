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

const WORKFLOW_ONLY_TIERS: &[ToolTier] = &[ToolTier::Workflow];
const WORKFLOW_THEN_ANALYSIS_TIERS: &[ToolTier] = &[ToolTier::Workflow, ToolTier::Analysis];
const WORKFLOW_ANALYSIS_PRIMITIVE_TIERS: &[ToolTier] =
    &[ToolTier::Workflow, ToolTier::Analysis, ToolTier::Primitive];
const PRIMITIVE_ANALYSIS_TIERS: &[ToolTier] = &[ToolTier::Primitive, ToolTier::Analysis];

const PLANNER_NAMESPACES: &[&str] = &["reports", "symbols", "graph", "session"];
const BUILDER_NAMESPACES: &[&str] = &["reports", "symbols", "filesystem", "session"];
const REVIEWER_NAMESPACES: &[&str] = &["reports", "graph", "symbols", "session"];
const REFACTOR_NAMESPACES: &[&str] = &["reports", "session"];
const CI_AUDIT_NAMESPACES: &[&str] = &["reports", "graph", "session"];
const EVALUATOR_NAMESPACES: &[&str] = &["reports", "symbols", "lsp", "session"];
const WORKFLOW_FIRST_NAMESPACES: &[&str] = &["workflow", "session"];
const MINIMAL_NAMESPACES: &[&str] = &["symbols", "filesystem", "mutation"];
const BALANCED_NAMESPACES: &[&str] = &["reports", "symbols", "graph", "filesystem", "session"];

const PLANNER_BOOTSTRAP: &[&str] = &[
    "explore_codebase",
    "review_architecture",
    "review_changes",
    "prepare_harness_session",
];
const BUILDER_BOOTSTRAP: &[&str] = &[
    "explore_codebase",
    "trace_request_path",
    "plan_safe_refactor",
    "prepare_harness_session",
];
const REVIEWER_BOOTSTRAP: &[&str] = &[
    "review_architecture",
    "review_changes",
    "cleanup_duplicate_logic",
    "prepare_harness_session",
];
const REFACTOR_BOOTSTRAP: &[&str] = &[
    "plan_safe_refactor",
    "review_changes",
    "trace_request_path",
    "prepare_harness_session",
];
const CI_AUDIT_BOOTSTRAP: &[&str] = &[
    "review_changes",
    "semantic_code_review",
    "review_architecture",
    "prepare_harness_session",
];
const BALANCED_BOOTSTRAP: &[&str] = &[
    "explore_codebase",
    "review_architecture",
    "review_changes",
    "prepare_harness_session",
];
const FULL_BOOTSTRAP: &[&str] = &[
    "explore_codebase",
    "review_architecture",
    "plan_safe_refactor",
    "prepare_harness_session",
];

#[derive(Clone, Copy)]
struct SurfacePreferenceSpec {
    surface: ToolSurface,
    preferred_namespaces: Option<&'static [&'static str]>,
    preferred_bootstrap: Option<&'static [&'static str]>,
    preferred_tiers: &'static [ToolTier],
}

const SURFACE_PREFERENCE_SPECS: &[SurfacePreferenceSpec] = &[
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::PlannerReadonly),
        preferred_namespaces: Some(PLANNER_NAMESPACES),
        preferred_bootstrap: Some(PLANNER_BOOTSTRAP),
        preferred_tiers: WORKFLOW_ONLY_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::BuilderMinimal),
        preferred_namespaces: Some(BUILDER_NAMESPACES),
        preferred_bootstrap: Some(BUILDER_BOOTSTRAP),
        preferred_tiers: WORKFLOW_ANALYSIS_PRIMITIVE_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::ReviewerGraph),
        preferred_namespaces: Some(REVIEWER_NAMESPACES),
        preferred_bootstrap: Some(REVIEWER_BOOTSTRAP),
        preferred_tiers: WORKFLOW_ONLY_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::RefactorFull),
        preferred_namespaces: Some(REFACTOR_NAMESPACES),
        preferred_bootstrap: Some(REFACTOR_BOOTSTRAP),
        preferred_tiers: WORKFLOW_ONLY_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::CiAudit),
        preferred_namespaces: Some(CI_AUDIT_NAMESPACES),
        preferred_bootstrap: Some(CI_AUDIT_BOOTSTRAP),
        preferred_tiers: WORKFLOW_ONLY_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::EvaluatorCompact),
        preferred_namespaces: Some(EVALUATOR_NAMESPACES),
        preferred_bootstrap: None,
        preferred_tiers: PRIMITIVE_ANALYSIS_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Profile(ToolProfile::WorkflowFirst),
        preferred_namespaces: Some(WORKFLOW_FIRST_NAMESPACES),
        preferred_bootstrap: None,
        preferred_tiers: WORKFLOW_ONLY_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Preset(ToolPreset::Minimal),
        preferred_namespaces: Some(MINIMAL_NAMESPACES),
        preferred_bootstrap: None,
        preferred_tiers: PRIMITIVE_ANALYSIS_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Preset(ToolPreset::Balanced),
        preferred_namespaces: Some(BALANCED_NAMESPACES),
        preferred_bootstrap: Some(BALANCED_BOOTSTRAP),
        preferred_tiers: WORKFLOW_THEN_ANALYSIS_TIERS,
    },
    SurfacePreferenceSpec {
        surface: ToolSurface::Preset(ToolPreset::Full),
        preferred_namespaces: None,
        preferred_bootstrap: Some(FULL_BOOTSTRAP),
        preferred_tiers: WORKFLOW_ANALYSIS_PRIMITIVE_TIERS,
    },
];

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

fn surface_preferences(surface: ToolSurface) -> Option<&'static SurfacePreferenceSpec> {
    SURFACE_PREFERENCE_SPECS
        .iter()
        .find(|spec| spec.surface == surface)
}

pub(crate) fn preferred_namespaces(surface: ToolSurface) -> Vec<&'static str> {
    surface_preferences(surface)
        .and_then(|spec| spec.preferred_namespaces)
        .map(|namespaces| namespaces.to_vec())
        .unwrap_or_else(|| raw_visible_namespaces(surface))
}

pub(crate) fn preferred_bootstrap_tools(surface: ToolSurface) -> Option<&'static [&'static str]> {
    surface_preferences(surface).and_then(|spec| spec.preferred_bootstrap)
}

pub(crate) fn preferred_tiers(surface: ToolSurface) -> Vec<ToolTier> {
    surface_preferences(surface)
        .map(|spec| spec.preferred_tiers.to_vec())
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_preference_specs_preserve_bootstrap_priority() {
        assert_eq!(
            preferred_bootstrap_tools(ToolSurface::Profile(ToolProfile::BuilderMinimal)),
            Some(BUILDER_BOOTSTRAP)
        );
        assert_eq!(
            preferred_bootstrap_tools(ToolSurface::Profile(ToolProfile::ReviewerGraph)),
            Some(REVIEWER_BOOTSTRAP)
        );
        assert_eq!(
            preferred_bootstrap_tools(ToolSurface::Profile(ToolProfile::WorkflowFirst)),
            None
        );
    }

    #[test]
    fn surface_preference_specs_preserve_namespace_preferences() {
        assert_eq!(
            preferred_namespaces(ToolSurface::Profile(ToolProfile::CiAudit)),
            CI_AUDIT_NAMESPACES
        );
        assert_eq!(
            preferred_namespaces(ToolSurface::Preset(ToolPreset::Balanced)),
            BALANCED_NAMESPACES
        );
    }

    #[test]
    fn full_surface_keeps_dynamic_namespace_fallback() {
        let full_namespaces = preferred_namespaces(ToolSurface::Preset(ToolPreset::Full));
        assert!(full_namespaces.contains(&"reports"));
        assert!(full_namespaces.contains(&"session"));
        assert!(full_namespaces.contains(&"symbols"));
    }
}
