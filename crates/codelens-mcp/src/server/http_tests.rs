//! Integration tests for the Streamable HTTP transport.
//!
//! Uses tower::ServiceExt::oneshot to test axum handlers without starting a real server.
//! Run with: `cargo test --features http`

#![cfg(feature = "http")]

use super::session::SessionStore;
use super::transport_http::build_router;
use crate::{AppState, state::RuntimeCompatMode};
use axum::http::{Request, StatusCode};
use codelens_engine::ProjectRoot;
use http_body_util::BodyExt;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

fn test_state() -> Arc<AppState> {
    let dir = std::env::temp_dir().join(format!(
        "codelens-http-test-{}-{:?}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id(),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
    let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
    let state = AppState::new(project, crate::tool_defs::ToolPreset::Balanced);
    Arc::new(state.with_session_store())
}

fn test_state_with_compat(compat_mode: RuntimeCompatMode) -> Arc<AppState> {
    let state = test_state();
    state.configure_compat_mode(compat_mode);
    state
}

fn temp_project_dir(name: &str) -> std::path::PathBuf {
    crate::test_helpers::fixtures::temp_project_dir(name)
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn next_sse_chunk(resp: axum::response::Response) -> String {
    let mut body = resp.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .expect("timed out waiting for SSE frame")
        .expect("SSE stream ended before first frame")
        .expect("SSE frame error");
    let bytes = frame.into_data().expect("expected SSE data frame");
    String::from_utf8(bytes.to_vec()).expect("SSE chunk should be utf-8")
}

fn first_resource_text(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("result").cloned())
        .and_then(|result| result.get("contents").cloned())
        .and_then(|contents| contents.as_array().cloned())
        .and_then(|contents| contents.first().cloned())
        .and_then(|content| content.get("text").cloned())
        .and_then(|text| text.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

fn first_tool_payload(body: &str) -> serde_json::Value {
    let value = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let mut payload = value
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(|contents| contents.as_array())
        .and_then(|contents| contents.first())
        .and_then(|content| content.get("text"))
        .and_then(|text| text.as_str())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .unwrap_or_default();

    if let Some(structured_content) = value
        .get("result")
        .and_then(|result| result.get("structuredContent"))
        .cloned()
    {
        if !payload.is_object() {
            payload = serde_json::json!({});
        }
        payload
            .as_object_mut()
            .expect("payload object")
            .insert("data".to_owned(), structured_content);
    }

    payload
}

// ── Bearer/JWKS auth ─────────────────────────────────────────────────

mod analysis_job_tests;
mod auth_tests;
mod deferred_expansion_tests;
mod deferred_tests;
mod lifecycle_project_tests;
mod lifecycle_tests;
mod mutation_tests;
mod protocol_tests;
mod protocol_version_tests;
mod session_store_tests;
mod tools_list_tests;
