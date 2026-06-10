pub mod admin;
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

mod scip_health;
pub(crate) mod semantic_edit;
pub(crate) mod semantic_edit_args;
pub(crate) mod semantic_retriever;
pub mod session;
mod suggestions;
pub(crate) mod symbol_query;
pub mod symbols;
pub mod workflows;

use crate::AppState;
pub use crate::tool_runtime::{
    ToolResult, optional_bool, optional_string, optional_usize, required_string, success_meta,
};
// Re-export the recommendation-engine API so `crate::tools::*` consumers keep
// working after the split out of `tools/mod.rs`. `suggest_next` itself is only
// called from integration tests that go through `#[cfg(test)]`; internal
// callers use `suggest_next_contextual`, which wraps it.
use std::collections::HashMap;
#[cfg(test)]
pub(crate) use suggestions::suggest_next;
pub(crate) use suggestions::{
    composite_guidance_for_chain, infer_harness_phase, suggest_next_contextual,
    suggestion_reasons_for,
};

/// Declarative tool registry macro — reduces boilerplate and prevents drift.
/// Each entry is `"tool_name" => module::handler_fn`.
macro_rules! tool_registry {
    ($($name:expr => $handler:expr),* $(,)?) => {{
        let mut m: HashMap<&'static str, crate::tool_defs::tool::ToolHandler> = HashMap::new();
        $(
            m.insert($name, std::sync::Arc::new($handler));
        )*
        m
    }};
}

/// Build the dispatch table. Add new tools here — one line per tool.
#[allow(deprecated)]
pub fn dispatch_table() -> HashMap<&'static str, crate::tool_defs::tool::ToolHandler> {
    tool_registry! {
        // ── File I/O ──
        "get_current_config"           => filesystem::get_current_config,
        "read_file"                    => filesystem::read_file_tool,
        "list_dir"                     => filesystem::list_dir_tool,
        "find_file"                    => filesystem::find_file_tool,
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
        // ── LSP ──
        "find_referencing_symbols"     => lsp::find_referencing_symbols,
        "get_file_diagnostics"         => lsp::get_file_diagnostics,
        "search_workspace_symbols"     => lsp::search_workspace_symbols,
        "get_type_hierarchy"           => lsp::get_type_hierarchy,
        "resolve_symbol_target"        => lsp::resolve_symbol_target,
        "plan_symbol_rename"           => lsp::plan_symbol_rename,
        "get_lsp_recipe"               => lsp::get_lsp_recipe,
        // D1 LSP read trio (#346 Phase 4) — degrade gracefully without LSP
        "find_declaration"             => lsp::find_declaration,
        "find_implementations"         => lsp::find_implementations,
        "get_diagnostics_for_symbol"   => lsp::get_diagnostics_for_symbol,
        // ── Analysis ──
        "get_changed_files"            => graph::get_changed_files_tool,
        "get_symbol_importance"        => graph::get_symbol_importance,
        "find_scoped_references"       => graph::find_scoped_references_tool,
        "get_callers"                  => graph::get_callers_tool,
        "get_callees"                  => graph::get_callees_tool,
        // ── Edit (symbolic core) — pending-D3 allowlist (#346): dispatch-only
        // until ADR-0009/D3 decides the tools.toml re-listing. The line-edit
        // family is tombstoned (see TOMBSTONED_TOOLS).
        "rename_symbol"                => mutation::rename_symbol,
        "replace_symbol_body"          => mutation::replace_symbol_body_tool,
        "insert_before_symbol"         => mutation::insert_before_symbol_tool,
        "insert_after_symbol"          => mutation::insert_after_symbol_tool,
        // ── Memory ──
        "list_memories"                => memory::list_memories,
        "read_memory"                  => memory::read_memory,
        "write_memory"                 => memory::write_memory,
        "delete_memory"                => memory::delete_memory,
        "rename_memory"                => memory::rename_memory,
        "archive_memory"              => memory::archive_memory,
        "restore_memory"              => memory::restore_memory,
        "list_archived"                => memory::list_archived,
        "read_policy"                  => memory::read_policy,
        // ── Session ──
        "activate_project"             => session::activate_project,
        "prepare_harness_session"      => session::prepare_harness_session,
        "register_agent_work"          => session::register_agent_work,
        "list_active_agents"           => session::list_active_agents,
        "claim_files"                  => session::claim_files,
        "release_files"                => session::release_files,
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
        "audit_log_query"              => admin::audit_log_query,
        "audit_tool_surface_consistency" => admin::audit_tool_surface_consistency,
        "find_phantom_modules"         => admin::find_phantom_modules,
        "find_redundant_definitions"   => admin::find_redundant_definitions,
        "find_over_visible_apis"       => admin::find_over_visible_apis,
        "audit_memory_consistency"     => admin::audit_memory_consistency,
        "export_session_markdown"      => session::export_session_markdown,
        // ── Refactor substrate — pending-D3 allowlist (#346): these arms are
        // the only callers keeping the semantic_edit substrate alive for the
        // ADR-0009/D3 decision.
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
        "cleanup_duplicate_logic"      => workflows::cleanup_duplicate_logic,
        "review_changes"               => workflows::review_changes,
        "diagnose_issues"              => workflows::diagnose_issues,
        // ── Reports / compressed context ──
        // (orchestrate_change / analyze_change_request still in dispatch for backward compat)
        "orchestrate_change"           => reports::orchestrate_change,
        "analyze_change_request"       => reports::analyze_change_request,
        "verify_change_readiness"      => reports::verify_change_readiness,
        "module_boundary_report"       => reports::module_boundary_report,
        "mermaid_module_graph"         => reports::mermaid_module_graph,
        "safe_rename_report"           => reports::safe_rename_report,
        "unresolved_reference_check"   => reports::unresolved_reference_check,
        "dead_code_report"             => reports::dead_code_report,
        "impact_report"                => reports::impact_report,
        "refactor_safety_report"       => reports::refactor_safety_report,
        "diff_aware_references"        => reports::diff_aware_references,
        "start_analysis_job"           => report_jobs::start_analysis_job,
        "get_analysis_job"             => report_jobs::get_analysis_job,
        "cancel_analysis_job"          => report_jobs::cancel_analysis_job,
        "get_analysis_section"         => report_jobs::get_analysis_section,
        "list_analysis_jobs"           => report_jobs::list_analysis_jobs,
        "list_analysis_artifacts"      => report_jobs::list_analysis_artifacts,
    }
}

