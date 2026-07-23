//! Streamable HTTP transport for MCP.
//!
//! Negotiates protocol versions declared in `protocol::SUPPORTED_PROTOCOL_VERSIONS`
//! (currently 2025-11-25, 2025-06-18, and 2025-03-26). Clients pin a version on
//! subsequent requests via the `MCP-Protocol-Version` header; absent → legacy
//! 2025-03-26, unknown → 400.
//!
//! Endpoints:
//! - POST /mcp: JSON-RPC requests. Supports Accept: application/json (default) or text/event-stream (SSE).
//! - GET /mcp: Persistent SSE stream for server→client push (requires Mcp-Session-Id).
//! - DELETE /mcp: Session termination (requires Mcp-Session-Id).

#![cfg(feature = "http")]

use super::router::handle_request;
use super::session::SseEvent;
use super::transport_http_support::{
    create_initialize_session, extract_initialize_metadata, into_mcp_response,
};
use crate::AppState;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse, SUPPORTED_PROTOCOL_VERSIONS};
use crate::tool_defs::{ToolProfile, ToolSurface, default_budget_for_profile};
use anyhow::Result;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Router, routing};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Once};
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Clone)]
pub(crate) struct TlsConfig {
    pub(crate) cert_path: PathBuf,
    pub(crate) key_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct HttpServerConfig {
    pub(crate) listen: IpAddr,
    pub(crate) port: u16,
    pub(crate) tls: Option<TlsConfig>,
}

impl HttpServerConfig {
    pub(crate) fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.listen, self.port)
    }

    pub(crate) fn transport_label(&self) -> &'static str {
        if self.tls.is_some() { "https" } else { "http" }
    }
}

pub(crate) fn install_default_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Workspace pins `rustls` to the ring backend; axum-server uses
        // `tls-rustls-no-provider` so the provider is registered here.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub(crate) async fn load_rustls_config(
    tls: &TlsConfig,
) -> Result<axum_server::tls_rustls::RustlsConfig> {
    install_default_rustls_provider();
    Ok(axum_server::tls_rustls::RustlsConfig::from_pem_file(
        tls.cert_path.clone(),
        tls.key_path.clone(),
    )
    .await?)
}

/// Origin header gate — spec §"Security Warning": servers MUST validate Origin
/// to defeat DNS rebinding attacks. We allow requests that either omit Origin
/// (curl, same-process tests, stdio-style agents) or present one pointing at
/// localhost. A non-local Origin means a browser on another site is driving
/// the request, which is exactly the rebind scenario we refuse.
fn origin_is_permitted(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    if origin == "null" {
        return true;
    }
    let Some(host) = origin
        .split_once("://")
        .and_then(|(_, rest)| rest.split('/').next())
    else {
        return false;
    };
    let host_only = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(host_only, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Spec §"Protocol Version Header": clients MUST send `MCP-Protocol-Version`
/// on every request after initialize. When absent we fall back to `2025-03-26`
/// for legacy clients; when present but unsupported we reply 400.
fn protocol_version_header_ok(headers: &HeaderMap) -> bool {
    match headers
        .get("mcp-protocol-version")
        .and_then(|v| v.to_str().ok())
    {
        None => true,
        Some(version) => SUPPORTED_PROTOCOL_VERSIONS.contains(&version),
    }
}

fn initialize_surface_and_budget(
    state: &AppState,
    metadata: Option<&super::session::SessionClientMetadata>,
) -> (ToolSurface, usize) {
    if let Some(profile) = metadata
        .and_then(|metadata| metadata.requested_profile.as_deref())
        .and_then(ToolProfile::from_str)
    {
        return (
            ToolSurface::Profile(profile),
            default_budget_for_profile(profile),
        );
    }

    (*state.surface(), state.token_budget())
}

/// Build the axum Router for the MCP HTTP transport.
/// Exposed for testing via `cargo test --features http`.
pub(crate) fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/mcp",
            routing::post(mcp_post_handler)
                .get(mcp_get_handler)
                .delete(mcp_delete_handler),
        )
        .route("/.well-known/mcp.json", routing::get(server_card_handler))
        .route(
            "/.well-known/oauth-protected-resource",
            routing::get(protected_resource_metadata_handler),
        )
        .route(
            "/.well-known/oauth-protected-resource/{*path}",
            routing::get(protected_resource_metadata_handler),
        )
        .with_state(state)
}

