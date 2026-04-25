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
