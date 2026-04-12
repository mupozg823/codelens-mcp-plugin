pub mod composite;
pub mod filesystem;
pub mod graph;
pub mod lsp;
pub mod memory;
pub mod mutation;
pub(crate) mod query_analysis;
mod report_contract;
pub(crate) mod report_jobs;
mod report_payload;
mod report_utils;
mod report_verifier;
pub mod reports;
pub mod session;
pub mod symbols;
pub mod workflows;

use crate::AppState;
use crate::tool_defs::{ToolProfile, ToolSurface};
pub use crate::tool_runtime::{
    ToolHandler, ToolResult, optional_bool, optional_string, optional_usize, required_string,
    success_meta,
};
use std::collections::HashMap;

/// Declarative tool registry macro — reduces boilerplate and prevents drift.
/// Each entry is `"tool_name" => module::handler_fn`.
macro_rules! tool_registry {
    ($($name:expr => $handler:expr),* $(,)?) => {{
        let entries: &[(&str, ToolHandler)] = &[
            $(($name, $handler)),*
        ];
        let mut m: HashMap<&'static str, ToolHandler> = HashMap::with_capacity(entries.len());
        for &(name, handler) in entries {
            m.insert(name, handler);
        }
        m
    }};
}

/// Build the dispatch table. Add new tools here — one line per tool.
pub fn dispatch_table() -> HashMap<&'static str, ToolHandler> {
    tool_registry! {
        // ── File I/O ──
        "get_current_config"           => filesystem::get_current_config,
        "read_file"                    => filesystem::read_file_tool,
        "list_dir"                     => filesystem::list_dir_tool,
        "find_file"                    => filesystem::find_file_tool,
        "search_for_pattern"           => filesystem::search_for_pattern_tool,
        "find_annotations"             => filesystem::find_annotations,
        "find_tests"                   => filesystem::find_tests,
        // ── Symbol ──
        "get_symbols_overview"         => symbols::get_symbols_overview,
        "find_symbol"                  => symbols::find_symbol,
        "get_ranked_context"           => symbols::get_ranked_context,
        "refresh_symbol_index"         => symbols::refresh_symbol_index,
        "get_complexity"               => symbols::get_complexity,
        "search_symbols_fuzzy"         => symbols::search_symbols_fuzzy,
        "get_project_structure"        => symbols::get_project_structure,
        // ── LSP ──
        "find_referencing_symbols"     => lsp::find_referencing_symbols,
        "get_file_diagnostics"         => lsp::get_file_diagnostics,
        "search_workspace_symbols"     => lsp::search_workspace_symbols,
        "get_type_hierarchy"           => lsp::get_type_hierarchy,
        "plan_symbol_rename"           => lsp::plan_symbol_rename,
        "check_lsp_status"             => lsp::check_lsp_status,
        "get_lsp_recipe"               => lsp::get_lsp_recipe,
        // ── Analysis ──
        "get_changed_files"            => graph::get_changed_files_tool,
        "get_impact_analysis"          => graph::get_impact_analysis,
        "find_importers"               => graph::find_importers_tool,
        "get_symbol_importance"        => graph::get_symbol_importance,
        "find_dead_code"               => graph::find_dead_code_v2_tool,
        "find_referencing_code_snippets" => graph::find_referencing_code_snippets,
        "find_scoped_references"       => graph::find_scoped_references_tool,
        "get_callers"                  => graph::get_callers_tool,
        "get_callees"                  => graph::get_callees_tool,
        "find_circular_dependencies"   => graph::find_circular_dependencies_tool,
        "get_change_coupling"          => graph::get_change_coupling_tool,
        "get_architecture"             => graph::get_architecture_tool,
        // ── Edit (individual) ──
        "rename_symbol"                => mutation::rename_symbol,
        "create_text_file"             => mutation::create_text_file_tool,
        "delete_lines"                 => mutation::delete_lines_tool,
        "insert_at_line"               => mutation::insert_at_line_tool,
        "replace_lines"                => mutation::replace_lines_tool,
        "replace_content"              => mutation::replace_content_tool,
        "replace_symbol_body"          => mutation::replace_symbol_body_tool,
        "insert_before_symbol"         => mutation::insert_before_symbol_tool,
        "insert_after_symbol"          => mutation::insert_after_symbol_tool,
        "analyze_missing_imports"      => mutation::analyze_missing_imports_tool,
        "add_import"                   => mutation::add_import_tool,
        // ── Edit (unified — preferred in BALANCED/MINIMAL) ──
        "insert_content"               => mutation::insert_content_tool,
        "replace"                      => mutation::replace_content_unified,
        // ── Memory ──
        "list_memories"                => memory::list_memories,
        "read_memory"                  => memory::read_memory,
        "write_memory"                 => memory::write_memory,
        "delete_memory"                => memory::delete_memory,
        "rename_memory"                => memory::rename_memory,
        // ── Session ──
        "activate_project"             => session::activate_project,
        "prepare_harness_session"      => session::prepare_harness_session,
        "onboarding"                   => session::onboarding,
        "prepare_for_new_conversation" => session::prepare_for_new_conversation,
        "summarize_changes"            => session::summarize_changes,
        "list_queryable_projects"      => session::list_queryable_projects,
        "add_queryable_project"        => session::add_queryable_project,
        "remove_queryable_project"     => session::remove_queryable_project,
        "query_project"                => session::query_project,
        "get_watch_status"             => session::get_watch_status,
        "prune_index_failures"         => session::prune_index_failures,
        "set_preset"                   => session::set_preset,
        "set_profile"                  => session::set_profile,
        "get_capabilities"             => session::get_capabilities,
        "get_tool_metrics"             => session::get_tool_metrics,
        "export_session_markdown"      => session::export_session_markdown,
        // ── Composite ──
        "summarize_file"               => composite::summarize_file,
        "explain_code_flow"            => composite::explain_code_flow,
        "refactor_extract_function"    => composite::refactor_extract_function,
        "refactor_inline_function"     => composite::refactor_inline_function,
        "refactor_move_to_file"        => composite::refactor_move_to_file,
        "refactor_change_signature"    => composite::refactor_change_signature,
        "propagate_deletions"          => composite::propagate_deletions,
        "onboard_project"              => composite::onboard_project,
        // ── Workflow aliases (problem-first) ──
        "explore_codebase"             => workflows::explore_codebase,
        "trace_request_path"           => workflows::trace_request_path,
        "review_architecture"          => workflows::review_architecture,
        "plan_safe_refactor"           => workflows::plan_safe_refactor,
        "audit_security_context"       => workflows::audit_security_context,
        "analyze_change_impact"        => workflows::analyze_change_impact,
        "cleanup_duplicate_logic"      => workflows::cleanup_duplicate_logic,
        "review_changes"               => workflows::review_changes,
        "assess_change_readiness"      => workflows::assess_change_readiness,
        "diagnose_issues"              => workflows::diagnose_issues,
        // ── Reports / compressed context ──
        "analyze_change_request"       => reports::analyze_change_request,
        "verify_change_readiness"      => reports::verify_change_readiness,
        "find_minimal_context_for_change" => reports::find_minimal_context_for_change,
        "summarize_symbol_impact"      => reports::summarize_symbol_impact,
        "module_boundary_report"       => reports::module_boundary_report,
        "mermaid_module_graph"         => reports::mermaid_module_graph,
        "safe_rename_report"           => reports::safe_rename_report,
        "unresolved_reference_check"   => reports::unresolved_reference_check,
        "dead_code_report"             => reports::dead_code_report,
        "impact_report"                => reports::impact_report,
        "refactor_safety_report"       => reports::refactor_safety_report,
        "diff_aware_references"        => reports::diff_aware_references,
        "semantic_code_review"         => reports::semantic_code_review,
        "start_analysis_job"           => report_jobs::start_analysis_job,
        "get_analysis_job"             => report_jobs::get_analysis_job,
        "cancel_analysis_job"          => report_jobs::cancel_analysis_job,
        "get_analysis_section"         => report_jobs::get_analysis_section,
        "list_analysis_jobs"           => report_jobs::list_analysis_jobs,
        "list_analysis_artifacts"      => report_jobs::list_analysis_artifacts,
        "retry_analysis_job"           => report_jobs::retry_analysis_job,
    }
}