/// MCP Server Card — static metadata for agent discovery without a live session.
async fn server_card_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let card = crate::surface_manifest::build_server_card(&state);

    (
        StatusCode::OK,
        [("content-type", "application/json")],
        serde_json::to_string_pretty(&card).unwrap_or_default(),
    )
}

async fn protected_resource_metadata_handler(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    match state.http_auth().protected_resource_metadata() {
        Some(mut metadata) => {
            if let Some(resource) = protected_resource_path(uri.path()) {
                metadata["resource"] = serde_json::Value::String(resource);
            }
            (
                StatusCode::OK,
                [("content-type", "application/json")],
                serde_json::to_string_pretty(&metadata).unwrap_or_default(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Issue #300 / #301 / #318: the client-side stored session id has gone stale
/// (daemon restart or idle timeout). Returned only on the **strict** session
/// policy and on the SSE GET path; under the default **lenient** policy the POST
/// gate resurrects instead (see `SessionStore::get_or_resurrect`). Surfaces a
/// structured envelope plus the `x-codelens-session-rotate: 1` response header
/// so a cooperative client can reinitialize. Both POST and SSE GET funnel
/// through this helper so the hint is uniform (the #318 fix). Note: SCIP index
/// hot-reload does NOT wipe the in-memory session store, so it is not a cause.
fn unknown_session_response() -> Response {
    let body = serde_json::json!({
        "error": "unknown_session",
        "code": "session_rotate_required",
        "rotate_required": true,
        "hint": "Daemon may have restarted or the session timed out. Reinitialize the MCP session.",
        "recommended_action": "reinitialize_mcp_session",
        "action_target": "mcp_client",
    })
    .to_string();
    let mut response = (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response();
    response
        .headers_mut()
        .insert("x-codelens-session-rotate", HeaderValue::from_static("1"));
    response
}

fn protected_resource_path(request_path: &str) -> Option<String> {
    let prefix = "/.well-known/oauth-protected-resource/";
    request_path
        .strip_prefix(prefix)
        .filter(|path| !path.is_empty())
        .map(|path| format!("/{path}"))
}

/// L1 (ADR-0009 §1): authenticate the HTTP request and surface the
/// resolved principal id. Returns `Ok(Some(sub))` when JWT auth is
/// enabled and validation succeeds, `Ok(None)` when auth is `Off`
/// and no `X-Codelens-Principal` dev header is present, and `Err`
/// (with the rejection response) on auth failure.
async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<String>, Response> {
    let auth = state.http_auth();
    match auth.authorize(headers).await {
        Ok(principal) => Ok(principal),
        Err(crate::server::auth::AuthFailure::InsufficientScope) => {
            let mut challenge = auth.www_authenticate();
            if !challenge.contains("error=\"insufficient_scope\"") {
                challenge.push_str(", error=\"insufficient_scope\"");
            }
            Err((
                StatusCode::FORBIDDEN,
                [(header::WWW_AUTHENTICATE, challenge)],
                "Insufficient scope",
            )
                .into_response())
        }
        Err(_) => Err((
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, auth.www_authenticate())],
            "Unauthorized",
        )
            .into_response()),
    }
}

/// L1 (ADR-0009 §1): inject the authenticated principal id into the
/// JSON-RPC request params under the `_session_principal_id` key.
/// `SessionRequestContext::from_json` reads that key only while the HTTP
/// execution scope is active, and the role gate then prefers it over the
/// `CODELENS_PRINCIPAL` env fallback. The JSON field alone carries no
/// provenance.
fn inject_principal_id_into_params(request: &mut JsonRpcRequest, principal_id: &str) {
    if let Some(metadata) = http_session_metadata_mut(request) {
        metadata.insert(
            "_session_principal_id".to_owned(),
            serde_json::Value::String(principal_id.to_owned()),
        );
    }
}

/// Return the server-owned metadata object for HTTP methods that carry session
/// context. Omitted or non-object containers are normalized before any trusted
/// fields are inserted so dispatch never sees an authoritative principal
/// without matching transport provenance.
fn http_session_metadata_mut(
    request: &mut JsonRpcRequest,
) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
    match request.method.as_str() {
        "tools/call" => {
            let params = request.params.get_or_insert_with(|| serde_json::json!({}));
            if !params.is_object() {
                *params = serde_json::json!({});
            }
            let params = params.as_object_mut()?;
            let arguments = params
                .entry("arguments".to_owned())
                .or_insert_with(|| serde_json::json!({}));
            if !arguments.is_object() {
                *arguments = serde_json::json!({});
            }
            arguments.as_object_mut()
        }
        "tools/list" | "resources/read" => {
            let params = request.params.get_or_insert_with(|| serde_json::json!({}));
            if !params.is_object() {
                *params = serde_json::json!({});
            }
            params.as_object_mut()
        }
        _ => None,
    }
}

/// Remove caller-controlled `_session_*` metadata before the HTTP transport
/// adds its authoritative session block. Stdio retains its legacy single-client
/// argument contract; shared HTTP never trusts identity embedded in JSON.
fn sanitize_http_session_metadata(request: &mut JsonRpcRequest) {
    let metadata = match request.method.as_str() {
        "tools/call" => request
            .params
            .as_mut()
            .and_then(|params| params.as_object_mut())
            .and_then(|params| params.get_mut("arguments"))
            .and_then(|arguments| arguments.as_object_mut()),
        "tools/list" | "resources/read" => request
            .params
            .as_mut()
            .and_then(|params| params.as_object_mut()),
        _ => None,
    };
    if let Some(metadata) = metadata {
        metadata.retain(|key, _| !key.starts_with("_session_"));
    }
}

/// Phase 4d (§single-instance guard): probe the target port before
/// attempting to bind. Returns `true` if the port is already
/// accepting connections, which we interpret as "another CodeLens
/// MCP daemon is already live here — defer to it".
///
/// Uses a short 200 ms connect timeout so startup doesn't stall on
/// weird network states. `ConnectionRefused` (the normal "nothing is
/// listening" response from the kernel) is the happy path and
/// returns `false`. Any other error (timeout, permission denied,
/// network unreachable) is also treated as "port is free" —
/// conservative, since the actual `bind` call will still catch a
/// real bind conflict.
async fn addr_is_occupied(addr: SocketAddr) -> bool {
    use tokio::net::TcpStream;
    use tokio::time::{Duration, timeout};
    match timeout(Duration::from_millis(200), TcpStream::connect(addr)).await {
        // Successful connect within 200 ms → something is listening.
        Ok(Ok(_stream)) => true,
        // Any error (ConnectionRefused, etc.) → port is free.
        Ok(Err(_)) => false,
        // Timeout → treat as free (bind will catch a real conflict).
        Err(_) => false,
    }
}

#[cfg(test)]
async fn port_is_occupied(port: u16) -> bool {
    addr_is_occupied(SocketAddr::from(([127, 0, 0, 1], port))).await
}

/// Phase 4d (§single-instance guard): log and exit 0 when we detect
/// an existing instance. `std::process::exit(0)` is deliberate — it
/// tells launchd (configured with `KeepAlive.SuccessfulExit=false`)
/// that this invocation is a normal termination and should **not**
/// trigger an automatic retry. If the user's launchd config does
/// not yet carry that key, launchd will keep retrying but each retry
/// will hit the same exit 0 path until the existing instance dies
/// naturally — so the worst case is log noise, not a spin.
fn emit_existing_instance_exit(port: u16, project_root: String, daemon_started_at: &str) -> ! {
    tracing::warn!(
        port,
        project_root = %project_root,
        git_sha = crate::build_info::BUILD_GIT_SHA,
        daemon_started_at = daemon_started_at,
        existing_instance_detected = true,
        "another CodeLens MCP daemon is already listening on this port — deferring to existing instance (exit 0)"
    );
    std::process::exit(0);
}

/// Start the HTTP server with Streamable HTTP transport.
#[tokio::main]
pub(crate) async fn run_http(state: Arc<AppState>, config: HttpServerConfig) -> Result<()> {
    state
        .metrics()
        .record_transport_session(config.transport_label());
    let addr = config.socket_addr();

    // Phase 4d §single-instance guard: probe before bind. Catches
    // the common duplicate-launcher case (two launchd-style sources
    // racing for the same port) with a sub-second check instead of
    // letting both processes reach `bind()` and stack bind errors
    // in the append-only daemon log.
    if addr_is_occupied(addr).await {
        let project_root = state.current_project_scope();
        let daemon_started_at = state.daemon_started_at().to_string();
        emit_existing_instance_exit(config.port, project_root, &daemon_started_at);
    }

    let app = build_router(state.clone());

    // Session cleanup background task
    let cleanup_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Some(store) = &cleanup_state.session_store {
                let removed = store.cleanup();
                if removed > 0 {
                    tracing::debug!(removed, "expired sessions cleaned up");
                }
            }
        }
    });

    tracing::info!(
        transport = config.transport_label(),
        "CodeLens MCP server listening on {}://{addr}/mcp",
        config.transport_label()
    );

    // Phase 4d §single-instance guard: even with pre-probe, a
    // race window exists where a second instance bound the port
    // between our probe and our bind. `bind` will then fail with
    // `AddrInUse`. We catch that specific error and re-route to
    // the same graceful exit 0 path.
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            let project_root = state.current_project_scope();
            let daemon_started_at = state.daemon_started_at().to_string();
            tracing::warn!(
                port = config.port,
                project_root = %project_root,
                git_sha = crate::build_info::BUILD_GIT_SHA,
                daemon_started_at = %daemon_started_at,
                "bind raced with existing instance (AddrInUse after probe) — deferring"
            );
            emit_existing_instance_exit(config.port, project_root, &daemon_started_at);
        }
        Err(error) => {
            tracing::error!(
                port = config.port,
                project_root = %state.current_project_scope(),
                git_sha = crate::build_info::BUILD_GIT_SHA,
                daemon_started_at = state.daemon_started_at(),
                error = %error,
                "failed to bind CodeLens MCP HTTP listener"
            );
            return Err(error.into());
        }
    };
    if let Some(tls) = config.tls {
        let rustls_config = load_rustls_config(&tls).await?;
        let bound = listener.local_addr()?;
        tracing::info!("HTTP server accepting HTTPS connections on {}", bound);
        axum_server::from_tcp_rustls(listener.into_std()?, rustls_config)?
            .serve(app.into_make_service())
            .await?;
    } else {
        axum::serve(listener, app).await.map_err(|error| {
            tracing::error!(
                port = config.port,
                project_root = %state.current_project_scope(),
                git_sha = crate::build_info::BUILD_GIT_SHA,
                daemon_started_at = state.daemon_started_at(),
                error = %error,
                "CodeLens MCP HTTP server exited with error"
            );
            error
        })?;
    }
    Ok(())
}

