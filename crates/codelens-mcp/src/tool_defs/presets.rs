//! Tool presets, profiles, surfaces, and their filtering logic.

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
    "refresh_symbol_index",
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
    "get_change_coupling",
    "get_symbol_importance",
    "find_dead_code",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
    "get_complexity",
    "search_symbols_fuzzy",
    "check_lsp_status",
    "get_lsp_recipe",
    // ── Overlap with Claude Code built-in tools ──
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    // ── Diagnostics / session (not needed for normal work) ──
    "prepare_for_new_conversation",
    "get_watch_status",
    "prune_index_failures",
    "get_tool_metrics",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
    "export_session_markdown",
    "summarize_changes",
    "summarize_file",
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
    "analyze_change_request",
    "verify_change_readiness",
    "find_minimal_context_for_change",
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
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_file_diagnostics",
    "find_tests",
    "refresh_symbol_index",
    // Phase 4a §capability-reporting: builders occasionally need NL
    // lookups ("where is the error handler for invalid credentials?"
    // type questions during mid-edit debugging). Exposing
    // `semantic_search` + `index_embeddings` keeps the builder
    // surface aligned with planner surface and removes the
    // "surface policy blocks a healthy feature" reporting mismatch.
    "semantic_search",
    "index_embeddings",
    "plan_symbol_rename",
    "rename_symbol",
    "replace_symbol_body",
    "insert_content",
    "replace",
    "create_text_file",
    "analyze_missing_imports",
    "add_import",
    "find_minimal_context_for_change",
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
    // Diagnostics
    "get_file_diagnostics",
    // Graph / impact
    "get_impact_analysis",
    "get_changed_files",
    // Workflow composites
    "impact_report",
    "refactor_safety_report",
    "verify_change_readiness",
    "summarize_symbol_impact",
    "diff_aware_references",
    "semantic_code_review",
    "module_boundary_report",
    "mermaid_module_graph",
    // Async analysis
    "start_analysis_job",
    "get_analysis_job",
    "get_analysis_section",
];

pub(crate) const REFACTOR_FULL_TOOLS: &[&str] = &[
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
    "get_tool_metrics",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
    // Workflow-first entrypoints
    "explore_codebase",
    "trace_request_path",
    "review_architecture",
    "plan_safe_refactor",
    "review_changes",
    "diagnose_issues",
    // Symbol exploration
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    "find_scoped_references",
    // Diagnostics
    "get_file_diagnostics",
    // Graph / impact
    "get_impact_analysis",
    "get_changed_files",
    // Mutation (core)
    "plan_symbol_rename",
    "rename_symbol",
    "replace_symbol_body",
    "insert_content",
    "replace",
    "create_text_file",
    "analyze_missing_imports",
    "add_import",
    // Refactoring
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
    // Workflow composites (preflight gate requires these)
    "refactor_safety_report",
    "safe_rename_report",
    "unresolved_reference_check",
    "verify_change_readiness",
    "impact_report",
    "diff_aware_references",
    // Content mutation (used by preflight tests)
    "replace_content",
    // Async analysis
    "start_analysis_job",
    "get_analysis_job",
    "get_analysis_section",
];

pub(crate) const CI_AUDIT_TOOLS: &[&str] = &[
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
    "review_architecture",
    "cleanup_duplicate_logic",
    "review_changes",
    "diagnose_issues",
    "read_file",
    "search_for_pattern",
    "find_tests",
    "get_symbols_overview",
    "find_symbol",
    "get_ranked_context",
    "get_changed_files",
    "get_impact_analysis",
    "find_scoped_references",
    "find_dead_code",
    "find_circular_dependencies",
    "get_change_coupling",
    "analyze_change_request",
    "verify_change_readiness",
    "summarize_symbol_impact",
    "unresolved_reference_check",
    "module_boundary_report",
    "dead_code_report",
    "impact_report",
    "refactor_safety_report",
    "diff_aware_references",
    "start_analysis_job",
    "get_analysis_job",
    "get_analysis_section",
];