/// Rough token count estimate: 1 token ≈ 4 bytes of UTF-8 text.
/// Accuracy: ~±30% vs tiktoken cl100k_base. Sufficient for budget control,
/// not for precise measurement. JSON-heavy output tends to undercount.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Parse LSP args from arguments, falling back to defaults for the given command.
pub fn parse_lsp_args(arguments: &serde_json::Value, command: &str) -> Vec<String> {
    arguments
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| default_lsp_args_for_command(command))
}

pub fn default_lsp_command_for_path(file_path: &str) -> Option<String> {
    codelens_engine::default_lsp_command_for_path(file_path).map(str::to_owned)
}

pub fn default_lsp_args_for_command(command: &str) -> Vec<String> {
    codelens_engine::default_lsp_args_for_command(command)
        .unwrap_or(&[])
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect()
}

/// Tools relevant during harness PLAN phase
pub(crate) const PLAN_PHASE_TOOLS: &[&str] = &[
    "explore_codebase",
    "review_architecture",
    "analyze_change_impact",
    "analyze_change_request",
    "verify_change_readiness",
    "find_minimal_context_for_change",
    "onboard_project",
    "get_ranked_context",
    "get_symbols_overview",
    "find_symbol",
    "get_impact_analysis",
    "impact_report",
    "module_boundary_report",
    "summarize_symbol_impact",
    "get_changed_files",
    "find_referencing_symbols",
    "get_type_hierarchy",
];

