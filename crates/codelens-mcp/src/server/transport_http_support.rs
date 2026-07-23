use super::project_binding::ProjectBindingSource;
use super::session::{SessionClientMetadata, SessionSeed, SessionStore};
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
    let host_context = string_param(params, "hostContext", "host_context").or_else(|| {
        headers
            .get("x-codelens-host-context")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
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

    // #347: capture the caller's workspace at initialize so a shared
    // daemon never silently serves its own default project. Two paths:
    // `params.project` (programmatic clients) and the
    // `x-codelens-project` header (host configs — e.g. a per-project
    // `.mcp.json` emitted by `codelens-mcp attach`).
    let initialize_project_path = params
        .get("project")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let header_project_path = project_header_value(headers);
    let (project_path, project_binding_source) =
        ProjectBindingSource::from_initialize(initialize_project_path, header_project_path);
    let available_mcp_servers =
        string_array_param(params, "availableMcpServers", "available_mcp_servers")
            .unwrap_or_else(|| csv_header_values(headers, "x-codelens-available-mcp-servers"));
    let available_mcp_tools =
        string_array_param(params, "availableMcpTools", "available_mcp_tools")
            .unwrap_or_else(|| csv_header_values(headers, "x-codelens-available-mcp-tools"));
    let skill_roots = string_array_param(params, "skillRoots", "skill_roots")
        .unwrap_or_else(|| csv_header_values(headers, "x-codelens-skill-roots"));
    let memory_roots = string_array_param(params, "memoryRoots", "memory_roots")
        .unwrap_or_else(|| csv_header_values(headers, "x-codelens-memory-roots"));
    let host_setting_keys = string_array_param(params, "hostSettingKeys", "host_setting_keys")
        .unwrap_or_else(|| csv_header_values(headers, "x-codelens-host-setting-keys"));
    let harness_profile = string_param(params, "harnessProfile", "harness_profile").or_else(|| {
        headers
            .get("x-codelens-harness-profile")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    });
    let host_capabilities =
        crate::host_capabilities::HostCapabilities::from_initialize_params(params);

    if client_name.is_none()
        && client_version.is_none()
        && requested_profile.is_none()
        && host_context.is_none()
        && trusted_client.is_none()
        && deferred_tool_loading.is_none()
        && project_path.is_none()
        && available_mcp_servers.is_empty()
        && available_mcp_tools.is_empty()
        && skill_roots.is_empty()
        && memory_roots.is_empty()
        && host_setting_keys.is_empty()
        && harness_profile.is_none()
        && host_capabilities.is_none()
    {
        return None;
    }

    Some(SessionClientMetadata {
        client_name,
        client_version,
        requested_profile,
        host_context,
        trusted_client,
        deferred_tool_loading,
        project_path,
        project_binding_source,
        loaded_namespaces: Vec::new(),
        loaded_tiers: Vec::new(),
        full_tool_exposure: None,
        available_mcp_servers,
        available_mcp_tools,
        skill_roots,
        memory_roots,
        host_setting_keys,
        harness_profile,
        host_capabilities,
    })
}

impl SessionSeed {
    /// Guard #2/#8: build a resurrection seed from request headers. Reads only
    /// soft surface knobs (`x-codelens-profile`, `x-codelens-deferred-tool-loading`,
    /// `x-codelens-client`) plus the non-privileged `x-codelens-project`
    /// workspace binding (#351). It deliberately does NOT read
    /// `x-codelens-trusted-client` — that privilege-bearing header is honored
    /// only on `initialize`, so a non-initialize resurrection can never assert
    /// trust and bypass the mutation gate.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let header = |key: &str| {
            headers
                .get(key)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        };
        SessionSeed {
            requested_profile: header("x-codelens-profile"),
            deferred_tool_loading: headers
                .get("x-codelens-deferred-tool-loading")
                .and_then(|value| value.to_str().ok())
                .and_then(parse_bool_header),
            client_name: header("x-codelens-client"),
            host_context: header("x-codelens-host-context"),
            project_path: project_header_value(headers),
            available_mcp_servers: csv_header_values(headers, "x-codelens-available-mcp-servers"),
            available_mcp_tools: csv_header_values(headers, "x-codelens-available-mcp-tools"),
            skill_roots: csv_header_values(headers, "x-codelens-skill-roots"),
            memory_roots: csv_header_values(headers, "x-codelens-memory-roots"),
            host_setting_keys: csv_header_values(headers, "x-codelens-host-setting-keys"),
            harness_profile: header("x-codelens-harness-profile"),
            host_capabilities: None,
        }
    }
}

