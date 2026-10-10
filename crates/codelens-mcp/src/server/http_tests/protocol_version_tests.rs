use super::*;

#[tokio::test]
async fn post_with_unsupported_protocol_version_header_returns_bad_request() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "1999-01-01")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// The request Claude Code 2.1.296 opens with (measured 2026-10-10). Until this
/// server speaks 2026-07-28, the client has to fall back to `initialize`, and
/// per that spec (§Streamable HTTP, Backward Compatibility) it falls back only
/// when the 400 body "is empty or is not a recognized modern JSON-RPC error";
/// a recognized one makes it retry instead and the server is lost. So this 400
/// must stay a plain-text body, not a spec-shaped `-32022` error.
#[tokio::test]
async fn a_modern_discover_request_gets_a_400_the_client_falls_back_from() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "server/discover")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":0,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"claude-code","version":"2.1.296"}}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_string(resp).await;
    let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
    assert!(
        parsed
            .as_ref()
            .and_then(|value| value.get("error"))
            .is_none(),
        "a JSON-RPC error body would stop the client's legacy fallback, got: {body}"
    );
}

#[tokio::test]
async fn post_with_supported_protocol_version_header_is_accepted() {
    let app = build_router(test_state());
    for version in ["2025-11-25", "2025-06-18", "2025-03-26"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .header("mcp-protocol-version", version)
                    .body(axum::body::Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "version {version}");
    }
}

#[tokio::test]
async fn initialize_echoes_requested_supported_protocol_version() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""protocolVersion":"2025-06-18""#),
        "expected 2025-06-18 echoed, got: {body}"
    );
}

#[tokio::test]
async fn initialize_echoes_latest_supported_protocol_version() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""protocolVersion":"2025-11-25""#),
        "expected 2025-11-25 echoed, got: {body}"
    );
}

#[tokio::test]
async fn initialize_falls_back_to_latest_for_unknown_client_version() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""protocolVersion":"2025-11-25""#),
        "expected latest fallback, got: {body}"
    );
}

#[tokio::test]
async fn post_from_remote_origin_is_forbidden() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("origin", "https://evil.example.com")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn post_from_localhost_origin_is_allowed() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("origin", "http://localhost:5173")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