/// Tools relevant during harness BUILD phase
pub(crate) const BUILD_PHASE_TOOLS: &[&str] = &[
    "explore_codebase",
    "trace_request_path",
    "plan_safe_refactor",
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_file_diagnostics",
    "replace_symbol_body",
    "insert_content",
    "replace",
    "rename_symbol",
    "create_text_file",
    "add_import",
    "analyze_missing_imports",
    "find_tests",
    "refresh_symbol_index",
    "verify_change_readiness",
];

/// Tools relevant during harness REVIEW phase
pub(crate) const REVIEW_PHASE_TOOLS: &[&str] = &[
    "review_architecture",
    "analyze_change_impact",
    "audit_security_context",
    "cleanup_duplicate_logic",
    "verify_change_readiness",
    "get_file_diagnostics",
    "get_impact_analysis",
    "find_scoped_references",
    "impact_report",
    "refactor_safety_report",
    "diff_aware_references",
    "semantic_code_review",
    "dead_code_report",
    "find_dead_code",
    "find_circular_dependencies",
    "get_changed_files",
    "find_tests",
    "unresolved_reference_check",
    "export_session_markdown",
];

/// Tools relevant during harness EVAL phase
pub(crate) const EVAL_PHASE_TOOLS: &[&str] = &[
    "analyze_change_impact",
    "audit_security_context",
    "verify_change_readiness",
    "get_file_diagnostics",
    "get_changed_files",
    "find_tests",
    "get_symbols_overview",
    "find_symbol",
    "read_file",
    "get_analysis_section",
];

const MUTATION_TOOLS: &[&str] = &[
    "rename_symbol",
    "replace_symbol_body",
    "replace_content",
    "replace_lines",
    "delete_lines",
    "insert_at_line",
    "insert_before_symbol",
    "insert_after_symbol",
    "insert_content",
    "replace",
    "create_text_file",
    "add_import",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
];

const REVIEW_TOOLS: &[&str] = &[
    "review_architecture",
    "analyze_change_impact",
    "audit_security_context",
    "get_changed_files",
    "get_impact_analysis",
    "find_scoped_references",
];

const EXPLORATION_TOOLS: &[&str] = &[
    "explore_codebase",
    "trace_request_path",
    "get_symbols_overview",
    "get_project_structure",
    "onboard_project",
    "get_current_config",
];