/// Trimmed, non-empty `x-codelens-project` header value.
pub(crate) fn project_header_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-codelens-project")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// #351: re-assert the workspace binding on EVERY request that carries
/// `x-codelens-project`, not only at `initialize`. This makes
/// header-attached hosts immune to session eviction: even when the 30-min
/// idle sweep dropped the session and the lenient gate resurrected it with
/// default metadata, the very same request re-binds it before dispatch.
/// Also covers clients that never declared a project at initialize but
/// send the header later. Header-bound sessions can still switch workspaces,
/// while initialize params and explicit prepare/activate requests take
/// precedence over lower-precedence recurring headers.
pub(crate) fn rebind_session_project_from_headers(
    store: &SessionStore,
    session_id: &str,
    headers: &HeaderMap,
) {
    let Some(project) = project_header_value(headers) else {
        return;
    };
    store.set_project_path_from_header(session_id, &project);
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
        store.set_client_metadata(&session.id, metadata);
    }
    if !resumed {
        session.set_surface(initial_surface);
        session.set_token_budget(initial_budget);
    }
    store.seed_default_project_path(&session.id, initial_project_path);
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

fn string_param(params: &serde_json::Value, camel_key: &str, snake_key: &str) -> Option<String> {
    params
        .get(camel_key)
        .or_else(|| params.get(snake_key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_array_param(
    params: &serde_json::Value,
    camel_key: &str,
    snake_key: &str,
) -> Option<Vec<String>> {
    let values = params.get(camel_key).or_else(|| params.get(snake_key))?;
    let mut normalized = Vec::new();
    for item in values.as_array()? {
        let Some(value) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !normalized.iter().any(|existing| existing == value) {
            normalized.push(value.to_owned());
        }
    }
    Some(normalized)
}

fn csv_header_values(headers: &HeaderMap, key: &str) -> Vec<String> {
    let Some(raw) = headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for item in raw.split(',') {
        let value = item.trim();
        if value.is_empty() || values.iter().any(|existing| existing == value) {
            continue;
        }
        values.push(value.to_owned());
    }
    values
}

fn annotate_initialize_response(
    mut resp: JsonRpcResponse,
    session: &InitializeSession,
    daemon_mode: &str,
) -> JsonRpcResponse {
    if let Some(result) = resp.result.as_mut()
        && let Some(obj) = result.as_object_mut()
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_seed_reads_profile_deferred_client_not_trusted() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codelens-profile",
            HeaderValue::from_static("reviewer-graph"),
        );
        headers.insert(
            "x-codelens-deferred-tool-loading",
            HeaderValue::from_static("1"),
        );
        headers.insert("x-codelens-client", HeaderValue::from_static("codex"));
        // Guard #2: trusted_client must be ignored on the resurrection path.
        headers.insert("x-codelens-trusted-client", HeaderValue::from_static("1"));
        let seed = SessionSeed::from_headers(&headers);
        assert_eq!(seed.requested_profile.as_deref(), Some("reviewer-graph"));
        assert_eq!(seed.deferred_tool_loading, Some(true));
        assert_eq!(seed.client_name.as_deref(), Some("codex"));
        // SessionSeed has no trusted_client field — guard #2 enforced by type.
    }
}
