//! Integration tests for the Streamable HTTP transport.
//!
//! Uses tower::ServiceExt::oneshot to test axum handlers without starting a real server.
//! Run with: `cargo test --features http`

#![cfg(feature = "http")]

use super::session::SessionStore;
use super::transport_http::build_router;
use crate::AppState;
use axum::http::{Request, StatusCode};
use codelens_core::ProjectRoot;
use http_body_util::BodyExt;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

fn test_state() -> Arc<AppState> {
    let dir = std::env::temp_dir().join(format!(
        "codelens-http-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
    let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
    let state = AppState::new(project, crate::tool_defs::ToolPreset::Balanced);
    Arc::new(state.with_session_store())
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ── POST /mcp ────────────────────────────────────────────────────────

#[tokio::test]
async fn post_initialize_returns_session_id() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("mcp-session-id").is_some(),
        "initialize should return Mcp-Session-Id header"
    );
    let body = body_string(resp).await;
    assert!(body.contains("\"jsonrpc\":\"2.0\""));
    assert!(body.contains("\"id\":1"));
}

#[tokio::test]
async fn post_invalid_json_returns_parse_error() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("-32700"), "should return JSON-RPC parse error code");
}

#[tokio::test]
async fn post_non_initialize_without_session_works() {
    // Non-initialize requests without session ID should still work
    // (session validation only rejects unknown session IDs, not missing ones)
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("get_symbols_overview"), "tools/list should return tools");
}

#[tokio::test]
async fn post_unknown_session_returns_not_found() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", "nonexistent-session-id")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_with_sse_accept_returns_event_stream() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("mcp-session-id").is_some(),
        "SSE response should also include session ID"
    );
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("text/event-stream"),
        "Accept: text/event-stream should return SSE content-type, got: {ct}"
    );
}

#[tokio::test]
async fn post_notification_returns_no_content() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// ── GET /mcp (SSE stream) ────────────────────────────────────────────

#[tokio::test]
async fn get_without_session_returns_bad_request() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_with_unknown_session_returns_not_found() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .header("mcp-session-id", "bogus-id")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── DELETE /mcp (session termination) ────────────────────────────────

#[tokio::test]
async fn delete_returns_no_content() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", "any-id")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_removes_session() {
    let state = test_state();
    let session = state.session_store.as_ref().unwrap().create();
    let sid = session.id.clone();

    // Verify session exists
    assert!(state.session_store.as_ref().unwrap().get(&sid).is_some());

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(
        state.session_store.as_ref().unwrap().get(&sid).is_none(),
        "session should be removed after DELETE"
    );
}

// ── Session lifecycle ────────────────────────────────────────────────

#[tokio::test]
async fn full_session_lifecycle() {
    let state = test_state();
    let app = build_router(state.clone());

    // 1. Initialize — get session ID
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!sid.is_empty());

    // 2. Use session for a tool call
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("get_symbols_overview"));

    // 3. Terminate session
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 4. Verify session is gone
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Session store edge cases ─────────────────────────────────────────

#[test]
fn concurrent_session_creation() {
    let store = SessionStore::new(Duration::from_secs(300));
    let sessions: Vec<_> = (0..100).map(|_| store.create()).collect();

    // All IDs unique
    let mut ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 100, "all 100 session IDs should be unique");
    assert_eq!(store.len(), 100);
}

#[test]
fn session_touch_refreshes_expiry() {
    let store = SessionStore::new(Duration::from_millis(50));
    let session = store.create();
    let id = session.id.clone();

    std::thread::sleep(Duration::from_millis(30));
    // Touch should reset the timer
    store.get(&id); // get() calls touch()
    std::thread::sleep(Duration::from_millis(30));

    // 60ms total but touched at 30ms, so 30ms since touch < 50ms timeout
    assert!(
        store.get(&id).is_some(),
        "session should still be alive after touch"
    );
}

#[test]
fn cleanup_only_removes_expired() {
    let store = SessionStore::new(Duration::from_millis(20));
    let s1 = store.create();
    std::thread::sleep(Duration::from_millis(30));
    let s2 = store.create(); // created after sleep, still fresh

    let removed = store.cleanup();
    assert_eq!(removed, 1, "only the expired session should be removed");
    assert!(store.get(&s1.id).is_none());
    assert!(store.get(&s2.id).is_some());
}
