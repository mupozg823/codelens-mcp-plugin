use crate::AppState;
use crate::resource_context::ResourceRequestContext;
use serde_json::{Value, json};

use super::analysis::{
    analysis_summary_payload, recent_analysis_jobs_payload, recent_analysis_payload,
};
use super::format::json_resource;

pub(super) fn recent_analysis_resource(state: &AppState, uri: &str) -> Value {
    json_resource(uri, recent_analysis_payload(state))
}

pub(super) fn analysis_jobs_resource(state: &AppState, uri: &str) -> Value {
    json_resource(uri, recent_analysis_jobs_payload(state))
}

pub(super) fn analysis_artifact_resource(
    state: &AppState,
    uri: &str,
    request: &ResourceRequestContext,
) -> Value {
    let trimmed = uri.trim_start_matches("codelens://analysis/");
    let mut parts = trimmed.splitn(2, '/');
    let analysis_id = parts.next().unwrap_or_default();
    let section = parts.next().unwrap_or("summary");
    if let Some(artifact) = state.get_analysis(analysis_id) {
        let content = if section == "summary" {
            state
                .metrics()
                .record_analysis_read_for_session(false, session_id_for_metrics(request));
            analysis_summary_payload(&artifact)
        } else {
            state
                .metrics()
                .record_analysis_read_for_session(true, session_id_for_metrics(request));
            state
                .get_analysis_section(analysis_id, section)
                .unwrap_or_else(|_| json!({"error": format!("Unknown section `{section}`")}))
        };
        json_resource(uri, content)
    } else {
        json_resource(
            uri,
            json!({"error": format!("Unknown analysis `{analysis_id}`")}),
        )
    }
}

fn session_id_for_metrics(request: &ResourceRequestContext) -> Option<&str> {
    if request.session.is_local() {
        None
    } else {
        Some(request.session.session_id.as_str())
    }
}