// ── POST /mcp ─────────────────────────────────────────────────────────

async fn mcp_post_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !origin_is_permitted(&headers) {
        return (StatusCode::FORBIDDEN, "Origin not permitted").into_response();
    }
    if !protocol_version_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, "Unsupported MCP-Protocol-Version").into_response();
    }
    let principal_id = match authenticate_request(&state, &headers).await {
        Ok(principal_id) => principal_id,
        Err(response) => return response,
    };

    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let accept = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    // Parse JSON-RPC request
    let request = match serde_json::from_str::<JsonRpcRequest>(&body) {
        Ok(req) => req,
        Err(error) => {
            let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {error}"));
            return into_mcp_response(resp, accept, None, state.daemon_mode().as_str());
        }
    };

    let is_initialize = request.method == "initialize";
    let initialize_metadata = if is_initialize {
        extract_initialize_metadata(&request, &headers)
    } else {
        None
    };
    let mut request = request;
    sanitize_http_session_metadata(&mut request);

    // Validate / recover session for non-initialize requests (#300/#301).
    // Under the default lenient policy a UUID-shaped, non-tombstoned unknown
    // session is resurrected so a daemon restart / idle timeout never locks the
    // whole client out; strict policy keeps the spec 404 envelope.
    let mut session_resurrected = false;
    if !is_initialize
        && let Some(ref sid) = session_id
        && let Some(store) = &state.session_store
        && store.get(sid).is_none()
    {
        match store.policy() {
            crate::server::session::SessionPolicy::Strict => return unknown_session_response(),
            crate::server::session::SessionPolicy::Lenient => {
                let seed = crate::server::session::SessionSeed::from_headers(&headers);
                match store.get_or_resurrect(sid, &seed) {
                    Some((session, true)) => {
                        session_resurrected = true;
                        // Issue #252: a freshly-resurrected SessionState defaults
                        // to the Balanced preset. When the request carried no
                        // `x-codelens-profile` seed, `apply_seed` leaves that
                        // default in place, so a resurrected session on a
                        // `--profile`-launched daemon silently drops to Balanced
                        // instead of the daemon's startup surface — the same
                        // surface a fresh `initialize` inherits via
                        // `initialize_surface_and_budget`. Mirror that no-profile
                        // branch here so startup `--profile` survives idle-timeout
                        // / restart resurrection. A seed-provided profile already
                        // set the surface in `apply_seed`, so only fill the gap.
                        if seed.requested_profile.is_none() {
                            session.set_surface(*state.surface());
                            session.set_token_budget(state.token_budget());
                        }
                        tracing::info!(
                            session_id = sid.as_str(),
                            "resurrected stale MCP session (#300): daemon restart or idle timeout — recovered without lockout"
                        );
                    }
                    // A concurrent request already recreated it — proceed.
                    Some((_session, false)) => {}
                    // None: non-UUID id, tombstoned (explicit DELETE), or the map
                    // is full of active sessions — keep the strict envelope.
                    None => return unknown_session_response(),
                }
            }
        }
    }

    // Inject session metadata into request params based on method. A captured
    // project remains pinned against runtime-cache eviction through dispatch.
    let mut request_project_pin = None;
    if !is_initialize
        && let Some(ref sid) = session_id
        && let Some(store) = &state.session_store
    {
        // #351/#386: capture the recurring project header and the metadata
        // snapshot in the injection path. The session lock covers both the
        // header update and snapshot, so concurrent A/B requests cannot execute
        // against one another's project.
        let project_header = super::transport_http_support::project_header_value(&headers);
        request_project_pin = match request.method.as_str() {
            "tools/call" => super::session_injection::inject_tool_call_session(
                &mut request,
                sid,
                store,
                project_header.as_deref(),
            ),
            "tools/list" => super::session_injection::inject_tools_list_session(
                &mut request,
                sid,
                store,
                &state,
                project_header.as_deref(),
            ),
            "resources/read" => super::session_injection::inject_resources_read_session(
                &mut request,
                sid,
                store,
                &state,
                project_header.as_deref(),
            ),
            _ => store
                .client_metadata_for_project_header(sid, project_header.as_deref())
                .map(|snapshot| snapshot.project_pin),
        };
        if request_project_pin.is_none() {
            // The session vanished after the resurrection gate but before the
            // authoritative request snapshot. Fail closed instead of dispatching
            // without transport-owned session metadata.
            return unknown_session_response();
        }
    }

    // Populate the final metadata container only after any session injection
    // has run. Provenance itself is carried by the execution scope below, never
    // by a caller-visible JSON marker.
    if let Some(principal_id) = principal_id.as_deref() {
        inject_principal_id_into_params(&mut request, principal_id);
    }

    // Dispatch via spawn_blocking (handle_request is synchronous)
    let state_clone = Arc::clone(&state);
    let response = tokio::task::spawn_blocking(move || {
        // Keep the captured project pinned inside the blocking task itself.
        // If the HTTP handler future is cancelled, Tokio detaches this task;
        // ownership here still protects the project until dispatch completes.
        let _request_project_pin = request_project_pin;
        crate::session_context::with_http_transport_context(|| {
            handle_request(&state_clone, request)
        })
    })
    .await
    .unwrap_or_else(|e| {
        Some(JsonRpcResponse::error(
            None,
            -32603,
            format!("Internal error: {e}"),
        ))
    });

    // Create session on initialize
    let initialize_session = if is_initialize {
        let (initial_surface, initial_budget) =
            initialize_surface_and_budget(&state, initialize_metadata.as_ref());
        create_initialize_session(
            state.session_store.as_ref(),
            session_id.as_deref(),
            initialize_metadata,
            &state.current_project_scope(),
            initial_surface,
            initial_budget,
        )
    } else {
        None
    };

    let Some(resp) = response else {
        // Spec §"Sending Messages to the Server" item 4: a notification or
        // response body with no JSON-RPC reply returns 202 Accepted, not 204.
        let mut accepted = StatusCode::ACCEPTED.into_response();
        if session_resurrected {
            accepted.headers_mut().insert(
                "x-codelens-session-resurrected",
                HeaderValue::from_static("1"),
            );
        }
        return accepted;
    };

    let mut response = into_mcp_response(
        resp,
        accept,
        initialize_session.as_ref(),
        state.daemon_mode().as_str(),
    );
    if session_resurrected {
        response.headers_mut().insert(
            "x-codelens-session-resurrected",
            HeaderValue::from_static("1"),
        );
    }
    response
}

