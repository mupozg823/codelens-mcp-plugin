#![cfg(feature = "http")]

use super::session::{SessionClientMetadata, SessionStore};
use crate::client_profile::ClientProfile;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tool_defs::ToolSurface;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone, Debug)]
pub(crate) struct InitializeSession {
    pub(crate) id: String,
    pub(crate) resumed: bool,
    pub(crate) active_sessions: usize,
    pub(crate) timeout_secs: u64,
}

pub(crate) fn extract_initialize_metadata(
    request: &JsonRpcRequest,
    headers: &HeaderMap,
) -> Option<SessionClientMetadata> {
    let params = request.params.as_ref()?;
    let client_info = params.get("clientInfo");
    let client_name = client_info
        .and_then(|info| info.get("name"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("x-codelens-client")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        });
    let client_version = client_info
        .and_then(|info| info.get("version"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("x-codelens-client-version")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        });
    let requested_profile = params
        .get("profile")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("x-codelens-profile")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        });
    let trusted_client = headers
        .get("x-codelens-trusted-client")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_bool_header);
    let client_profile = client_name
        .as_deref()
        .map(|name| ClientProfile::detect(Some(name)));
    let deferred_tool_loading = params
        .get("deferredToolLoading")
        .and_then(|value| value.as_bool())
        .or_else(|| {
            params
                .get("clientCapabilities")
                .and_then(|value| value.get("deferredToolLoading"))
                .and_then(|value| value.as_bool())
        })
        .or_else(|| {
            headers
                .get("x-codelens-deferred-tool-loading")
                .and_then(|value| value.to_str().ok())
                .and_then(parse_bool_header)
        })
        .or_else(|| client_profile.and_then(|profile| profile.default_deferred_tool_loading()));

    if client_name.is_none()
        && client_version.is_none()
        && requested_profile.is_none()
        && trusted_client.is_none()
        && deferred_tool_loading.is_none()
    {
        return None;
    }

    Some(SessionClientMetadata {
        client_name,
        client_version,
        requested_profile,
        trusted_client,
        deferred_tool_loading,
        project_path: None,
        loaded_namespaces: Vec::new(),
        loaded_tiers: Vec::new(),
        full_tool_exposure: None,
    })
}

pub(crate) fn create_initialize_session(
    store: Option<&SessionStore>,
    requested_session_id: Option<&str>,
    metadata: Option<SessionClientMetadata>,
    initial_project_path: &str,
    initial_surface: ToolSurface,
    initial_budget: usize,
) -> Option<InitializeSession> {
    let store = store?;
    let (session, resumed) = store.create_or_resume(requested_session_id);
    if let Some(metadata) = metadata {
        session.set_client_metadata(metadata);
    }
    if !resumed {
        session.set_surface(initial_surface);
        session.set_token_budget(initial_budget);
    }
    if session.client_metadata().project_path.is_none() {
        session.set_project_path(initial_project_path);
    }
    Some(InitializeSession {
        id: session.id.clone(),
        resumed,
        active_sessions: store.len(),
        timeout_secs: store.timeout_secs(),
    })
}

pub(crate) fn into_mcp_response(
    resp: JsonRpcResponse,
    accept: &str,
    initialize_session: Option<&InitializeSession>,
    daemon_mode: &str,
) -> Response {
    let resp = if let Some(session) = initialize_session {
        annotate_initialize_response(resp, session, daemon_mode)
    } else {
        resp
    };

    if accept.contains("text/event-stream") {
        return sse_single_response(resp, initialize_session);
    }

    json_response(resp, initialize_session)
}

fn parse_bool_header(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn annotate_initialize_response(
    mut resp: JsonRpcResponse,
    session: &InitializeSession,
    daemon_mode: &str,
) -> JsonRpcResponse {
    if let Some(result) = resp.result.as_mut() {
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "session".to_owned(),
                serde_json::json!({
                    "id": session.id,
                    "resumed": session.resumed,
                    "active_sessions": session.active_sessions,
                    "timeout_seconds": session.timeout_secs,
                    "daemon_mode": daemon_mode
                }),
            );
        }
    }
    resp
}

fn json_response(resp: JsonRpcResponse, session: Option<&InitializeSession>) -> Response {
    let json = match serde_json::to_string(&resp) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(r#"{{"error":"{}"}}"#, e),
            )
                .into_response();
        }
    };

    let mut response =
        (StatusCode::OK, [("content-type", "application/json")], json).into_response();
    apply_session_headers(&mut response, session);
    response
}

fn sse_single_response(resp: JsonRpcResponse, session: Option<&InitializeSession>) -> Response {
    let json =
        serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"error":"serialization"}"#.to_owned());

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(2);
    tokio::spawn(async move {
        let event = Event::default().event("message").data(json);
        let _ = tx.send(Ok(event)).await;
    });

    let stream = ReceiverStream::new(rx);
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    apply_session_headers(&mut response, session);
    response
}

fn apply_session_headers(response: &mut Response, session: Option<&InitializeSession>) {
    let Some(session) = session else {
        return;
    };
    if let Ok(val) = HeaderValue::from_str(&session.id) {
        response.headers_mut().insert("mcp-session-id", val);
    }
    let resumed_header = if session.resumed { "true" } else { "false" };
    if let Ok(val) = HeaderValue::from_str(resumed_header) {
        response
            .headers_mut()
            .insert("x-codelens-session-resumed", val);
    }
}