/// Context-aware tool suggestions: overrides static suggestions based on recent workflow.
pub fn suggest_next_contextual(
    tool_name: &str,
    recent_tools: &[String],
    harness_phase: Option<&str>,
) -> Option<Vec<String>> {
    let mut suggestions = suggest_next(tool_name)?;

    // After any mutation tool: always put get_file_diagnostics first
    let recent_has_mutation = recent_tools
        .iter()
        .any(|t| MUTATION_TOOLS.contains(&t.as_str()));
    if recent_has_mutation || MUTATION_TOOLS.contains(&tool_name) {
        suggestions.retain(|s| s != "get_file_diagnostics");
        suggestions.insert(0, "get_file_diagnostics".to_owned());
        suggestions.truncate(3);
    }

    // Before mutation: suggest verify_change_readiness after exploration/review
    // so agents run preflight before editing code
    if !MUTATION_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"verify_change_readiness".to_owned())
    {
        let is_pre_mutation_context = REVIEW_TOOLS.contains(&tool_name)
            || EXPLORATION_TOOLS.contains(&tool_name)
            || tool_name == "get_ranked_context"
            || tool_name == "find_symbol"
            || tool_name == "find_referencing_symbols";
        if is_pre_mutation_context {
            suggestions.push("verify_change_readiness".to_owned());
            suggestions.truncate(4);
        }
    }

    // During review workflow: boost review-oriented tools
    let recent_has_review = recent_tools
        .iter()
        .any(|t| REVIEW_TOOLS.contains(&t.as_str()));
    if recent_has_review
        && !MUTATION_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"get_impact_analysis".to_owned())
    {
        suggestions.push("get_impact_analysis".to_owned());
        suggestions.truncate(3);
    }

    // During exploration: boost deeper exploration tools
    let recent_has_exploration = recent_tools
        .iter()
        .any(|t| EXPLORATION_TOOLS.contains(&t.as_str()));
    if recent_has_exploration
        && !MUTATION_TOOLS.contains(&tool_name)
        && !REVIEW_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"get_ranked_context".to_owned())
    {
        suggestions.push("get_ranked_context".to_owned());
        suggestions.truncate(3);
    }

    // Filter suggestions by harness phase if specified
    if let Some(phase) = harness_phase {
        let phase_tools: &[&str] = match phase {
            "plan" => PLAN_PHASE_TOOLS,
            "build" => BUILD_PHASE_TOOLS,
            "review" => REVIEW_PHASE_TOOLS,
            "eval" => EVAL_PHASE_TOOLS,
            _ => return Some(suggestions), // unknown phase, no filtering
        };
        suggestions.retain(|s| phase_tools.contains(&s.as_str()));
        // Ensure we always have at least 1 suggestion
        if suggestions.is_empty() {
            suggestions = suggest_next(tool_name).unwrap_or_default();
        }
    }

    Some(suggestions)
}

fn is_workflow_tool_name(name: &str) -> bool {
    matches!(
        name,
        "explore_codebase"
            | "trace_request_path"
            | "review_architecture"
            | "plan_safe_refactor"
            | "audit_security_context"
            | "analyze_change_impact"
            | "cleanup_duplicate_logic"
            | "review_changes"
            | "assess_change_readiness"
            | "diagnose_issues"
            | "analyze_change_request"
            | "verify_change_readiness"
            | "find_minimal_context_for_change"
            | "summarize_symbol_impact"
            | "module_boundary_report"
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
    )
}

fn has_recent_low_level_chain(recent_tools: &[String]) -> bool {
    if recent_tools.len() < 3 {
        return false;
    }
    recent_tools[recent_tools.len() - 3..]
        .iter()
        .all(|tool| !is_workflow_tool_name(tool))
}