// ── GET /mcp (persistent SSE stream) ──────────────────────────────────

async fn mcp_get_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !origin_is_permitted(&headers) {
        return (StatusCode::FORBIDDEN, "Origin not permitted").into_response();
    }
    if !protocol_version_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, "Unsupported MCP-Protocol-Version").into_response();
    }
    if let Err(response) = authenticate_request(&state, &headers).await {
        return response;
    }

    let session_id = headers.get("mcp-session-id").and_then(|v| v.to_str().ok());

    let Some(session_id) = session_id else {
        return (StatusCode::BAD_REQUEST, "Missing Mcp-Session-Id header").into_response();
    };

    let store = match &state.session_store {
        Some(s) => s,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Session store not initialized",
            )
                .into_response();
        }
    };

    let Some(session) = store.get(session_id) else {
        return unknown_session_response();
    };

    // Create SSE channel and store the sender in the session
    let (tx, rx) = tokio::sync::mpsc::channel::<SseEvent>(32);
    {
        if let Ok(mut sse_tx) = session.sse_tx.lock() {
            *sse_tx = Some(tx);
        }
    }

    // Map SseEvent → axum SSE Event
    let stream = ReceiverStream::new(rx).map(|event| {
        Ok::<_, Infallible>(Event::default().event(event.event_type).data(event.data))
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

// ── DELETE /mcp (session termination) ─────────────────────────────────

async fn mcp_delete_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !origin_is_permitted(&headers) {
        return (StatusCode::FORBIDDEN, "Origin not permitted").into_response();
    }
    if !protocol_version_header_ok(&headers) {
        return (StatusCode::BAD_REQUEST, "Unsupported MCP-Protocol-Version").into_response();
    }
    if let Err(response) = authenticate_request(&state, &headers).await {
        return response;
    }
    if let Some(id) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok())
        && let Some(store) = &state.session_store
    {
        // Guard #3: tombstone (not just remove) so an explicitly-terminated id
        // is refused resurrection under the lenient policy — DELETE stays
        // authoritative.
        store.mark_tombstone(id);
        tracing::debug!(session_id = id, "session terminated by client");
    }
    StatusCode::NO_CONTENT.into_response()
}

