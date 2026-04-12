//! Tool presets, profiles, surfaces, and their filtering logic.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPreset {
    Minimal,  // core tools — symbol/file/search + safe edits
    Balanced, // default — excludes niche analysis + built-in overlaps
    Full,     // all tools
}

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
}

impl ToolProfile {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "planner-readonly" | "planner" => Some(Self::PlannerReadonly),
            "builder-minimal" | "builder" => Some(Self::BuilderMinimal),
            "reviewer-graph" | "reviewer" => Some(Self::ReviewerGraph),
            "refactor-full" | "refactor" => Some(Self::RefactorFull),
            "evaluator-compact" | "evaluator" => Some(Self::EvaluatorCompact),
            "ci-audit" | "ci" => Some(Self::CiAudit),
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
];

pub(crate) const PLANNER_READONLY_TOOLS: &[&str] = &[
    // Session
    "activate_project",
    "prepare_harness_session",
    "get_current_config",
    "get_capabilities",
    "set_profile",
    "set_preset",
    "get_tool_metrics",
    // Workflow-first entrypoints
    "explore_codebase",
    "review_architecture",
    "analyze_change_impact",
    "plan_safe_refactor",
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
    "get_current_config",
    "get_capabilities",
    "set_profile",
    "set_preset",
    "get_tool_metrics",
    "explore_codebase",
    "trace_request_path",
    "plan_safe_refactor",
    "analyze_change_impact",
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
    "get_current_config",
    "set_profile",
    "set_preset",
    // Workflow-first entrypoints
    "review_architecture",
    "analyze_change_impact",
    "audit_security_context",
    "cleanup_duplicate_logic",
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
    "get_current_config",
    "set_profile",
    "set_preset",
    "get_tool_metrics",
    // Workflow-first entrypoints
    "explore_codebase",
    "trace_request_path",
    "review_architecture",
    "plan_safe_refactor",
    "analyze_change_impact",
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
    "get_current_config",
    "get_capabilities",
    "set_profile",
    "set_preset",
    "get_tool_metrics",
    "export_session_markdown",
    "explore_codebase",
    "review_architecture",
    "analyze_change_impact",
    "audit_security_context",
    "cleanup_duplicate_logic",
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
    }
}

// ── Filtering ──────────────────────────────────────────────────────────

pub(crate) fn is_tool_in_profile(name: &str, profile: ToolProfile) -> bool {
    match profile {
        ToolProfile::PlannerReadonly => PLANNER_READONLY_TOOLS.contains(&name),
        ToolProfile::BuilderMinimal => BUILDER_MINIMAL_TOOLS.contains(&name),
        ToolProfile::ReviewerGraph => REVIEWER_GRAPH_TOOLS.contains(&name),
        ToolProfile::EvaluatorCompact => EVALUATOR_COMPACT_TOOLS.contains(&name),
        ToolProfile::RefactorFull => REFACTOR_FULL_TOOLS.contains(&name),
        ToolProfile::CiAudit => CI_AUDIT_TOOLS.contains(&name),
    }
}

pub(crate) fn is_tool_in_surface(name: &str, surface: ToolSurface) -> bool {
    match surface {
        ToolSurface::Preset(preset) => is_tool_in_preset(name, preset),
        ToolSurface::Profile(profile) => is_tool_in_profile(name, profile),
    }
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

pub(crate) fn tool_namespace(name: &str) -> &'static str {
    match name {
        "read_file" | "list_dir" | "find_file" | "search_for_pattern" | "find_annotations"
        | "find_tests" => "filesystem",
        "get_symbols_overview"
        | "find_symbol"
        | "get_ranked_context"
        | "search_symbols_fuzzy"
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
        | "onboard_project" => "reports",
        "list_memories" | "read_memory" | "write_memory" | "delete_memory" | "rename_memory" => {
            "memory"
        }
        "get_file_diagnostics" | "check_lsp_status" | "get_lsp_recipe" => "lsp",
        _ => "session",
    }
}