fn composite_suggestions_for_surface(surface: ToolSurface) -> &'static [&'static str] {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => &[
            "explore_codebase",
            "review_architecture",
            "analyze_change_impact",
            "plan_safe_refactor",
        ],
        ToolSurface::Profile(ToolProfile::ReviewerGraph)
        | ToolSurface::Profile(ToolProfile::CiAudit) => &[
            "review_architecture",
            "analyze_change_impact",
            "audit_security_context",
            "cleanup_duplicate_logic",
        ],
        ToolSurface::Profile(ToolProfile::RefactorFull) => &[
            "plan_safe_refactor",
            "analyze_change_impact",
            "trace_request_path",
            "review_architecture",
        ],
        ToolSurface::Profile(ToolProfile::EvaluatorCompact) => &[
            "verify_change_readiness",
            "get_file_diagnostics",
            "find_tests",
        ],
        ToolSurface::Profile(ToolProfile::WorkflowFirst) => &[
            "explore_codebase",
            "review_architecture",
            "analyze_change_impact",
            "plan_safe_refactor",
            "review_changes",
            "diagnose_issues",
        ],
        ToolSurface::Profile(ToolProfile::BuilderMinimal) | ToolSurface::Preset(_) => &[
            "explore_codebase",
            "trace_request_path",
            "plan_safe_refactor",
            "analyze_change_impact",
        ],
    }
}