/// Problem-first workflow surface: canonical workflow entrypoints + session essentials.
/// Agents see these by default; low-level tools are deferred.
pub(crate) const WORKFLOW_FIRST_TOOLS: &[&str] = &[
    // Session
    "activate_project",
    "register_agent_work",
    "list_active_agents",
    "claim_files",
    "release_files",
    "get_current_config",
    "set_preset",
    "set_profile",
    "audit_planner_session",
    "export_session_markdown",
    // Canonical workflow entrypoints
    "explore_codebase",
    "trace_request_path",
    "review_architecture",
    "plan_safe_refactor",
    "cleanup_duplicate_logic",
    "review_changes",
    "diagnose_issues",
    // Essential workflow-level tools
    "analyze_change_request",
    "onboard_project",
];

pub(crate) const EVALUATOR_COMPACT_TOOLS: &[&str] = &[
    "activate_project",
    "prepare_harness_session",
    "get_current_config",
    "get_capabilities",
    "set_profile",
    "set_preset",
    "get_changed_files",
    "verify_change_readiness",
    "get_file_diagnostics",
    "find_tests",
    "read_file",
    "get_symbols_overview",
    "find_symbol",
    "get_analysis_section",
];

// ── Budget defaults ────────────────────────────────────────────────────

pub(crate) fn default_budget_for_preset(preset: ToolPreset) -> usize {
    match preset {
        ToolPreset::Minimal => 2000,
        ToolPreset::Balanced => 4000,
        ToolPreset::Full => 8000,
    }
}

pub(crate) fn default_budget_for_profile(profile: ToolProfile) -> usize {
    match profile {
        ToolProfile::PlannerReadonly => 2400,
        ToolProfile::BuilderMinimal => 2400,
        ToolProfile::ReviewerGraph => 2800,
        ToolProfile::EvaluatorCompact => 1600,
        ToolProfile::RefactorFull => 4000,
        ToolProfile::CiAudit => 3600,
        ToolProfile::WorkflowFirst => 2400,
    }
}

// ── Filtering ──────────────────────────────────────────────────────────

/// Full deprecation info: `(since_version, replacement_tool, removal_target)`.
///
/// Used by `tools/list` and `tools/call` envelope annotations so clients can
/// surface deprecation status without consulting docs.
pub(crate) fn tool_deprecation(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        "audit_security_context" => Some(("1.12.0", "semantic_code_review", "v2.0")),
        "analyze_change_impact" => Some(("1.12.0", "impact_report", "v2.0")),
        "assess_change_readiness" => Some(("1.12.0", "verify_change_readiness", "v2.0")),
        "get_impact_analysis" => Some(("1.9.46", "impact_report", "v2.0")),
        "find_dead_code" => Some(("1.9.46", "dead_code_report", "v2.0")),
        _ => None,
    }
}

pub(crate) fn deprecated_workflow_alias(name: &str) -> Option<(&'static str, &'static str)> {
    tool_deprecation(name).map(|(_, replacement, removal)| (replacement, removal))
}

