mod audit_common;
pub(crate) mod builder_audit;
mod coordination;
pub(crate) mod metrics_config;
pub(crate) mod planner_audit;
mod project_ops;
mod registry_ops;
mod surface_mutation;

pub use builder_audit::audit_builder_session;
pub use coordination::{claim_files, list_active_agents, register_agent_work, release_files};
pub use metrics_config::{
    export_session_markdown, get_capabilities, get_tool_metrics, get_watch_status,
    prune_index_failures,
};
pub use planner_audit::audit_planner_session;
pub use project_ops::{
    activate_project, auto_set_embed_hint_lang, prepare_for_new_conversation,
    prepare_harness_session, summarize_changes,
};
pub use registry_ops::{
    add_queryable_project, list_queryable_projects, query_project, remove_queryable_project,
};
pub use surface_mutation::{set_preset, set_profile};