pub fn composite_guidance_for_chain(
    tool_name: &str,
    recent_tools: &[String],
    surface: ToolSurface,
) -> Option<(Vec<String>, String)> {
    if is_workflow_tool_name(tool_name) || !has_recent_low_level_chain(recent_tools) {
        return None;
    }

    let suggestions = composite_suggestions_for_surface(surface)
        .iter()
        .copied()
        .filter(|candidate| *candidate != tool_name)
        .take(3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if suggestions.is_empty() {
        return None;
    }

    let hint = format!(
        "Repeated low-level chain detected. Prefer {} for compressed context before continuing.",
        suggestions.join(", ")
    );
    Some((suggestions, hint))
}

pub fn suggest_next(tool_name: &str) -> Option<Vec<String>> {
    let suggestions: &[&str] = match tool_name {
        // ── Symbols / index ──────────────────────────────────────────
        "get_symbols_overview" => &["find_symbol", "get_impact_analysis", "get_ranked_context"],
        "find_symbol" => &[
            "find_referencing_symbols",
            "get_impact_analysis",
            "replace_symbol_body",
        ],
        "get_ranked_context" => &["find_symbol", "replace_symbol_body", "semantic_search"],
        "refresh_symbol_index" => &["index_embeddings", "get_symbols_overview"],
        "get_project_structure" => &["get_symbols_overview", "get_ranked_context", "find_symbol"],
        "get_complexity" => &["find_symbol", "get_symbols_overview"],
        "search_symbols_fuzzy" => &["find_symbol", "get_ranked_context"],

        // ── LSP ──────────────────────────────────────────────────────
        "find_referencing_symbols" => &["get_impact_analysis", "rename_symbol"],
        "get_file_diagnostics" => &["find_symbol", "get_symbols_overview"],
        "search_workspace_symbols" => &["find_symbol", "get_symbols_overview"],
        "get_type_hierarchy" => &["find_referencing_symbols", "get_symbols_overview"],
        "plan_symbol_rename" => &["rename_symbol"],
        "check_lsp_status" => &["get_capabilities", "get_file_diagnostics"],
        "get_lsp_recipe" => &["check_lsp_status"],

        // ── Graph / analysis ─────────────────────────────────────────
        "get_changed_files" => &["get_impact_analysis", "get_symbols_overview"],
        "get_impact_analysis" => &["find_referencing_symbols", "get_symbols_overview"],
        "get_importers" => &["get_impact_analysis", "get_symbol_importance"],
        "get_symbol_importance" => &["get_importers", "get_impact_analysis"],
        "find_dead_code" => &["get_symbols_overview", "delete_lines"],
        "find_circular_dependencies" => &["get_impact_analysis", "get_symbols_overview"],
        "get_change_coupling" => &["get_impact_analysis", "find_dead_code"],
        "get_callers" => &["get_callees", "find_symbol"],
        "get_callees" => &["get_callers", "find_symbol"],
        "find_scoped_references" => &["rename_symbol", "find_referencing_symbols"],

        // ── Filesystem ───────────────────────────────────────────────
        "get_current_config" => &["get_capabilities", "get_project_structure"],
        "read_file" => &["get_symbols_overview", "find_symbol"],
        "search_for_pattern" => &["find_referencing_symbols", "get_ranked_context"],
        "find_annotations" => &["get_symbols_overview", "find_symbol"],
        "find_tests" => &["get_symbols_overview"],

        // ── Mutation ─────────────────────────────────────────────────
        "rename_symbol" => &[
            "safe_rename_report",
            "unresolved_reference_check",
            "get_file_diagnostics",
        ],
        "replace_symbol_body" => &["find_symbol", "get_file_diagnostics"],
        "replace_content" => &["get_file_diagnostics", "get_symbols_overview"],
        "replace_lines" => &["get_file_diagnostics"],
        "delete_lines" => &["get_file_diagnostics"],
        "insert_at_line" => &["get_file_diagnostics"],
        "insert_before_symbol" => &["get_file_diagnostics", "find_symbol"],
        "insert_after_symbol" => &["get_file_diagnostics", "find_symbol"],
        "insert_content" => &["get_file_diagnostics", "find_symbol"],
        "replace" => &["get_file_diagnostics", "get_symbols_overview"],
        "create_text_file" => &["verify_change_readiness", "get_symbols_overview"],
        "add_import" => &["get_file_diagnostics", "analyze_missing_imports"],
        "analyze_missing_imports" => &["add_import"],

        // ── Memory ───────────────────────────────────────────────────
        "write_memory" => &["list_memories", "read_memory"],
        "read_memory" => &["write_memory", "list_memories"],
        "list_memories" => &["read_memory", "write_memory"],

        // ── Session ──────────────────────────────────────────────────
        "activate_project" => &[
            "get_project_structure",
            "get_current_config",
            "get_capabilities",
        ],
        "prepare_harness_session" => &[
            "get_current_config",
            "get_capabilities",
            "get_ranked_context",
        ],
        "explore_codebase" => &[
            "find_symbol",
            "review_architecture",
            "analyze_change_impact",
        ],
        "trace_request_path" => &["plan_safe_refactor", "find_symbol", "analyze_change_impact"],
        "review_architecture" => &[
            "analyze_change_impact",
            "explore_codebase",
            "plan_safe_refactor",
        ],
        "plan_safe_refactor" => &[
            "trace_request_path",
            "analyze_change_impact",
            "get_file_diagnostics",
        ],
        "audit_security_context" => &[
            "analyze_change_impact",
            "get_analysis_section",
            "review_architecture",
        ],
        "analyze_change_impact" => &[
            "review_architecture",
            "audit_security_context",
            "get_analysis_section",
        ],
        "cleanup_duplicate_logic" => &[
            "audit_security_context",
            "review_architecture",
            "get_analysis_section",
        ],
        "onboard_project" => &["get_ranked_context", "find_symbol", "get_capabilities"],
        "get_watch_status" => &["refresh_symbol_index", "prune_index_failures"],
        "prune_index_failures" => &["get_watch_status", "refresh_symbol_index"],
        "list_queryable_projects" => &["add_queryable_project", "query_project"],
        "add_queryable_project" => &["query_project", "list_queryable_projects"],
        "query_project" => &["find_symbol", "list_queryable_projects"],
        "set_preset" => &["get_capabilities"],
        "get_capabilities" => &[
            "get_project_structure",
            "get_ranked_context",
            "check_lsp_status",
        ],
        "get_tool_metrics" => &["export_session_markdown", "set_preset", "get_capabilities"],

        // ── Semantic ─────────────────────────────────────────────────
        "semantic_search" => &["find_symbol", "get_symbols_overview", "find_similar_code"],
        "index_embeddings" => &[
            "semantic_search",
            "find_code_duplicates",
            "find_misplaced_code",
        ],
        "find_similar_code" => &["get_symbols_overview", "semantic_search"],
        "find_code_duplicates" => &["find_similar_code", "get_symbols_overview"],
        "classify_symbol" => &["find_similar_code", "get_symbols_overview"],
        "find_misplaced_code" => &["get_symbols_overview", "find_similar_code"],

        // ── Composite ────────────────────────────────────────────────
        "summarize_file" => &["get_symbols_overview", "find_symbol"],
        "explain_code_flow" => &["get_callers", "get_callees"],
        "refactor_extract_function" => &["get_file_diagnostics", "find_symbol"],
        "refactor_inline_function" => &["get_file_diagnostics", "find_symbol"],
        "refactor_move_to_file" => &["get_file_diagnostics", "find_referencing_symbols"],
        "refactor_change_signature" => &["get_file_diagnostics", "find_referencing_symbols"],
        "propagate_deletions" => &[
            "delete_lines",
            "get_file_diagnostics",
            "get_impact_analysis",
        ],
        "analyze_change_request" => &[
            "get_analysis_section",
            "verify_change_readiness",
            "impact_report",
            "refactor_safety_report",
        ],
        "verify_change_readiness" => &[
            "get_analysis_section",
            "safe_rename_report",
            "unresolved_reference_check",
        ],
        "find_minimal_context_for_change" => &["get_analysis_section", "analyze_change_request"],
        "summarize_symbol_impact" => &["get_analysis_section", "safe_rename_report"],
        "module_boundary_report" => &[
            "get_analysis_section",
            "mermaid_module_graph",
            "impact_report",
            "dead_code_report",
        ],
        "mermaid_module_graph" => &[
            "get_analysis_section",
            "module_boundary_report",
            "impact_report",
        ],
        "safe_rename_report" => &[
            "get_analysis_section",
            "unresolved_reference_check",
            "rename_symbol",
            "refactor_safety_report",
        ],
        "unresolved_reference_check" => &[
            "get_analysis_section",
            "safe_rename_report",
            "find_referencing_symbols",
        ],
        "dead_code_report" => &["get_analysis_section", "impact_report"],
        "impact_report" => &["get_analysis_section", "diff_aware_references"],
        "refactor_safety_report" => &[
            "get_analysis_section",
            "verify_change_readiness",
            "safe_rename_report",
        ],
        "diff_aware_references" => &[
            "get_analysis_section",
            "impact_report",
            "semantic_code_review",
        ],
        "semantic_code_review" => &["get_analysis_section", "impact_report"],
        "start_analysis_job" => &["get_analysis_job"],
        "get_analysis_job" => &["get_analysis_section"],
        "cancel_analysis_job" => &["start_analysis_job"],

        _ => return None,
    };
    Some(suggestions.iter().map(|s| s.to_string()).collect())
}

/// Returns a map of tool name → brief reason explaining why it is suggested.
/// Called after `suggest_next_contextual` / doom-loop overrides have finalized the list.
pub fn suggestion_reasons_for(
    tools: &[String],
    _tool_name: &str,
) -> std::collections::HashMap<String, String> {
    let mut reasons = std::collections::HashMap::new();
    for tool in tools {
        let reason = match tool.as_str() {
            "get_file_diagnostics" => "Check for type errors or lint issues after this change",
            "get_analysis_section" => "Expand a specific section from the analysis handle",
            "verify_change_readiness" => "Validate mutation safety before editing code",
            "impact_report" => "Assess blast radius of the changes",
            "module_boundary_report" => "Check coupling and boundary violations",
            "safe_rename_report" => "Preview rename safety before executing",
            "diff_aware_references" => "Find references affected by recent changes",
            "dead_code_report" => "Identify unused code after refactoring",
            "find_referencing_symbols" => "Find all callers/users of this symbol",
            "get_ranked_context" => "Get relevant context ranked by multiple signals",
            "start_analysis_job" => "Run heavy analysis asynchronously",
            "find_minimal_context_for_change" => "Get smallest context needed for this task",
            "analyze_change_request" => "Compress the change request into ranked files and risks",
            "explore_codebase" => "Get a high-level overview or targeted search",
            "review_changes" => "Review impact of changed files before merge",
            "diagnose_issues" => "Check for diagnostics or unresolved references",
            _ => "Suggested as next step in the workflow chain",
        };
        reasons.insert(tool.clone(), reason.to_owned());
    }
    reasons
}
