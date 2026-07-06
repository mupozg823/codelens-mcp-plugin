use crate::AppState;
use crate::resource_context::{
    ResourceRequestContext, build_agent_activity_payload, build_http_session_payload,
};
use crate::session_metrics_payload::build_session_metrics_payload;
use serde_json::Value;

use super::format::json_resource;

pub(super) fn token_efficiency_resource(
    state: &AppState,
    uri: &str,
    request: &ResourceRequestContext,
) -> Value {
    let metrics_payload = build_session_metrics_payload(
        state,
        if request.session.is_local() {
            None
        } else {
            Some(request.session.session_id.as_str())
        },
        request.session.project_path.as_deref(),
    );
    let mut stats = metrics_payload.session;
    stats.insert("token_bill".to_owned(), metrics_payload.token_bill);
    stats.insert("derived_kpis".to_owned(), metrics_payload.derived_kpis);
    json_resource(uri, Value::Object(stats))
}

pub(super) fn http_session_resource(
    state: &AppState,
    uri: &str,
    request: &ResourceRequestContext,
) -> Value {
    json_resource(uri, build_http_session_payload(state, request))
}

pub(super) fn agent_activity_resource(
    state: &AppState,
    uri: &str,
    request: &ResourceRequestContext,
) -> Value {
    json_resource(uri, build_agent_activity_payload(state, request))
}
