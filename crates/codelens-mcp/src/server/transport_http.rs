//! Streamable HTTP transport for MCP (protocol version 2025-03-26).
//!
//! Endpoints:
//! - POST /mcp: JSON-RPC requests. Supports Accept: application/json (default) or text/event-stream (SSE).
//! - GET /mcp: Persistent SSE stream for server→client push (requires Mcp-Session-Id).
//! - DELETE /mcp: Session termination (requires Mcp-Session-Id).

#![cfg(feature = "http")]

use super::router::handle_request;
use super::session::SseEvent;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::AppState;
use anyhow::Result;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Router, routing};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

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
async fn server_card_handler(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let surface = *state.surface();
    let tool_count = crate::tool_defs::visible_tools(surface).len();

    let card = serde_json::json!({
        "name": "codelens-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Compressed context provider for planner/reviewer/refactor agent harnesses",
        "transport": ["stdio", "streamable-http"],
        "capabilities": {
            "tools": true,
            "resources": true,
            "prompts": true,
            "sampling": false
        },
        "tool_count": tool_count,
        "active_surface": surface.as_label(),
        "languages": 25,
        "features": [
            "role-based-tool-surfaces",
            "composite-workflow-tools",
            "analysis-handles-and-sections",
            "durable-analysis-jobs",
            "mutation-audit-log",
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

/// Start the HTTP server with Streamable HTTP transport.
#[tokio::main]
pub(crate) async fn run_http(state: Arc<AppState>, port: u16) -> Result<()> {
    state.metrics().record_transport_session("http");
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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
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
            return json_response(resp, None);
        }
    };

    let is_initialize = request.method == "initialize";

    // Validate session for non-initialize requests
    if !is_initialize {
        if let Some(ref sid) = session_id {
            if let Some(store) = &state.session_store {
                if store.get(sid).is_none() {
                    return (StatusCode::NOT_FOUND, "Unknown session").into_response();
                }
            }
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
    let new_session_id = if is_initialize {
        state
            .session_store
            .as_ref()
            .map(|store| store.create().id.clone())
    } else {
        None
    };

    let Some(resp) = response else {
        // Notification — no response needed
        return StatusCode::NO_CONTENT.into_response();
    };

    // Check if client wants SSE
    if accept.contains("text/event-stream") {
        return sse_single_response(resp, new_session_id);
    }

    json_response(resp, new_session_id)
}

/// Build a standard JSON response with optional Mcp-Session-Id header.
fn json_response(resp: JsonRpcResponse, session_id: Option<String>) -> Response {
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

    let mut response = (
        StatusCode::OK,
        [("content-type", "application/json")],
        json,
    )
        .into_response();

    if let Some(sid) = session_id {
        if let Ok(val) = HeaderValue::from_str(&sid) {
            response.headers_mut().insert("mcp-session-id", val);
        }
    }

    response
}

/// Build an SSE response wrapping a single JSON-RPC response.
fn sse_single_response(resp: JsonRpcResponse, session_id: Option<String>) -> Response {
    let json = serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"error":"serialization"}"#.to_owned());

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(2);
    tokio::spawn(async move {
        let event = Event::default().event("message").data(json);
        let _ = tx.send(Ok(event)).await;
        // Channel drops after single event, ending the stream
    });

    let stream = ReceiverStream::new(rx);
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();

    if let Some(sid) = session_id {
        if let Ok(val) = HeaderValue::from_str(&sid) {
            response.headers_mut().insert("mcp-session-id", val);
        }
    }

    response
}

// ── GET /mcp (persistent SSE stream) ──────────────────────────────────

async fn mcp_get_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok());

    let Some(session_id) = session_id else {
        return (StatusCode::BAD_REQUEST, "Missing Mcp-Session-Id header").into_response();
    };

    let store = match &state.session_store {
        Some(s) => s,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "Session store not initialized").into_response(),
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
        Ok::<_, Infallible>(
            Event::default()
                .event(event.event_type)
                .data(event.data),
        )
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

// ── DELETE /mcp (session termination) ─────────────────────────────────

async fn mcp_delete_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> StatusCode {
    if let Some(id) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) {
        if let Some(store) = &state.session_store {
            store.remove(id);
            tracing::debug!(session_id = id, "session terminated by client");
        }
    }
    StatusCode::NO_CONTENT
}
