mod metrics_config;
mod project_ops;

pub use metrics_config::{
    export_session_markdown, get_capabilities, get_tool_metrics, get_watch_status,
    prune_index_failures, set_preset, set_profile,
};
pub use project_ops::{
    activate_project, add_queryable_project, list_queryable_projects, onboarding,
    prepare_for_new_conversation, query_project, remove_queryable_project, summarize_changes,
};
