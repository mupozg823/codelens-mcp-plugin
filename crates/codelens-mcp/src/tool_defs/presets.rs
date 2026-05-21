//! Tool presets, profiles, surfaces, and their filtering logic.

mod budgets;
mod metadata;
mod overlay;

pub(crate) use budgets::{default_budget_for_preset, default_budget_for_profile};
pub(crate) use metadata::{
    apply_tool_deprecation_meta, deprecated_workflow_alias, tool_anthropic_always_load,
    tool_anthropic_search_hint, tool_deprecation, tool_namespace, tool_phase_label,
    tool_preferred_executor, tool_preferred_executor_label,
};
pub(crate) use overlay::{HostContext, SurfaceCompilerInput, TaskOverlay, compile_surface_overlay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPreset {
    Minimal,  // core tools — symbol/file/search + safe edits
    Balanced, // default — excludes niche analysis + built-in overlaps
    Full,     // all tools
}

pub(crate) const ALL_PRESETS: &[ToolPreset] =
    &[ToolPreset::Minimal, ToolPreset::Balanced, ToolPreset::Full];

impl ToolPreset {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "minimal" | "min" => Self::Minimal,
            "balanced" | "bal" => Self::Balanced,
            _ => Self::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolProfile {
    PlannerReadonly,
    BuilderMinimal,
    ReviewerGraph,
    EvaluatorCompact,
    RefactorFull,
    CiAudit,
    WorkflowFirst,
}

pub(crate) const ALL_PROFILES: &[ToolProfile] = &[
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
    ToolProfile::WorkflowFirst,
];

impl ToolProfile {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "planner-readonly" | "planner" => Some(Self::PlannerReadonly),
            "builder-minimal" | "builder" => Some(Self::BuilderMinimal),
            "reviewer-graph" | "reviewer" => Some(Self::ReviewerGraph),
            "refactor-full" | "refactor" => Some(Self::RefactorFull),
            "evaluator-compact" | "evaluator" => Some(Self::EvaluatorCompact),
            "ci-audit" | "ci" => Some(Self::CiAudit),
            "workflow-first" | "workflow" => Some(Self::WorkflowFirst),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlannerReadonly => "planner-readonly",
            Self::BuilderMinimal => "builder-minimal",
            Self::ReviewerGraph => "reviewer-graph",
            Self::RefactorFull => "refactor-full",
            Self::EvaluatorCompact => "evaluator-compact",
            Self::CiAudit => "ci-audit",
            Self::WorkflowFirst => "workflow-first",
        }
    }

    /// Profiles slated for removal in v2.0. The enum entries stay so existing
    /// `from_str` callers and host overlays keep working through the
    /// deprecation window; surface_manifest exposes the marker so hosts can
    /// stop advertising them ahead of the cut.
    ///
    /// Rationale: the four below are either redundant aliases of the core
    /// trio (planner/builder/reviewer) or were aspirational profiles that
    /// never accumulated a distinct toolset. Keeping seven entries means
    /// the surface routing matrix has more cells to maintain than realised
    /// behaviour change. v2.0 collapses to the core trio.
    pub fn is_deprecated(&self) -> bool {
        matches!(
            self,
            Self::EvaluatorCompact | Self::RefactorFull | Self::CiAudit | Self::WorkflowFirst
        )
    }

    /// Removal target version for deprecated profiles. None for active ones.
    pub fn deprecation_target(&self) -> Option<&'static str> {
        if self.is_deprecated() {
            Some("v2.0")
        } else {
            None
        }
    }

    /// Resolve deprecated profiles to their canonical core equivalent.
    /// Active profiles return themselves; deprecated ones redirect:
    ///   EvaluatorCompact → PlannerReadonly
    ///   RefactorFull     → BuilderMinimal
    ///   CiAudit          → ReviewerGraph
    ///   WorkflowFirst    → PlannerReadonly
    ///
    /// All profile-sensitive logic (tool filtering, budgets, suggestions,
    /// overlays) should use this so deprecated names remain parseable
    /// but behave identically to the core trio.
    pub fn canonical(self) -> Self {
        match self {
            Self::EvaluatorCompact => Self::PlannerReadonly,
            Self::RefactorFull => Self::BuilderMinimal,
            Self::CiAudit => Self::ReviewerGraph,
            Self::WorkflowFirst => Self::PlannerReadonly,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolSurface {
    Preset(ToolPreset),
    Profile(ToolProfile),
}

impl ToolSurface {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Preset(ToolPreset::Minimal) => "preset:minimal",
            Self::Preset(ToolPreset::Balanced) => "preset:balanced",
            Self::Preset(ToolPreset::Full) => "preset:full",
            Self::Profile(profile) => profile.as_str(),
        }
    }
}