// ── Phase 4d: single-instance port guard tests ─────────────────────

#[cfg(test)]
mod single_instance_guard_tests {
    use super::port_is_occupied;

    /// Pick a port that's almost certainly free by binding 127.0.0.1:0
    /// (let the kernel choose) and returning the allocated number.
    /// The listener is dropped before we return, so by the time the
    /// caller probes, the port is definitely free.
    async fn reserve_free_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("kernel should hand out a free ephemeral port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    /// Phase 4d AC3: an empty port must be reported as not-occupied.
    /// This is the normal startup path — daemon probes, sees nothing,
    /// proceeds to bind.
    #[tokio::test]
    async fn port_is_occupied_returns_false_for_empty_port() {
        let port = reserve_free_port().await;
        assert!(
            !port_is_occupied(port).await,
            "empty port {port} must be reported as not-occupied"
        );
    }

    /// Phase 4d AC1: a port with a live listener must be reported as
    /// occupied. This is the single-instance trigger — a second
    /// launcher probing the same port must detect the existing
    /// daemon before calling `bind()`.
    #[tokio::test]
    async fn port_is_occupied_returns_true_for_live_listener() {
        // Bind a real listener on an ephemeral port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("kernel should hand out a free ephemeral port");
        let port = listener.local_addr().unwrap().port();

        // Spawn a minimal accept loop so the probe's TcpStream::connect
        // actually succeeds rather than hitting a race against the
        // kernel's accept queue. We just accept and immediately drop —
        // the probe only cares about the initial 3-way handshake.
        let accept_handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });

        assert!(
            port_is_occupied(port).await,
            "live listener on port {port} must be reported as occupied"
        );

        // Clean up the spawned accept task.
        accept_handle.abort();
    }

    /// Phase 4d AC1 edge case: unreachable ports (e.g. port 0, which
    /// is a reserved wildcard) should not panic and should return
    /// false so normal startup proceeds. The probe is conservative
    /// by design — bind will catch any real problem.
    #[tokio::test]
    async fn port_is_occupied_handles_port_zero_gracefully() {
        // Port 0 is reserved and unbindable via TcpStream::connect.
        // Should return false (not-occupied) without panicking.
        let _ = port_is_occupied(0).await;
    }
}

#[cfg(test)]
mod principal_injection_tests {
    use super::*;
    use crate::protocol::JsonRpcRequest;

    fn parse(body: &str) -> JsonRpcRequest {
        serde_json::from_str(body).expect("test request body must be valid json-rpc")
    }

    #[test]
    fn injects_principal_id_into_tools_call_arguments() {
        let mut request = parse(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"x"}}}"#,
        );
        inject_principal_id_into_params(&mut request, "alice@example.com");
        let arguments = request
            .params
            .as_ref()
            .and_then(|p| p.get("arguments"))
            .expect("arguments must remain present");
        assert_eq!(arguments["_session_principal_id"], "alice@example.com");
        // Existing fields untouched.
        assert_eq!(arguments["relative_path"], "x");
    }

    #[test]
    fn injects_principal_id_into_tools_list_params() {
        let mut request = parse(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#);
        inject_principal_id_into_params(&mut request, "alice@example.com");
        assert_eq!(
            request
                .params
                .as_ref()
                .and_then(|params| params.get("_session_principal_id")),
            Some(&serde_json::json!("alice@example.com")),
            "tools/list must carry the authenticated principal for role filtering"
        );
    }

    #[test]
    fn injects_principal_when_arguments_are_missing() {
        let mut request = parse(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_text_file"}}"#,
        );
        inject_principal_id_into_params(&mut request, "alice@example.com");
        assert_eq!(
            request
                .params
                .as_ref()
                .and_then(|params| params.get("arguments"))
                .and_then(|arguments| arguments.get("_session_principal_id")),
            Some(&serde_json::json!("alice@example.com")),
            "the transport must create an authoritative arguments object"
        );
    }

    #[test]
    fn http_transport_strips_forged_session_fields_without_json_provenance_marker() {
        let mut request = parse(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"x","_session_id":"victim","_session_principal_id":"admin","_session_trusted_client":true,"_session_project_path":"/tmp/other","_session_transport_authenticated":true}}}"#,
        );

        sanitize_http_session_metadata(&mut request);
        let arguments = request
            .params
            .as_ref()
            .and_then(|params| params.get("arguments"))
            .expect("tool arguments");

        assert_eq!(arguments["relative_path"], "x");
        assert!(arguments.get("_session_id").is_none());
        assert!(arguments.get("_session_principal_id").is_none());
        assert!(arguments.get("_session_trusted_client").is_none());
        assert!(arguments.get("_session_project_path").is_none());
        assert!(arguments.get("_session_transport_authenticated").is_none());
    }
}