pub(crate) fn is_tool_in_profile(name: &str, profile: ToolProfile) -> bool {
    match profile {
        ToolProfile::PlannerReadonly => PLANNER_READONLY_TOOLS.contains(&name),
        ToolProfile::BuilderMinimal => BUILDER_MINIMAL_TOOLS.contains(&name),
        ToolProfile::ReviewerGraph => REVIEWER_GRAPH_TOOLS.contains(&name),
        ToolProfile::EvaluatorCompact => EVALUATOR_COMPACT_TOOLS.contains(&name),
        ToolProfile::RefactorFull => REFACTOR_FULL_TOOLS.contains(&name),
        ToolProfile::CiAudit => CI_AUDIT_TOOLS.contains(&name),
        ToolProfile::WorkflowFirst => WORKFLOW_FIRST_TOOLS.contains(&name),
    }
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

// ── Namespace mapping ──────────────────────────────────────────────────

/// Phase alias per ADR-0005 step 4 — harness-phase scoping for `tools/list`
/// filter without introducing a second tool registry. Returning `None` marks
/// the tool as phase-agnostic (infrastructure / coordination).
pub(crate) fn tool_phase(name: &str) -> Option<crate::protocol::ToolPhase> {
    use crate::protocol::ToolPhase;
    match name {
        // Plan — analyze/retrieve/orient before deciding to edit.
        "analyze_change_request"
        | "explore_codebase"
        | "trace_request_path"
        | "review_architecture"
        | "plan_safe_refactor"
        | "plan_symbol_rename"
        | "analyze_change_impact"
        | "find_minimal_context_for_change"
        | "summarize_symbol_impact"
        | "module_boundary_report"
        | "mermaid_module_graph"
        | "impact_report"
        | "get_impact_analysis"
        | "onboard_project"
        | "get_ranked_context"
        | "get_symbols_overview"
        | "find_symbol"
        | "find_referencing_symbols"
        | "search_symbols_fuzzy"
        | "search_workspace_symbols"
        | "get_type_hierarchy"
        | "semantic_search"
        | "index_embeddings"
        | "find_scoped_references"
        | "get_symbol_importance"
        | "get_change_coupling"
        | "get_complexity"
        | "find_similar_code"
        | "get_changed_files" => Some(ToolPhase::Plan),

        // Build — mutation surface.
        "rename_symbol"
        | "replace_symbol_body"
        | "delete_lines"
        | "insert_at_line"
        | "insert_before_symbol"
        | "insert_after_symbol"
        | "insert_content"
        | "replace_content"
        | "replace_lines"
        | "replace"
        | "create_text_file"
        | "add_import"
        | "refactor_extract_function"
        | "refactor_inline_function"
        | "refactor_move_to_file"
        | "refactor_change_signature"
        | "cleanup_duplicate_logic"
        | "propagate_deletions" => Some(ToolPhase::Build),

        // Review — post-edit safety, verifier, diff-aware inspection, audits.
        "review_changes"
        | "diff_aware_references"
        | "verify_change_readiness"
        | "assess_change_readiness"
        | "safe_rename_report"
        | "refactor_safety_report"
        | "unresolved_reference_check"
        | "dead_code_report"
        | "find_dead_code"
        | "find_circular_dependencies"
        | "find_misplaced_code"
        | "find_code_duplicates"
        | "classify_symbol"
        | "diagnose_issues"
        | "audit_security_context"
        | "get_file_diagnostics"
        | "check_lsp_status"
        | "get_lsp_recipe"
        | "audit_builder_session"
        | "audit_planner_session"
        | "semantic_code_review" => Some(ToolPhase::Review),

        // Eval — telemetry, audit export, analysis artifact retrieval.
        "get_tool_metrics"
        | "export_session_markdown"
        | "start_analysis_job"
        | "get_analysis_job"
        | "cancel_analysis_job"
        | "get_analysis_section"
        | "list_analysis_jobs"
        | "list_analysis_artifacts" => Some(ToolPhase::Eval),

        // Infrastructure (filesystem, memory, session coordination) is
        // deliberately phase-agnostic: used in every phase.
        _ => None,
    }
}

pub(crate) fn tool_phase_label(name: &str) -> Option<&'static str> {
    tool_phase(name).map(|p| p.as_label())
}

/// ADR-0006 Layer 1 — routing hint advising which executor class is a
/// better fit for this tool. Advisory only: the host is free to ignore.
/// `Some("codex-builder")` — bulk implementation, pure relocation,
///   multi-file refactor. Cheap/fast executor wins here.
/// `Some("claude")` — orchestration, synthesis, design compression.
///   Reasoning budget is the bottleneck.
/// `None` — either executor is fine (retrieval primitives, reads,
///   audits, session coordination, eval).
pub(crate) fn tool_preferred_executor(name: &str) -> Option<&'static str> {
    match name {
        // Bulk implementation / mutation — Codex-class executor preferred.
        "rename_symbol"
        | "replace_symbol_body"
        | "delete_lines"
        | "insert_at_line"
        | "insert_before_symbol"
        | "insert_after_symbol"
        | "insert_content"
        | "replace_content"
        | "replace_lines"
        | "replace"
        | "create_text_file"
        | "add_import"
        | "refactor_extract_function"
        | "refactor_inline_function"
        | "refactor_move_to_file"
        | "refactor_change_signature"
        | "propagate_deletions" => Some("codex-builder"),

        // Orchestration / synthesis — Claude-class executor preferred.
        "analyze_change_request"
        | "plan_safe_refactor"
        | "review_architecture"
        | "trace_request_path"
        | "review_changes"
        | "cleanup_duplicate_logic"
        | "find_minimal_context_for_change"
        | "summarize_symbol_impact"
        | "semantic_code_review" => Some("claude"),

        // Everything else — retrieval primitives, file ops, audits,
        // session coordination, eval, diagnostics — both executors
        // handle equally well. Keep None conservative until we have
        // measured divergence.
        _ => None,
    }
}

