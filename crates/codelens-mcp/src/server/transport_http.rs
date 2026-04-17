//! Streamable HTTP transport for MCP (protocol version 2025-03-26).
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
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tool_defs::{ToolProfile, ToolSurface, default_budget_for_profile};
use anyhow::Result;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Router, routing};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

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
        .with_state(state)
}

/// MCP Server Card — static metadata for agent discovery without a live session.
async fn server_card_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let surface = *state.surface();
    let tool_count = crate::tool_defs::visible_tools(surface).len();
    let daemon_mode = state.daemon_mode().as_str();

    let card = serde_json::json!({
        "name": "codelens-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "description": format!(
            "Compressed context and verification tool for agent harnesses ({daemon_mode} daemon)"
        ),
        "transport": ["stdio", "streamable-http"],
        "capabilities": {
            "tools": true,
            "resources": true,
            "prompts": true,
            "sampling": false
        },
        "tool_count": tool_count,
        "active_surface": surface.as_label(),
        "daemon_mode": daemon_mode,
        "languages": 25,
        "features": [
            "role-based-tool-surfaces",
            "composite-workflow-tools",
            "analysis-handles-and-sections",
            "durable-analysis-jobs",
            "mutation-audit-log",
            "session-resume",
            "session-client-metadata",
            "deferred-tool-loading",
            "tree-sitter-symbol-parsing",
            "import-graph-analysis",
            "lsp-integration",
            "token-budget-control"
        ]
    });

    (
        StatusCode::OK,
        [("content-type", "application/json")],
        serde_json::to_string_pretty(&card).unwrap_or_default(),
    )
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
async fn port_is_occupied(port: u16) -> bool {
    use tokio::net::TcpStream;
    use tokio::time::{Duration, timeout};
    let addr = format!("127.0.0.1:{port}");
    match timeout(Duration::from_millis(200), TcpStream::connect(&addr)).await {
        // Successful connect within 200 ms → something is listening.
        Ok(Ok(_stream)) => true,
        // Any error (ConnectionRefused, etc.) → port is free.
        Ok(Err(_)) => false,
        // Timeout → treat as free (bind will catch a real conflict).
        Err(_) => false,
    }
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
pub(crate) async fn run_http(state: Arc<AppState>, port: u16) -> Result<()> {
    state.metrics().record_transport_session("http");

    // Phase 4d §single-instance guard: probe before bind. Catches
    // the common duplicate-launcher case (two launchd-style sources
    // racing for the same port) with a sub-second check instead of
    // letting both processes reach `bind()` and stack bind errors
    // in the append-only daemon log.
    if port_is_occupied(port).await {
        let project_root = state.current_project_scope();
        let daemon_started_at = state.daemon_started_at().to_string();
        emit_existing_instance_exit(port, project_root, &daemon_started_at);
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

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("CodeLens MCP HTTP server listening on http://{addr}/mcp");

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
                port,
                project_root = %project_root,
                git_sha = crate::build_info::BUILD_GIT_SHA,
                daemon_started_at = %daemon_started_at,
                "bind raced with existing instance (AddrInUse after probe) — deferring"
            );
            emit_existing_instance_exit(port, project_root, &daemon_started_at);
        }
        Err(error) => {
            tracing::error!(
                port,
                project_root = %state.current_project_scope(),
                git_sha = crate::build_info::BUILD_GIT_SHA,
                daemon_started_at = state.daemon_started_at(),
                error = %error,
                "failed to bind CodeLens MCP HTTP listener"
            );
            return Err(error.into());
        }
    };
    axum::serve(listener, app).await.map_err(|error| {
        tracing::error!(
            port,
            project_root = %state.current_project_scope(),
            git_sha = crate::build_info::BUILD_GIT_SHA,
            daemon_started_at = state.daemon_started_at(),
            error = %error,
            "CodeLens MCP HTTP server exited with error"
        );
        error
    })?;
    Ok(())
}

// ── POST /mcp ─────────────────────────────────────────────────────────

async fn mcp_post_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
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

    // Validate session for non-initialize requests
    if !is_initialize
        && let Some(ref sid) = session_id
        && let Some(store) = &state.session_store
        && store.get(sid).is_none()
    {
        return (StatusCode::NOT_FOUND, "Unknown session").into_response();
    }

    // Inject session metadata into request params based on method
    if !is_initialize
        && let Some(ref sid) = session_id
        && let Some(store) = &state.session_store
    {
        match request.method.as_str() {
            "tools/call" => {
                super::session_injection::inject_tool_call_session(&mut request, sid, store);
            }
            "tools/list" => {
                super::session_injection::inject_tools_list_session(
                    &mut request,
                    sid,
                    store,
                    &state,
                );
            }
            "resources/read" => {
                super::session_injection::inject_resources_read_session(
                    &mut request,
                    sid,
                    store,
                    &state,
                );
            }
            _ => {}
        }
    }

    // Dispatch via spawn_blocking (handle_request is synchronous)
    let state_clone = Arc::clone(&state);
    let response = tokio::task::spawn_blocking(move || handle_request(&state_clone, request))
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
        // Notification — no response needed
        return StatusCode::NO_CONTENT.into_response();
    };

    into_mcp_response(
        resp,
        accept,
        initialize_session.as_ref(),
        state.daemon_mode().as_str(),
    )
}

// ── GET /mcp (persistent SSE stream) ──────────────────────────────────

async fn mcp_get_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
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
        return (StatusCode::NOT_FOUND, "Unknown session").into_response();
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

async fn mcp_delete_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> StatusCode {
    if let Some(id) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok())
        && let Some(store) = &state.session_store
    {
        store.remove(id);
        tracing::debug!(session_id = id, "session terminated by client");
    }
    StatusCode::NO_CONTENT
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
