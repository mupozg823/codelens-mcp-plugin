pub mod composite;
pub mod filesystem;
pub mod graph;
pub mod lsp;
pub mod memory;
pub mod mutation;
pub(crate) mod query_analysis;
pub(crate) mod reasoning_scaffold;
mod report_contract;
pub(crate) mod report_jobs;
mod report_payload;
mod report_utils;
mod report_verifier;
pub mod reports;
pub mod rules;
pub mod session;
mod suggestions;
pub mod symbols;
pub mod workflows;

use crate::AppState;
pub use crate::tool_runtime::{
    ToolHandler, ToolResult, optional_bool, optional_string, optional_usize, required_string,
    success_meta,
};
// Re-export the recommendation-engine API so `crate::tools::*` consumers keep
// working after the split out of `tools/mod.rs`. `suggest_next` itself is only
// called from integration tests that go through `#[cfg(test)]`; internal
// callers use `suggest_next_contextual`, which wraps it.
use std::collections::HashMap;
#[allow(unused_imports)]
pub(crate) use suggestions::{
    composite_guidance_for_chain, infer_harness_phase, suggest_next, suggest_next_contextual,
    suggestion_reasons_for,
};

/// Declarative tool registry macro — reduces boilerplate and prevents drift.
/// Each entry is `"tool_name" => module::handler_fn`.
macro_rules! tool_registry {
    ($($name:expr => $handler:expr),* $(,)?) => {{
        let mut m: HashMap<&'static str, std::sync::Arc<dyn crate::tool_runtime::McpTool>> = HashMap::new();
        $(
            m.insert(
                $name,
                std::sync::Arc::new(
                    crate::tool_runtime::ToolBuilder::new($name)
                        .handler($handler)
                        .build()
                )
            );
        )*
        m
    }};
}

/// Build the dispatch table. Add new tools here — one line per tool.
#[allow(deprecated)]
pub fn dispatch_table() -> HashMap<&'static str, std::sync::Arc<dyn crate::tool_runtime::McpTool>> {
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
        "bm25_symbol_search"          => symbols::bm25_symbol_search,
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
        "register_agent_work"          => session::register_agent_work,
        "list_active_agents"           => session::list_active_agents,
        "claim_files"                  => session::claim_files,
        "release_files"                => session::release_files,
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
        "audit_builder_session"        => session::audit_builder_session,
        "audit_planner_session"        => session::audit_planner_session,
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
        // Dispatch of deprecated workflow wrappers (audit_security_context,
        // analyze_change_impact, assess_change_readiness) stays here until v2.0
        // removal so existing callers keep working. The `#[allow(deprecated)]`
        // on the enclosing fn suppresses the dispatch-site warnings.
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
        // ── Rule corpus retrieval ──
        "find_relevant_rules"          => rules::find_relevant_rules,
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