pub(crate) fn tool_preferred_executor_label(name: &str) -> &'static str {
    tool_preferred_executor(name).unwrap_or("any")
}

/// Claude Code snapshot compatibility:
/// these `_meta["anthropic/searchHint"]` phrases are consumed by the upstream
/// ToolSearch scorer for deferred MCP tools. Keep hints short and capability-
/// focused; omit low-signal tools instead of auto-generating noisy strings.
pub(crate) fn tool_anthropic_search_hint(name: &str) -> Option<&'static str> {
    match name {
        "prepare_harness_session" => Some("bootstrap CodeLens harness session"),
        "explore_codebase" => Some("explore codebase with compressed context"),
        "analyze_change_request" => Some("plan a code change safely"),
        "trace_request_path" => Some("trace a request path"),
        "review_changes" => Some("review changed files and risk"),
        "verify_change_readiness" => Some("verify edit safety before mutation"),
        "safe_rename_report" => Some("preview rename safety and blockers"),
        "refactor_safety_report" => Some("preview refactor safety and impact"),
        "start_analysis_job" => Some("run durable analysis in background"),
        "get_analysis_section" => Some("expand one analysis report section"),
        "audit_builder_session" => Some("audit builder session process"),
        "audit_planner_session" => Some("audit planner session process"),
        _ => None,
    }
}

/// Claude Code MCP tools are deferred by default. Mark only the minimal
/// turn-1 bootstrap surface as always-load so hosts can start efficiently
/// without bloating the initial tool prompt.
pub(crate) fn tool_anthropic_always_load(name: &str) -> bool {
    matches!(
        name,
        "prepare_harness_session" | "explore_codebase" | "analyze_change_request"
    )
}

pub(crate) fn tool_namespace(name: &str) -> &'static str {
    match name {
        "read_file" | "list_dir" | "find_file" | "search_for_pattern" | "find_annotations"
        | "find_tests" => "filesystem",
        "get_symbols_overview"
        | "find_symbol"
        | "get_ranked_context"
        | "search_symbols_fuzzy"
        | "bm25_symbol_search"
        | "find_referencing_symbols"
        | "search_workspace_symbols"
        | "get_type_hierarchy"
        | "plan_symbol_rename"
        | "semantic_search"
        | "index_embeddings" => "symbols",
        "get_changed_files"
        | "get_impact_analysis"
        | "find_scoped_references"
        | "get_symbol_importance"
        | "find_dead_code"
        | "find_circular_dependencies"
        | "get_change_coupling"
        | "find_similar_code"
        | "find_code_duplicates"
        | "classify_symbol"
        | "find_misplaced_code"
        | "get_complexity" => "graph",
        "rename_symbol"
        | "replace_symbol_body"
        | "delete_lines"
        | "insert_at_line"
        | "insert_before_symbol"
        | "insert_after_symbol"
        | "insert_content"
        | "replace_content"
        | "replace_lines"
        | "replace"
        | "create_text_file"
        | "add_import"
        | "refactor_extract_function"
        | "refactor_inline_function"
        | "refactor_move_to_file"
        | "refactor_change_signature" => "mutation",
        "analyze_change_request"
        | "explore_codebase"
        | "trace_request_path"
        | "review_architecture"
        | "plan_safe_refactor"
        | "audit_security_context"
        | "analyze_change_impact"
        | "cleanup_duplicate_logic"
        | "review_changes"
        | "assess_change_readiness"
        | "diagnose_issues"
        | "verify_change_readiness"
        | "find_minimal_context_for_change"
        | "summarize_symbol_impact"
        | "module_boundary_report"
        | "mermaid_module_graph"
        | "safe_rename_report"
        | "unresolved_reference_check"
        | "dead_code_report"
        | "impact_report"
        | "refactor_safety_report"
        | "diff_aware_references"
        | "start_analysis_job"
        | "get_analysis_job"
        | "cancel_analysis_job"
        | "get_analysis_section"
        | "onboard_project"
        | "find_relevant_rules" => "reports",
        "list_memories" | "read_memory" | "write_memory" | "delete_memory" | "rename_memory" => {
            "memory"
        }
        "get_file_diagnostics" | "check_lsp_status" | "get_lsp_recipe" => "lsp",
        _ => "session",
    }
}
