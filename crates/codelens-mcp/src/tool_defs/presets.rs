//! Tool presets, profiles, surfaces, and their filtering logic.

mod budgets;
mod metadata;
mod overlay;

pub(crate) use budgets::{default_budget_for_preset, default_budget_for_profile};
pub(crate) use metadata::{
    apply_tool_deprecation_meta, default_listed_tool_names, deprecated_workflow_alias,
    tool_anthropic_always_load, tool_anthropic_search_hint, tool_deprecation,
    tool_execution_policy, tool_execution_policy_payload, tool_feature_gate, tool_namespace,
    tool_phase_label,
};
pub(crate) use overlay::{
    AgentRole, HostContext, SurfaceCompilerInput, TaskOverlay, compile_surface_overlay_for_agent,
};

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
];

impl ToolProfile {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "readonly" | "planner-readonly" | "planner" => Some(Self::PlannerReadonly),
            "builder" | "builder-minimal" => Some(Self::BuilderMinimal),
            "review" | "reviewer-graph" | "reviewer" => Some(Self::ReviewerGraph),
            "refactor-full" | "refactor" => Some(Self::RefactorFull),
            "evaluator-compact" | "evaluator" => Some(Self::EvaluatorCompact),
            "ci-audit" | "ci" => Some(Self::CiAudit),
            "workflow-first" | "workflow" => Some(Self::WorkflowFirst),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlannerReadonly | Self::EvaluatorCompact | Self::WorkflowFirst => "readonly",
            Self::BuilderMinimal | Self::RefactorFull => "builder",
            Self::ReviewerGraph | Self::CiAudit => "review",
        }
    }

    pub fn compatibility_alias(value: &str) -> Option<&'static str> {
        match value.to_ascii_lowercase().as_str() {
            "planner-readonly" | "planner" | "workflow-first" | "workflow"
            | "evaluator-compact" | "evaluator" => Some("readonly"),
            "builder-minimal" | "refactor-full" | "refactor" => Some("builder"),
            "reviewer-graph" | "reviewer" | "ci-audit" | "ci" => Some("review"),
            _ => None,
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

// ── ADR-0016 default surface (CORE-10 / CORE-20) ─────────────────────────
//
// These two constants are the declarative source-of-truth for the workflow-
// first default surface (ADR-0016 decisions 1 & 2). They are *not* a runtime
// filter — the live default `tools/list` is generated from `tools.toml`
// `default_visible_rank` (`default_listed_tool_names()`), and the lock tests
// below assert the two agree. The constants document the intended roster and
// give the reference-audit / surface-manifest tooling a single place to read
// the ADR composition.
//
// Relationship to the preset/profile surfaces: Minimal/Balanced/Full and the
// planner/builder/reviewer profiles gate the *expanded* (`full` / namespace /
// tier) listing and the callable surface. The default (non-expanded)
// `tools/list` is surface-independent — every built-in surface exposes exactly
// this CORE-20 roster (feature-gated members aside). See
// `resource_context::visible_tools::filter_default_listed_tools`.

/// ADR-0016 decision 1 — always-loaded core (10). Schemas are preloaded so the
/// model can call them without a ToolSearch round-trip.
pub(crate) const CORE_10_TOOLS: &[&str] = &[
    "prepare_harness_session",
    "search",
    "overview",
    "graph",
    "diagnose",
    "review",
    "plan_safe_refactor",
    "verify_change_readiness",
    "get_changed_files",
    "get_current_config",
];

/// ADR-0016 decision 2 — static default surface (≤ 20) = CORE-10 plus ten
/// precision/analysis entrypoints.
///
/// `semantic_search` is listed here as part of the declared roster, but it is
/// only *registered* when the `semantic` feature is compiled in. When the
/// feature is off the tool is absent from the registry, so the runtime
/// `tools/list` (`filter_default_listed_tools`) drops it and the effective
/// default surface is 19. The constant stays feature-independent so it matches
/// the equally feature-independent generated `default_listed_tool_names()`.
pub(crate) const CORE_20_TOOLS: &[&str] = &[
    // CORE-10 (ADR order)
    "prepare_harness_session",
    "search",
    "overview",
    "graph",
    "diagnose",
    "review",
    "plan_safe_refactor",
    "verify_change_readiness",
    "get_changed_files",
    "get_current_config",
    // CORE-20 extras
    "get_ranked_context",
    "find_symbol",
    "find_referencing_symbols",
    "semantic_search",
    "refresh_symbol_index",
    "get_watch_status",
    "start_analysis_job",
    "get_analysis_job",
    "get_analysis_section",
    "cancel_analysis_job",
];

// Compile-time guard for the ADR-0016 roster sizes (also keeps CORE_20_TOOLS
// load-bearing in non-test builds, where the full-composition checks in the
// `adr_0016_default_surface_tests` module are compiled out). The exact roster
// composition and its agreement with the generated `default_listed_tool_names`
// are locked by that test module.
const _: () = {
    assert!(
        CORE_10_TOOLS.len() == 10,
        "ADR-0016 CORE-10 must have 10 tools"
    );
    assert!(
        CORE_20_TOOLS.len() == 20,
        "ADR-0016 CORE-20 must have 20 tools"
    );
};

pub(crate) const MINIMAL_TOOLS: &[&str] = &[
    // Verb facades (Phase-1/2 read-only consolidation)
    "search",
    "graph",
    "review",
    "overview",
    "diagnose",
    "analyze",
    "activate_project",
    "prepare_harness_session",
    "get_current_config",
    "set_preset",
    "set_profile",
    // File (kept for non-Claude-Code clients)
    "read_file",
    "list_dir",
    "find_file",
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
    // Mutation preflight (the symbolic edit core itself is dispatch-only
    // pending the ADR-0009/D3 re-listing decision, #346)
    "plan_symbol_rename",
];

pub(crate) const BALANCED_EXCLUDES: &[&str] = &[
    // ── Niche analysis (use Full preset for these) ──
    "get_symbol_importance",
    "get_complexity",
    "search_symbols_fuzzy",
    "get_lsp_recipe",
    // ── Overlap with Claude Code built-in tools ──
    "read_file",
    "list_dir",
    "find_file",
    // ── Diagnostics / session (not needed for normal work) ──
    "get_watch_status",
    "prune_index_failures",
    "get_tool_metrics",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
    // ── 2026-07 tool-surface diet, step 2: four host-owned subsystems
    //    (docs/operations/tool-surface-diet-2026-07.md "결정 확정", 2026-07-19).
    //    Non-destructive and reversible: tools.toml definitions + dispatch
    //    arms stay intact, so every one is still callable via `tools/call`
    //    under the Full preset (or after `set_preset full`); they are only
    //    dropped from the default listed surfaces. The paired
    //    `preset_tags = ["balanced-excluded"]` entries in tools.toml are kept
    //    in lockstep so `regen-tool-defs.py::validate_preset_tags` stays green.
    // Memory subsystem (host harness owns memory) — was preset_tags = []
    // (already off Minimal/planner/builder/reviewer), now also off Balanced.
    "list_memories",
    "read_memory",
    "write_memory",
    "delete_memory",
    "rename_memory",
    "archive_memory",
    "restore_memory",
    "list_archived",
    "read_policy",
    // Agent coordination (host harness owns multi-agent coordination) —
    // also removed from MINIMAL_TOOLS / PLANNER_READONLY_TOOLS /
    // BUILDER_MINIMAL_TOOLS above.
    "register_agent_work",
    "list_active_agents",
    "claim_files",
    "release_files",
];

pub(crate) const PLANNER_READONLY_TOOLS: &[&str] = &[
    // Verb facades (Phase-1/2 read-only consolidation)
    "search",
    "graph",
    "review",
    "overview",
    "diagnose",
    "analyze",
    // Session
    "activate_project",
    "prepare_harness_session",
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
    // #350: target of find_symbol / D1-trio fallback hints — must be
    // present wherever those emitters are, or the hint chain dead-ends.
    "bm25_symbol_search",
    // #350 / ADR-0016: `find_referencing_symbols` emits
    // `cross_file_callers_hint` → `get_callers`. The hint chain now resolves
    // through *callability*, not listing — get_callers/get_callees are
    // registered in tools.toml and stay dispatchable here as hidden aliases
    // (see dispatch/access.rs::is_tool_registered), so they no longer occupy a
    // listed slot on this read surface. They remain listed on builder-minimal.
    // D1 LSP read trio (#346 Phase 4) — degrade gracefully without LSP
    "find_declaration",
    "find_implementations",
    "get_diagnostics_for_symbol",
    // Phase 4a §capability-reporting: semantic_search belongs in
    // planner surface. Planners are read-only/exploratory — natural-
    // language search is the primary use case, and the engine now
    // lazy-initializes on first call so there is no startup cost.
    // `index_embeddings` is exposed alongside so planners whose
    // project lacks an on-disk index can remediate directly.
    "semantic_search",
    "embedding_coverage_report",
    "index_embeddings",
    // Graph / impact
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
    // Verb facades (Phase-1/2 read-only consolidation)
    "search",
    "graph",
    "review",
    "overview",
    "diagnose",
    "analyze",
    "activate_project",
    "prepare_harness_session",
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
    // #350: target of find_symbol / D1-trio fallback hints — must be
    // present wherever those emitters are, or the hint chain dead-ends.
    "bm25_symbol_search",
    "get_file_diagnostics",
    // D1 LSP read trio (#346 Phase 4) — degrade gracefully without LSP
    "find_declaration",
    "find_implementations",
    "get_diagnostics_for_symbol",
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
    "embedding_coverage_report",
    "index_embeddings",
    // Poll-handle coherence: index_embeddings / refresh_symbol_index
    // background responses direct the caller to get_analysis_job — a
    // surface that advertises the queueing tools must expose the poll
    // tool too (live-verified gap: builder sessions could queue but
    // not poll).
    "get_analysis_job",
    "plan_symbol_rename",
    // Pending-D3 symbolic edit core (#346): callable on builder surfaces
    // but schemaless (not in tools.toml) until the ADR-0009/D3 re-listing
    // decision — i.e. dispatchable yet absent from tools/list.
    "rename_symbol",
    "replace_symbol_body",
    "insert_before_symbol",
    "insert_after_symbol",
    // Workflow orchestration
    "orchestrate_change",
    "analyze_change_request",
    "verify_change_readiness",
];

// Curated default `review` surface (selected per session on :7838) — the core set from the
// 2026-07 tool-surface diet, step 1. Reduced from 49 → 20 to match the
// 14-day usage telemetry (docs/operations/tool-surface-diet-2026-07.md).
// ADR-0016 keeps this at ≤20: the #350 call-graph hint targets
// (get_callers/get_callees) resolve through hidden-alias callability rather
// than a listed slot, so the surface holds the core-20 cap.
//
// Reversible and non-destructive: the 33 tools dropped here are NOT
// deleted — their tools.toml definitions and dispatch arms stay intact,
// so every one remains callable via `tools/call`; they are simply no
// longer advertised on the default listed surface. The paired
// `preset_tags["reviewer-graph"]` entries in tools.toml are kept in
// lockstep so `regen-tool-defs.py::validate_preset_tags` stays green.
//
// Composition is locked by
// `reviewer_graph_core_surface_contains_alwaysload_and_verb_facades`:
//   - 5 canonical verb façades (search/graph are named directly by the
//     codelens-first hook deny message + rules/harness.md; the other
//     three complete the documented mode-routing façade family —
//     hiding any of them would break that guidance)
//   - 9 always-load entrypoints (v1.13.34 CHANGELOG)
//   - 6 change-safety / diagnostics tools kept by the usage +
//     "change safety" strategy axis
pub(crate) const REVIEWER_GRAPH_TOOLS: &[&str] = &[
    // Verb facades (canonical mode-routing entrypoints)
    "search",
    "graph",
    "overview",
    "diagnose",
    "review",
    // Always-load spine (bootstrap + precision ladder + change safety)
    "prepare_harness_session",
    "explore_codebase",
    "review_changes",
    "review_architecture",
    "verify_change_readiness",
    "find_symbol",
    "find_referencing_symbols",
    "get_symbols_overview",
    "get_ranked_context",
    // Change-safety + diagnostics core
    "get_file_diagnostics",
    "impact_report",
    "diff_aware_references",
    "safe_rename_report",
    // #350 / ADR-0016: `find_referencing_symbols` (above) emits
    // `cross_file_callers_hint` → `get_callers`. That hint no longer requires a
    // listed slot here: get_callers/get_callees are registered in tools.toml
    // and stay callable as hidden aliases on this surface (dispatch/access.rs
    // ::is_tool_registered), so the recovery chain resolves through dispatch,
    // not the listing. Keeping them off the listed surface restores the diet
    // core-20 cap (ADR-0016 ≤20; P1 had temporarily lifted it to 22).
    // Known scope cut: refresh_symbol_index background responses point at
    // get_analysis_job, which this surface does NOT expose (diet cap 20,
    // enforced by reviewer_graph_core_surface_contains_alwaysload_and_
    // verb_facades). Reviewer sessions needing background refresh should
    // switch to builder-minimal/planner-readonly; the sync default works
    // here unchanged.
    "refresh_symbol_index",
    "get_capabilities",
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

/// Listed-callable membership for a surface (surface listing ∪ deprecated-alias
/// resolution). Since ADR-0016 decoupled runtime callability from listing
/// (`dispatch/access.rs::is_tool_registered`), this predicate is no longer a
/// runtime gate — it survives only as a doc/overlay-integrity invariant helper
/// (host-adapter overlay tests assert overlays reference listed tools), so it
/// is test-only.
#[cfg(test)]
pub(crate) fn is_tool_callable_in_surface(name: &str, surface: ToolSurface) -> bool {
    is_tool_in_surface(name, surface)
        || deprecated_workflow_alias(name)
            .map(|(replacement, _)| is_tool_in_surface(replacement, surface))
            .unwrap_or(false)
}

/// ADR-0016 decision 1: the always-loaded CORE-10. This is the deferred
/// *bootstrap* slice — schemas preloaded so the model can call them without a
/// ToolSearch round-trip. Deliberately narrower than the CORE-20 static default
/// surface (`tool_default_listed`): a host using CodeLens-side deferred loading
/// gets CORE-10 up front and expands the remaining CORE-20 (and beyond) on
/// demand, whereas a host with native tool search (deferred disabled) receives
/// the full static CORE-20 directly.
pub(crate) fn tool_is_always_loaded_core(name: &str) -> bool {
    CORE_10_TOOLS.contains(&name)
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
mod adr_0016_default_surface_tests {
    use super::*;

    /// The 39 hidden aliases from `docs/design/workflow-first-tool-surface-migration.md`
    /// (disposition `alias→X`). They must be callable but never appear on the
    /// default listed surface (ADR-0016 decision 3).
    const HIDDEN_ALIASES_39: &[&str] = &[
        "get_symbols_overview",
        "bm25_symbol_search",
        "search_symbols_fuzzy",
        "search_workspace_symbols",
        "resolve_symbol_target",
        "find_declaration",
        "find_implementations",
        "get_type_hierarchy",
        "get_callers",
        "get_callees",
        "find_scoped_references",
        "trace_request_path",
        "impact_report",
        "diff_aware_references",
        "get_file_diagnostics",
        "get_diagnostics_for_symbol",
        "unresolved_reference_check",
        "diagnose_issues",
        "review_architecture",
        "review_changes",
        "module_boundary_report",
        "dead_code_report",
        "cleanup_duplicate_logic",
        "find_code_duplicates",
        "find_similar_code",
        "find_misplaced_code",
        "mermaid_module_graph",
        "safe_rename_report",
        "plan_symbol_rename",
        "refactor_safety_report",
        "analyze_change_request",
        "explore_codebase",
        "onboard_project",
        "analyze",
        "find_tests",
        "list_analysis_jobs",
        "list_analysis_artifacts",
        "activate_project",
        "get_capabilities",
    ];

    /// Tools whose registration is gated behind the `semantic` feature; when the
    /// feature is off they are legitimately unregistered, so callability is not
    /// asserted for them.
    fn is_semantic_gated_off(name: &str) -> bool {
        matches!(tool_feature_gate(name), Some("semantic")) && !cfg!(feature = "semantic")
    }

    /// Mirrors `dispatch/access.rs::is_registered_tool` — the runtime callability
    /// gate that keeps hidden aliases dispatchable without a listed slot.
    fn alias_is_callable(name: &str) -> bool {
        crate::tool_defs::tool_definition(name).is_some()
            || deprecated_workflow_alias(name).is_some()
            || whitelist_preset_member_union().contains(name)
    }

    #[test]
    fn core_10_is_subset_of_core_20() {
        for tool in CORE_10_TOOLS {
            assert!(
                CORE_20_TOOLS.contains(tool),
                "CORE-10 member `{tool}` must also be in CORE-20"
            );
        }
        assert_eq!(
            CORE_10_TOOLS.len(),
            10,
            "ADR-0016 decision 1 fixes CORE-10 at 10"
        );
    }

    #[test]
    fn core_10_matches_adr_decision_1_roster() {
        // ADR-0016 decision 1, verbatim.
        let adr: std::collections::BTreeSet<&str> = [
            "prepare_harness_session",
            "search",
            "overview",
            "graph",
            "diagnose",
            "review",
            "plan_safe_refactor",
            "verify_change_readiness",
            "get_changed_files",
            "get_current_config",
        ]
        .into_iter()
        .collect();
        let core: std::collections::BTreeSet<&str> = CORE_10_TOOLS.iter().copied().collect();
        assert_eq!(core, adr, "CORE_10_TOOLS must equal ADR-0016 decision 1");
    }

    #[test]
    fn core_20_matches_adr_decision_2_roster() {
        // ADR-0016 decision 2, verbatim (CORE-10 plus the ten extras).
        let adr: std::collections::BTreeSet<&str> = [
            "prepare_harness_session",
            "search",
            "overview",
            "graph",
            "diagnose",
            "review",
            "plan_safe_refactor",
            "verify_change_readiness",
            "get_changed_files",
            "get_current_config",
            "get_ranked_context",
            "find_symbol",
            "find_referencing_symbols",
            "semantic_search",
            "refresh_symbol_index",
            "get_watch_status",
            "start_analysis_job",
            "get_analysis_job",
            "get_analysis_section",
            "cancel_analysis_job",
        ]
        .into_iter()
        .collect();
        let core: std::collections::BTreeSet<&str> = CORE_20_TOOLS.iter().copied().collect();
        assert_eq!(core.len(), 20, "CORE-20 must contain exactly 20 names");
        assert_eq!(core, adr, "CORE_20_TOOLS must equal ADR-0016 decision 2");
        // No duplicates in the ordered constant.
        assert_eq!(
            CORE_20_TOOLS.len(),
            20,
            "CORE_20_TOOLS has duplicate entries"
        );
    }

    /// The generated default listing (from tools.toml `default_visible_rank`)
    /// is the single runtime source; it must agree with the declared CORE-20
    /// roster. Both are feature-independent (semantic_search present in each).
    #[test]
    fn default_listed_tool_names_equals_core_20() {
        let generated: std::collections::BTreeSet<&str> =
            default_listed_tool_names().iter().copied().collect();
        let core: std::collections::BTreeSet<&str> = CORE_20_TOOLS.iter().copied().collect();
        assert_eq!(
            generated, core,
            "tools.toml default_visible_rank must generate exactly the CORE-20 roster"
        );
    }

    #[test]
    fn hidden_aliases_are_unlisted_but_callable() {
        assert_eq!(
            HIDDEN_ALIASES_39.len(),
            39,
            "migration table lists 39 aliases"
        );
        let listed: std::collections::BTreeSet<&str> =
            default_listed_tool_names().iter().copied().collect();
        for alias in HIDDEN_ALIASES_39 {
            assert!(
                !listed.contains(alias),
                "hidden alias `{alias}` must not appear on the default listed surface (ADR-0016 decision 3)"
            );
            if is_semantic_gated_off(alias) {
                continue;
            }
            assert!(
                alias_is_callable(alias),
                "hidden alias `{alias}` must stay callable via the registration gate (dispatch/access.rs::is_registered_tool)"
            );
        }
    }

    /// ADR-0016 decision 4: host-native duplicates leave the default surface and
    /// remain behind the generic compatibility profile only. Their exclusion is
    /// expressed by membership in `BALANCED_EXCLUDES` (and absence from CORE-20).
    #[test]
    fn host_native_duplicates_are_off_the_default_surface() {
        for name in ["read_file", "list_dir", "find_file"] {
            assert!(
                BALANCED_EXCLUDES.contains(&name),
                "{name} must stay in BALANCED_EXCLUDES (generic profile only)"
            );
            assert!(
                !CORE_20_TOOLS.contains(&name),
                "{name} must not be part of the CORE-20 default surface"
            );
            assert!(
                !default_listed_tool_names().contains(&name),
                "{name} must not be default-listed"
            );
        }
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

    /// ADR-0018 Decision #3: hosts own agent coordination, so the
    /// server-side claim registry is deprecated for v2.0 removal. Through
    /// the deprecation window the quartet stays registered (callable via
    /// `tools/call`, callers see `codelens/deprecated*` response meta) and
    /// is dropped from default listings by the `tool_deprecation` filter.
    #[test]
    fn coordination_quartet_is_deprecated_for_v2_removal() {
        for name in [
            "register_agent_work",
            "list_active_agents",
            "claim_files",
            "release_files",
        ] {
            assert_eq!(
                tool_deprecation(name),
                Some(("1.13.34", "", "2.0")),
                "{name} must carry ADR-0018 D3 deprecation metadata"
            );
            assert!(
                crate::tool_defs::tool_definition(name).is_some(),
                "{name} stays registered through the deprecation window"
            );
        }
    }

    /// #350: every read surface that exposes a fallback-hint emitter
    /// (find_symbol miss hint, the D1 LSP read trio) must also expose
    /// the hint targets, or the suggested recovery chain dead-ends on
    /// "not available in active surface".
    ///
    /// Scope note (2026-07 tool-surface diet): the reviewer-graph surface
    /// is intentionally exempt from this invariant. It is now the curated
    /// core-20 default where the recovery targets (`bm25_symbol_search`,
    /// the D1 LSP trio) are dropped from the *listed* surface but stay
    /// callable via `tools/call` — the hint chain resolves through
    /// dispatch, not the listing. The invariant still holds for the
    /// planner/builder surfaces, which keep the full emitter+target set.
    #[test]
    fn fallback_hint_targets_are_present_wherever_their_emitters_are() {
        const EMITTERS: &[&str] = &["find_symbol", "find_declaration", "find_implementations"];
        const HINT_TARGETS: &[&str] = &["find_symbol", "bm25_symbol_search"];
        for (label, surface) in [
            ("planner-readonly", PLANNER_READONLY_TOOLS),
            ("builder-minimal", BUILDER_MINIMAL_TOOLS),
        ] {
            let has_emitter = EMITTERS.iter().any(|tool| surface.contains(tool));
            assert!(has_emitter, "{label} unexpectedly lost all hint emitters");
            for target in HINT_TARGETS {
                assert!(
                    surface.contains(target),
                    "{label} exposes a hint emitter but not its target `{target}` — the recovery chain dead-ends (#350)"
                );
            }
        }
    }

    /// 2026-07 tool-surface diet, step 1: the default `review` surface is
    /// the curated core-20. Lock its composition so a later edit can't
    /// silently drop an always-load entrypoint or a canonical verb façade
    /// (both are load-bearing — always-load per the v1.13.34 CHANGELOG;
    /// search/graph are named by the codelens-first hook + rules/harness.md,
    /// the other façades by the documented mode-routing family), and
    /// so the diet cap of 20 holds.
    #[test]
    fn reviewer_graph_core_surface_contains_alwaysload_and_verb_facades() {
        const ALWAYS_LOAD: &[&str] = &[
            "prepare_harness_session",
            "explore_codebase",
            "review_changes",
            "review_architecture",
            "verify_change_readiness",
            "find_symbol",
            "find_referencing_symbols",
            "get_symbols_overview",
            "get_ranked_context",
        ];
        const VERB_FACADES: &[&str] = &["search", "graph", "overview", "diagnose", "review"];
        for tool in ALWAYS_LOAD {
            assert!(
                REVIEWER_GRAPH_TOOLS.contains(tool),
                "core review surface must retain always-load entrypoint `{tool}`"
            );
        }
        for verb in VERB_FACADES {
            assert!(
                REVIEWER_GRAPH_TOOLS.contains(verb),
                "core review surface must retain canonical verb façade `{verb}`"
            );
        }
        // ADR-0016: the #350 call-graph hint targets (get_callers/get_callees)
        // resolve through hidden-alias callability, not a listed slot, so the
        // curated review surface holds the diet core-20 cap. (P1 had
        // temporarily lifted it to 22 by listing the two primitives directly.)
        assert!(
            REVIEWER_GRAPH_TOOLS.len() <= 20,
            "core review surface must stay within the diet cap of 20 (got {})",
            REVIEWER_GRAPH_TOOLS.len()
        );
        let unique: std::collections::BTreeSet<_> = REVIEWER_GRAPH_TOOLS.iter().collect();
        assert_eq!(
            unique.len(),
            REVIEWER_GRAPH_TOOLS.len(),
            "core review surface has duplicate entries"
        );
    }

    /// #350 / ADR-0016 hidden-alias contract: `find_referencing_symbols` emits
    /// `cross_file_callers_hint` → `get_callers`. The hint no longer dead-ends
    /// on read surfaces because the targets resolve through *callability*, not
    /// listing: get_callers/get_callees are registered in tools.toml, so
    /// `dispatch/access.rs::is_tool_registered` keeps them dispatchable as
    /// hidden aliases even on planner-readonly / reviewer-graph, which no longer
    /// spend a listed slot on them. builder-minimal still lists them outright.
    #[test]
    fn call_graph_hint_targets_callable_on_read_surfaces() {
        // Registration = callability under the ADR-0016 gate: the two hint
        // targets must exist in tools.toml so hidden-alias dispatch resolves.
        for target in ["get_callers", "get_callees"] {
            assert!(
                crate::tool_defs::tool_definition(target).is_some(),
                "call-graph hint target `{target}` must be registered so it stays \
                 callable as a hidden alias on read surfaces (#350 / ADR-0016)"
            );
        }
        // Restored diet cap: the two primitives are dropped from the *listed*
        // planner / reviewer surfaces (they resolve via hidden-alias dispatch),
        // while builder-minimal keeps them listed for mid-edit call-graph work.
        for (label, surface) in [
            ("planner-readonly", PLANNER_READONLY_TOOLS),
            ("reviewer-graph", REVIEWER_GRAPH_TOOLS),
        ] {
            for target in ["get_callers", "get_callees"] {
                assert!(
                    !surface.contains(&target),
                    "{label} must NOT list `{target}` — it resolves via hidden-alias \
                     callability, keeping the surface within the ≤20 diet cap"
                );
            }
        }
        for target in ["get_callers", "get_callees"] {
            assert!(
                BUILDER_MINIMAL_TOOLS.contains(&target),
                "builder-minimal must keep `{target}` listed for mid-edit call-graph work"
            );
        }
    }
}
