use super::super::{ToolPreset, ToolProfile, ToolSurface};
use super::surface_sets::{
    BALANCED_EXCLUDES, BUILDER_MINIMAL_TOOLS, CI_AUDIT_TOOLS, EVALUATOR_COMPACT_TOOLS,
    MINIMAL_TOOLS, PLANNER_READONLY_TOOLS, REFACTOR_FULL_TOOLS, REVIEWER_GRAPH_PRIMARY_TOOLS,
    REVIEWER_GRAPH_TOOLS, WORKFLOW_FIRST_TOOLS,
};

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
}

pub(crate) fn primary_tools_for_surface(surface: ToolSurface) -> Option<&'static [&'static str]> {
    match surface {
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => Some(REVIEWER_GRAPH_PRIMARY_TOOLS),
        _ => None,
    }
}

pub(crate) fn is_tool_primary_in_surface(name: &str, surface: ToolSurface) -> bool {
    match primary_tools_for_surface(surface) {
        Some(primary) => primary.contains(&name),
        None => is_tool_in_surface(name, surface),
    }
}

pub(crate) fn is_tool_in_preset(name: &str, preset: ToolPreset) -> bool {
    match preset {
        ToolPreset::Full => crate::tool_defs::build::has_listed_tool(name),
        ToolPreset::Minimal => MINIMAL_TOOLS.contains(&name),
        ToolPreset::Balanced => {
            crate::tool_defs::build::has_listed_tool(name) && !BALANCED_EXCLUDES.contains(&name)
        }
    }
}

pub(crate) fn tool_phase(name: &str) -> Option<crate::protocol::ToolPhase> {
    use crate::protocol::ToolPhase;
    match name {
        "analyze_change_request"
        | "explore_codebase"
        | "trace_request_path"
        | "review_architecture"
        | "plan_safe_refactor"
        | "plan_symbol_rename"
        | "find_minimal_context_for_change"
        | "summarize_symbol_impact"
        | "module_boundary_report"
        | "mermaid_module_graph"
        | "impact_report"
        | "get_impact_analysis"
        | "find_importers"
        | "find_referencing_code_snippets"
        | "get_callers"
        | "get_callees"
        | "get_architecture"
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
        "review_changes"
        | "diff_aware_references"
        | "verify_change_readiness"
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
        | "get_file_diagnostics"
        | "check_lsp_status"
        | "get_lsp_recipe"
        | "get_lsp_readiness"
        | "audit_builder_session"
        | "audit_planner_session"
        | "semantic_code_review" => Some(ToolPhase::Review),
        "get_tool_metrics"
        | "export_session_markdown"
        | "start_analysis_job"
        | "get_analysis_job"
        | "retry_analysis_job"
        | "cancel_analysis_job"
        | "get_analysis_section"
        | "list_analysis_jobs"
        | "list_analysis_artifacts" => Some(ToolPhase::Eval),
        _ => None,
    }
}

pub(crate) fn tool_phase_label(name: &str) -> Option<&'static str> {
    tool_phase(name).map(|p| p.as_label())
}

pub(crate) fn tool_preferred_executor(name: &str) -> Option<&'static str> {
    match name {
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
        "analyze_change_request"
        | "plan_safe_refactor"
        | "review_architecture"
        | "trace_request_path"
        | "review_changes"
        | "cleanup_duplicate_logic"
        | "find_minimal_context_for_change"
        | "summarize_symbol_impact"
        | "semantic_code_review" => Some("claude"),
        _ => None,
    }
}

pub(crate) fn tool_preferred_executor_label(name: &str) -> &'static str {
    tool_preferred_executor(name).unwrap_or("any")
}

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
        | "find_importers"
        | "find_referencing_code_snippets"
        | "find_scoped_references"
        | "get_symbol_importance"
        | "get_callers"
        | "get_callees"
        | "find_dead_code"
        | "find_circular_dependencies"
        | "get_change_coupling"
        | "get_architecture"
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
        | "cleanup_duplicate_logic"
        | "review_changes"
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
        | "retry_analysis_job"
        | "cancel_analysis_job"
        | "get_analysis_section"
        | "onboard_project"
        | "find_relevant_rules" => "reports",
        "list_memories" | "read_memory" | "write_memory" | "delete_memory" | "rename_memory" => {
            "memory"
        }
        "get_file_diagnostics" | "check_lsp_status" | "get_lsp_recipe" | "get_lsp_readiness" => {
            "lsp"
        }
        _ => "session",
    }
}
