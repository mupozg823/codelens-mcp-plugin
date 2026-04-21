//! Output schema definitions for MCP tools.

#[path = "output_schemas/analysis.rs"]
mod analysis;
#[path = "output_schemas/filesystem.rs"]
mod filesystem;
#[path = "output_schemas/session.rs"]
mod session;
#[path = "output_schemas/symbols.rs"]
mod symbols;

pub(super) use analysis::{
    analysis_artifact_list_output_schema, analysis_handle_output_schema,
    analysis_job_list_output_schema, analysis_job_output_schema, analysis_section_output_schema,
    workflow_output_schema,
};
pub(super) use filesystem::{
    add_import_output_schema, changed_files_output_schema, create_text_file_output_schema,
    file_content_output_schema, find_annotations_output_schema, find_tests_output_schema,
    get_project_structure_output_schema, memory_list_output_schema, onboard_output_schema,
    replace_content_output_schema, search_for_pattern_output_schema,
};
pub(super) use session::{
    activate_project_output_schema, builder_session_audit_output_schema, claim_files_output_schema,
    get_capabilities_output_schema, get_current_config_output_schema,
    list_active_agents_output_schema, planner_session_audit_output_schema,
    prepare_harness_session_output_schema, prune_index_failures_output_schema,
    register_agent_work_output_schema, release_files_output_schema, session_markdown_output_schema,
    tool_metrics_output_schema, watch_status_output_schema,
};
pub(super) use symbols::{
    bm25_symbol_search_output_schema, diagnostics_output_schema,
    get_type_hierarchy_output_schema, impact_output_schema, ranked_context_output_schema,
    references_output_schema, rename_output_schema, symbol_output_schema,
};
#[cfg(feature = "semantic")]
pub(super) use symbols::{
    classify_symbol_output_schema, find_code_duplicates_output_schema,
    find_misplaced_code_output_schema, find_similar_code_output_schema,
    semantic_search_output_schema,
};
