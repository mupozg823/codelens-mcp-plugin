use super::*;

#[tokio::test]
async fn initialize_with_existing_session_resumes_same_session() {
    let state = test_state();
    let app = build_router(state.clone());
    let first = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = first
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let second = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.2.0"},"profile":"planner-readonly"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        second
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok()),
        Some(sid.as_str())
    );
    assert_eq!(
        second
            .headers()
            .get("x-codelens-session-resumed")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    let body = body_string(second).await;
    assert!(body.contains("\"resumed\":true"));
    assert!(body.contains(&sid));

    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    assert_eq!(session.resume_count(), 1);
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_version.as_deref(), Some("2.2.0"));
    assert_eq!(
        metadata.requested_profile.as_deref(),
        Some("planner-readonly")
    );
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
    assert!(
        body.contains("-32700"),
        "should return JSON-RPC parse error code"
    );
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
    assert!(
        body.contains("get_ranked_context"),
        "tools/list should return tools"
    );
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
async fn server_card_exposes_daemon_mode() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::ReadOnly);
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/mcp.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"daemon_mode\": \"read-only\""));
    assert!(body.contains("session-client-metadata"));
    assert!(body.contains("\"surface_manifest\""));
}

#[tokio::test]
async fn server_card_advertises_supported_protocol_versions() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/mcp.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""latestProtocolVersion": "2025-11-25""#),
        "card should pin latest version, got: {body}"
    );
    assert!(
        body.contains(r#""supportedProtocolVersions""#)
            && body.contains(r#""2025-11-25""#)
            && body.contains(r#""2025-03-26""#)
            && body.contains(r#""2025-06-18""#),
        "card should list supported versions, got: {body}"
    );
}

#[tokio::test]
async fn post_notification_returns_accepted() {
    // Spec §"Sending Messages to the Server" item 4: JSON-RPC notifications
    // (no `id`) and responses MUST yield 202 Accepted, not 204 No Content.
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

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
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

#[tokio::test]
async fn set_profile_emits_tools_list_changed_notification_over_sse() {
    let app = build_router(test_state());
    let init = app
        .clone()
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
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let _ = body_string(init).await;

    let sse = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sse.status(), StatusCode::OK);

    let set_profile = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_profile.status(), StatusCode::OK);

    let chunk = next_sse_chunk(sse).await;
    assert!(
        chunk.contains("event: message"),
        "unexpected SSE event envelope: {chunk}"
    );
    assert!(
        chunk.contains(r#""method":"notifications/tools/list_changed""#),
        "expected tools/list_changed notification, got: {chunk}"
    );
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
    assert!(body.contains("get_ranked_context"));

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