pub(crate) const MINIMAL_TOOLS: &[&str] = &[
    "activate_project",
    "prepare_harness_session",
    "register_agent_work",
    "list_active_agents",
    "claim_files",
    "release_files",
    "get_current_config",
    "set_preset",
    "set_profile",
    // File (kept for non-Claude-Code clients)
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    // Symbol (core)
    "get_symbols_overview",
    "find_symbol",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_type_hierarchy",
    // `refresh_symbol_index` removed from Minimal (b07d5773 dogfood):
    // `find_over_visible_apis` flagged it as leakage — the annotation is
    // `approval_required=true` + `audit_category="mutation"`, but the
    // Minimal preset promises read-only safety. The tool remains in
    // `BUILDER_MINIMAL_TOOLS` and `REVIEWER_GRAPH_TOOLS` (line 291) where
    // mutation surface is expected. Callers that need it from a minimal
    // surface should `set_preset full` first.
    "get_file_diagnostics",
    "search_workspace_symbols",
    // Mutation (safe)
    "plan_symbol_rename",
    "rename_symbol",
    "replace_symbol_body",
    "insert_content",
    "create_text_file",
    "replace",
];

pub(crate) const BALANCED_EXCLUDES: &[&str] = &[
    // ── Niche analysis (use Full preset for these) ──
    "find_circular_dependencies",
    "get_symbol_importance",
    "find_dead_code",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
    "get_complexity",
    "search_symbols_fuzzy",
    "get_lsp_recipe",
    // ── Overlap with Claude Code built-in tools ──
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    // ── Diagnostics / session (not needed for normal work) ──
    "get_watch_status",
    "prune_index_failures",
    "get_tool_metrics",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
    // ── Superseded by unified tools (insert_content, replace) ──
    "insert_at_line",
    "insert_before_symbol",
    "insert_after_symbol",
    "replace_lines",
    // ── Superseded by onboard_project ──
    "get_project_structure",
    // ── Deprecated workflow aliases (keep direct-call compat only) ──
    "audit_security_context",
    "analyze_change_impact",
    "assess_change_readiness",
];

pub(crate) const PLANNER_READONLY_TOOLS: &[&str] = &[
    // Session
    "activate_project",
    "prepare_harness_session",
    "register_agent_work",
    "list_active_agents",
    "claim_files",
    "release_files",
    "get_current_config",
    "get_capabilities",
    "set_profile",
    "set_preset",
    "get_tool_metrics",
    "audit_builder_session",
    "audit_planner_session",
    // Workflow-first entrypoints
    "explore_codebase",
    "review_architecture",
    "plan_safe_refactor",
    "review_changes",
    "diagnose_issues",
    // Symbol exploration
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    // Phase 4a §capability-reporting: semantic_search belongs in
    // planner surface. Planners are read-only/exploratory — natural-
    // language search is the primary use case, and the engine now
    // lazy-initializes on first call so there is no startup cost.
    // `index_embeddings` is exposed alongside so planners whose
    // project lacks an on-disk index can remediate directly.
    "semantic_search",
    "index_embeddings",
    // Graph / impact
    "get_impact_analysis",
    "get_changed_files",
    "onboard_project",
    // Workflow composites
    "orchestrate_change",
    "analyze_change_request",
    "verify_change_readiness",
    "impact_report",
    "mermaid_module_graph",
    // Async analysis
    "start_analysis_job",
    "get_analysis_job",
    "get_analysis_section",
];

pub(crate) const BUILDER_MINIMAL_TOOLS: &[&str] = &[
    "activate_project",
    "prepare_harness_session",
    "register_agent_work",
    "list_active_agents",
    "claim_files",
    "release_files",
    "get_current_config",
    "get_capabilities",
    "set_profile",
    "set_preset",
    "get_tool_metrics",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
    "explore_codebase",
    "trace_request_path",
    "plan_safe_refactor",
    "cleanup_duplicate_logic",
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_file_diagnostics",
    "find_tests",
    "refresh_symbol_index",
    "get_callers",
    "get_callees",
    // Phase 4a §capability-reporting: builders occasionally need NL
    // lookups ("where is the error handler for invalid credentials?"
    // type questions during mid-edit debugging). Exposing
    // `semantic_search` + `index_embeddings` keeps the builder
    // surface aligned with planner surface and removes the
    // "surface policy blocks a healthy feature" reporting mismatch.
    "semantic_search",
    "index_embeddings",
    "plan_symbol_rename",
    // Deprecated mutation tools (still in dispatch, will be removed in v2.0)
    "rename_symbol",
    "replace_symbol_body",
    "insert_content",
    "replace",
    "create_text_file",
    "add_import",
    // Workflow orchestration (deprecated, kept for backward compat)
    "orchestrate_change",
    "analyze_change_request",
    "verify_change_readiness",
];

pub(crate) const REVIEWER_GRAPH_TOOLS: &[&str] = &[
    // Session
    "activate_project",
    "prepare_harness_session",
    "register_agent_work",
    "list_active_agents",
    "claim_files",
    "release_files",
    "get_current_config",
    "set_profile",
    "set_preset",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
    // Workflow-first entrypoints
    "review_architecture",
    "cleanup_duplicate_logic",
    "review_changes",
    "diagnose_issues",
    // Symbol exploration
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    "find_scoped_references",
    // Reviewer sessions need NL retrieval for architecture and risk
    // evidence. Deferred loading still keeps it out of the initial
    // tool prompt unless the host loads the symbols namespace/tier.
    "semantic_search",
    "index_embeddings",
    // Diagnostics
    "get_file_diagnostics",
    // Graph / impact
    "get_callers",
    "get_callees",
    "get_impact_analysis",
    "get_changed_files",
    // Workflow composites
    "orchestrate_change",
    "analyze_change_request",
    "impact_report",
    "refactor_safety_report",
    "verify_change_readiness",
    "diff_aware_references",
    "module_boundary_report",
    "mermaid_module_graph",
    // Async analysis
    "start_analysis_job",
    "get_analysis_job",
    "get_analysis_section",
];

