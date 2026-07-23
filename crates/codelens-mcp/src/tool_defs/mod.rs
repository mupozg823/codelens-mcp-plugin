//! MCP tool definitions and preset filtering.

mod build;
mod generated;
mod output_schemas;
mod presets;
pub mod tool;
mod tool_selection;
mod visibility;

// Re-exports from presets
pub(crate) use presets::{
    ALL_PRESETS, ALL_PROFILES, AgentRole, HostContext, SurfaceCompilerInput, TaskOverlay,
    ToolPreset, ToolProfile, ToolSurface, apply_tool_deprecation_meta,
    compile_surface_overlay_for_agent, default_budget_for_preset, default_budget_for_profile,
    default_listed_tool_names, deprecated_workflow_alias, is_tool_in_surface,
    tool_anthropic_always_load, tool_anthropic_search_hint, tool_deprecation,
    tool_execution_policy, tool_execution_policy_payload, tool_feature_gate, tool_namespace,
    tool_phase_label, whitelist_preset_member_union,
};
// ADR-0016 decoupled runtime callability from listing, leaving
// `is_tool_callable_in_surface` a test-only doc/overlay-integrity helper.
#[cfg(test)]
pub(crate) use presets::is_tool_callable_in_surface;

// Re-exports from build
#[cfg(feature = "http")]
pub(crate) use build::parse_tier_label;
pub(crate) use build::{tool_definition, tool_tier, tool_tier_label, tools};
pub(crate) use tool_selection::{
    parse_tool_selection_requests, tool_name_requests, tool_request_omissions,
    tool_selection_diagnostics,
};
pub(crate) use visibility::{visible_namespaces, visible_tools};

use crate::protocol::ToolTier;

fn raw_visible_namespaces(surface: ToolSurface) -> Vec<&'static str> {
    let mut namespaces = tools()
        .iter()
        .filter(|tool| is_tool_in_surface(tool.name, surface))
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
            // "graph" added for the graph verb (Phase-2 bootstrap slice).
            vec!["reports", "symbols", "graph", "filesystem", "session"]
        }
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => {
            vec!["reports", "graph", "symbols", "session"]
        }
        ToolSurface::Profile(ToolProfile::RefactorFull) => {
            // "symbols"/"graph" added for the search/graph verbs.
            vec!["reports", "symbols", "graph", "session"]
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

// Phase-2 verb consolidation: profile bootstrap slices route through the
// verb facades (overview/search/graph/review/diagnose) — the absorbed
// workflow aliases (explore_codebase, review_architecture, review_changes,
// trace_request_path) stay callable but yield their bootstrap slots.
pub(crate) fn preferred_bootstrap_tools(surface: ToolSurface) -> Option<&'static [&'static str]> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => Some(&[
            "overview",
            "search",
            "graph",
            "review",
            "prepare_harness_session",
        ]),
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => Some(&[
            "overview",
            "search",
            "graph",
            "review",
            "diagnose",
            "plan_safe_refactor",
            "prepare_harness_session",
            "analyze_change_request",
        ]),
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => Some(&[
            "review",
            "graph",
            "diagnose",
            "cleanup_duplicate_logic",
            "prepare_harness_session",
        ]),
        // Keep refactor bootstrap preview-first. Mutation and broader report tools
        // are still reachable after an explicit expansion or follow-up step.
        ToolSurface::Profile(ToolProfile::RefactorFull) => Some(&[
            "plan_safe_refactor",
            "review",
            "graph",
            "search",
            "prepare_harness_session",
        ]),
        ToolSurface::Profile(ToolProfile::CiAudit) => Some(&[
            "review",
            "diagnose",
            "cleanup_duplicate_logic",
            "prepare_harness_session",
        ]),
        ToolSurface::Preset(ToolPreset::Balanced) => Some(&[
            "overview",
            "search",
            "graph",
            "review",
            "prepare_harness_session",
        ]),
        ToolSurface::Preset(ToolPreset::Full) => Some(&[
            "overview",
            "search",
            "review",
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
    // Normalize deprecated profiles to their canonical core equivalent.
    let surface = match surface {
        ToolSurface::Profile(p) if p.is_deprecated() => ToolSurface::Profile(p.canonical()),
        other => other,
    };
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => vec!["plan", "review"],
        ToolSurface::Profile(ToolProfile::BuilderMinimal) => vec!["build", "review"],
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => vec!["review", "eval"],
        // Presets are version-legacy concepts; phase shaping is a
        // profile-layer decision.
        ToolSurface::Preset(_) => vec![],
        // Deprecated profiles canonicalize above; this arm is unreachable
        // after normalization but required for exhaustiveness.
        _ => vec![],
    }
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

/// Ranked bootstrap slice membership (`default_visible_rank` in
/// tools.toml). Crate-visible wrapper over the generated predicate so
/// the dispatch access gates can honor the "advertised = callable"
/// contract for the default tools/list.
pub(crate) fn tool_default_listed(name: &str) -> bool {
    generated::tool_default_listed(name)
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
    generated::tool_is_content_mutation(name)
}

/// Whether a resolved handler must return a payload from one committed symbol
/// index generation. The predicate is generated from `tools.toml`; verb
/// facades inherit the contract of their resolved target.
pub(crate) fn tool_symbol_generation_consistent(name: &str) -> bool {
    generated::tool_symbol_generation_consistent(name)
}

pub(crate) fn experimental_feature_for_tool(name: &str) -> Option<&'static str> {
    generated::tool_experimental_feature(name)
}

pub(crate) fn experimental_tool_enabled(name: &str) -> bool {
    let Some(feature) = experimental_feature_for_tool(name) else {
        return true;
    };
    if cfg!(test) {
        return true;
    }
    std::env::var("CODELENS_EXPERIMENTAL_FEATURES")
        .ok()
        .is_some_and(|configured| {
            configured
                .split(',')
                .map(str::trim)
                .any(|value| value == feature || value == "all")
        })
}
