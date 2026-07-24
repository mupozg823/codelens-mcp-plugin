//! Output schema definitions for MCP tools.
//!
//! Split into category submodules:
//! - `symbols` — symbol navigation and code-graph tools
//! - `jobs`    — analysis jobs and project activation
//! - `harness` — capabilities, configuration, and session tools
//! - `misc`    — file, session, agent, and shared tools

mod harness;
mod jobs;
mod misc;
mod surface_core;
mod symbols;

// Re-export symbols submodule at the original pub(super) = tool_defs level.
pub(super) use symbols::bm25_symbol_search_output_schema;
pub(super) use symbols::diagnostics_output_schema;
#[cfg(feature = "semantic")]
pub(super) use symbols::embedding_coverage_report_output_schema;
pub(super) use symbols::get_callees_output_schema;
pub(super) use symbols::get_callers_output_schema;
pub(super) use symbols::lsp_navigation_output_schema;
pub(super) use symbols::ranked_context_output_schema;
pub(super) use symbols::references_output_schema;
pub(super) use symbols::resolve_symbol_target_output_schema;
#[cfg(feature = "semantic")]
pub(super) use symbols::semantic_search_output_schema;
pub(super) use symbols::symbol_diagnostics_output_schema;
pub(super) use symbols::symbol_output_schema;

// Re-export jobs submodule.
pub(super) use jobs::activate_project_output_schema;
pub(super) use jobs::analysis_artifact_list_output_schema;
pub(super) use jobs::analysis_handle_output_schema;
pub(super) use jobs::analysis_job_list_output_schema;
pub(super) use jobs::analysis_job_output_schema;
pub(super) use jobs::analysis_section_output_schema;
pub(super) use jobs::workflow_alias_output_schema;

// Re-export harness submodule.
pub(super) use harness::find_annotations_output_schema;
pub(super) use harness::find_tests_output_schema;
pub(super) use harness::get_capabilities_output_schema;
pub(super) use harness::get_current_config_output_schema;
pub(super) use harness::get_type_hierarchy_output_schema;
pub(super) use harness::prepare_harness_session_output_schema;

// Re-export misc submodule (pub(super) items).
pub(super) use misc::builder_session_audit_output_schema;
pub(super) use misc::changed_files_output_schema;
pub(super) use misc::file_content_output_schema;
pub(super) use misc::memory_list_output_schema;
pub(super) use misc::planner_session_audit_output_schema;
pub(super) use misc::prune_index_failures_output_schema;
pub(super) use misc::session_markdown_output_schema;
pub(super) use misc::tool_metrics_output_schema;
pub(super) use misc::watch_status_output_schema;

// Re-export the ADR-0016 default-surface supplemental-schema attachment map.
// `tool_defs::build` consumes this in its post-build pass to bind output
// schemas for the verb facades + reviewer-graph/ci-audit profile tools that
// the `tools.toml` codegen path does not yet declare (keeps tools.toml
// untouched while the surface-listing restructure is in flight).
#[cfg(feature = "semantic")]
pub(super) use surface_core::classify_symbol_output_schema;
pub(super) use surface_core::{
    audit_log_query_output_schema, audit_tool_surface_consistency_output_schema,
    find_over_visible_apis_output_schema, find_phantom_modules_output_schema,
    find_redundant_definitions_output_schema, get_complexity_output_schema,
    get_symbol_importance_output_schema, refresh_symbol_index_output_schema,
    verb_facade_output_schema,
};

// Re-export the 4 pub fn items from misc at their original pub visibility.
pub use misc::claim_files_output_schema;
pub use misc::list_active_agents_output_schema;
pub use misc::register_agent_work_output_schema;
pub use misc::release_files_output_schema;
