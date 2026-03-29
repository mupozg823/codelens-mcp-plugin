#[cfg(feature = "http")]
use super::router::handle_request;
#[cfg(feature = "http")]
use crate::protocol::JsonRpcRequest;
#[cfg(feature = "http")]
use crate::AppState;
#[cfg(feature = "http")]
use anyhow::Result;
#[cfg(feature = "http")]
use serde_json::json;
#[cfg(feature = "http")]
use std::sync::Arc;

#[cfg(feature = "http")]
#[tokio::main]
pub(crate) async fn run_http(state: Arc<AppState>, port: u16) -> Result<()> {
    use axum::{Router, routing::post};

    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("CodeLens MCP HTTP server listening on http://{addr}/mcp");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(feature = "http")]
async fn mcp_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    body: String,
) -> (axum::http::StatusCode, String) {
    let response = match serde_json::from_str::<JsonRpcRequest>(&body) {
        Ok(request) => handle_request(&state, request),
        Err(error) => Some(crate::protocol::JsonRpcResponse::error(
            None,
            -32700,
            format!("Parse error: {error}"),
        )),
    };
    match response {
        Some(resp) => match serde_json::to_string(&resp) {
            Ok(json) => (axum::http::StatusCode::OK, json),
            Err(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::to_string(&json!({"error": e.to_string()}))
                    .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_owned()),
            ),
        },
        // Notification — HTTP 204 No Content
        None => (axum::http::StatusCode::NO_CONTENT, String::new()),
    }
}