/// Tools removed from dispatch for good (surface hygiene, #346 / spec
/// 2026-06-10 §D2). Serena `ToolRegistry._deleted_tools` pattern: the name
/// stays here as a tombstone so re-introduction fails tests and the script
/// drift gate, and callers of the old name get a replacement hint instead
/// of a bare "Unknown tool".
pub(crate) const TOMBSTONED_TOOLS: &[(&str, &str)] = &[
    (
        "create_text_file",
        "removed (#346) — create files with the host-native Write tool",
    ),
    (
        "delete_lines",
        "removed (#346) — line edits belong to the host-native Edit tool",
    ),
    (
        "insert_at_line",
        "removed (#346) — line edits belong to the host-native Edit tool",
    ),
    (
        "replace_lines",
        "removed (#346) — line edits belong to the host-native Edit tool",
    ),
    (
        "replace_content",
        "removed (#346) — content replacement belongs to the host-native Edit tool",
    ),
    (
        "insert_content",
        "removed (#346) — content insertion belongs to the host-native Edit tool",
    ),
    (
        "replace",
        "removed (#346) — content replacement belongs to the host-native Edit tool",
    ),
    (
        "add_import",
        "removed (#346) — add imports with the host-native Edit tool",
    ),
];

/// Pending ADR-0009/D3 (#346): the symbolic edit core + refactor
/// substrate stay dispatch-only — callable but schemaless — until the
/// re-listing decision. Must stay member-identical to
/// `DISPATCH_ONLY_ALLOWLIST` in `scripts/regen-tool-defs.py`; the
/// script enforces its side via `--enforce-drift`, the runtime
/// `audit_tool_surface_consistency` consumes this side.
pub(crate) const PENDING_D3_ALLOWLIST: &[&str] = &[
    "replace_symbol_body",
    "insert_before_symbol",
    "insert_after_symbol",
    "rename_symbol",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
    "propagate_deletions",
];

/// Replacement hint for a tombstoned tool name, if any.
pub(crate) fn tombstone_guidance(name: &str) -> Option<&'static str> {
    TOMBSTONED_TOOLS
        .iter()
        .find(|(tombstoned, _)| *tombstoned == name)
        .map(|(_, guidance)| *guidance)
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

#[cfg(test)]
mod tombstone_tests {
    use super::{TOMBSTONED_TOOLS, dispatch_table, tombstone_guidance};

    #[test]
    fn tombstoned_tools_stay_out_of_dispatch() {
        let table = dispatch_table();
        for (name, _) in TOMBSTONED_TOOLS {
            assert!(
                !table.contains_key(name),
                "{name} is tombstoned (#346) and must not be re-introduced to dispatch"
            );
        }
    }

    #[test]
    fn tombstone_guidance_for_known_and_unknown_names() {
        let guidance =
            tombstone_guidance("insert_at_line").expect("insert_at_line must be tombstoned");
        assert!(
            guidance.contains("Edit"),
            "guidance must name the replacement path: {guidance}"
        );
        assert!(tombstone_guidance("find_symbol").is_none());
    }
}