// ── Deprecated profile tool lists removed (v1.13.27 diet).
// EvaluatorCompact, RefactorFull, CiAudit, WorkflowFirst now resolve to
// their canonical core equivalents via `ToolProfile::canonical()`:
//   EvaluatorCompact → PlannerReadonly
//   RefactorFull     → BuilderMinimal
//   CiAudit          → ReviewerGraph
//   WorkflowFirst    → PlannerReadonly

// ── Filtering ──────────────────────────────────────────────────────────

/// Check if a tool belongs to a profile. Deprecated profiles resolve to
/// their canonical core equivalent, so the alias behaves identically.
pub(crate) fn is_tool_in_profile(name: &str, profile: ToolProfile) -> bool {
    match profile.canonical() {
        ToolProfile::PlannerReadonly => PLANNER_READONLY_TOOLS.contains(&name),
        ToolProfile::BuilderMinimal => BUILDER_MINIMAL_TOOLS.contains(&name),
        ToolProfile::ReviewerGraph => REVIEWER_GRAPH_TOOLS.contains(&name),
        // Unreachable: all deprecated variants canonicalize above.
        dep => unreachable!("canonical() should not return {dep:?}"),
    }
}

/// Union of all whitelist-style preset members. `BALANCED_EXCLUDES` is
/// intentionally excluded because it is a *deny*-list, not a membership
/// list — folding it in would make the result meaningless.
///
/// Returns sorted+deduplicated names. Used by
/// `audit_tool_surface_consistency` (admin tool, P1-4 Sprint A) to spot
/// preset members that no longer exist in `tools.toml`.
pub(crate) fn whitelist_preset_member_union() -> std::collections::BTreeSet<&'static str> {
    MINIMAL_TOOLS
        .iter()
        .copied()
        .chain(PLANNER_READONLY_TOOLS.iter().copied())
        .chain(BUILDER_MINIMAL_TOOLS.iter().copied())
        .chain(REVIEWER_GRAPH_TOOLS.iter().copied())
        .collect()
}

pub(crate) fn is_tool_in_surface(name: &str, surface: ToolSurface) -> bool {
    match surface {
        ToolSurface::Preset(preset) => is_tool_in_preset(name, preset),
        ToolSurface::Profile(profile) => is_tool_in_profile(name, profile),
    }
}

pub(crate) fn is_tool_callable_in_surface(name: &str, surface: ToolSurface) -> bool {
    is_tool_in_surface(name, surface)
        || deprecated_workflow_alias(name)
            .map(|(replacement, _)| is_tool_in_surface(replacement, surface))
            .unwrap_or(false)
}

/// Check if a tool is included in a given preset.
pub(crate) fn is_tool_in_preset(name: &str, preset: ToolPreset) -> bool {
    match preset {
        ToolPreset::Full => true,
        ToolPreset::Minimal => MINIMAL_TOOLS.contains(&name),
        ToolPreset::Balanced => !BALANCED_EXCLUDES.contains(&name),
    }
}

#[cfg(test)]
mod deprecation_tests {
    use super::*;

    #[test]
    fn core_trio_is_active() {
        for p in [
            ToolProfile::PlannerReadonly,
            ToolProfile::BuilderMinimal,
            ToolProfile::ReviewerGraph,
        ] {
            assert!(!p.is_deprecated(), "{:?} should be active", p);
            assert_eq!(p.deprecation_target(), None, "{:?}", p);
        }
    }

    #[test]
    fn four_profiles_marked_for_v2_removal() {
        for p in [
            ToolProfile::EvaluatorCompact,
            ToolProfile::RefactorFull,
            ToolProfile::CiAudit,
            ToolProfile::WorkflowFirst,
        ] {
            assert!(p.is_deprecated(), "{:?} should be deprecated", p);
            assert_eq!(p.deprecation_target(), Some("v2.0"));
        }
    }

    #[test]
    fn from_str_still_resolves_deprecated_aliases() {
        // Deprecation does not break parsing — host overlays may still
        // request these profiles through the v1.13 deprecation window.
        assert_eq!(
            ToolProfile::from_str("evaluator-compact"),
            Some(ToolProfile::EvaluatorCompact)
        );
        assert_eq!(
            ToolProfile::from_str("refactor"),
            Some(ToolProfile::RefactorFull)
        );
        assert_eq!(ToolProfile::from_str("ci"), Some(ToolProfile::CiAudit));
        assert_eq!(
            ToolProfile::from_str("workflow"),
            Some(ToolProfile::WorkflowFirst)
        );
    }
}
