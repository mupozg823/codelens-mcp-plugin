mod audit_common;
pub(crate) mod builder_audit;
mod coordination;
pub(crate) mod metrics_config;
pub(crate) mod planner_audit;
mod project_ops;
mod tool_search;

pub(crate) use builder_audit::audit_builder_session;
pub(crate) use coordination::{
    claim_files, list_active_agents, register_agent_work, release_files,
};
pub(crate) use metrics_config::{
    export_session_markdown, get_capabilities, get_tool_metrics, get_watch_status,
    prune_index_failures, set_preset, set_profile,
};
pub(crate) use planner_audit::audit_planner_session;
pub(crate) use project_ops::{
    activate_project, add_queryable_project, auto_set_embed_hint_lang, list_queryable_projects,
    prepare_for_new_conversation, prepare_harness_session, query_project, remove_queryable_project,
    summarize_changes,
};
pub(crate) use tool_search::tool_search;
