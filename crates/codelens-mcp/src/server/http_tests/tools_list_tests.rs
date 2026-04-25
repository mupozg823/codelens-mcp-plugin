use super::*;

#[tokio::test]
async fn post_get_capabilities_returns_machine_readable_guidance() {
    let state = test_state();
    let app = build_router(state.clone());

    std::fs::write(state.project().as_path().join("notes.unknown"), "hello\n").unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_capabilities","arguments":{"file_path":"notes.unknown"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(resp).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["status"],
        serde_json::json!("unsupported_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["reason_code"],
        serde_json::json!("diagnostics_unsupported_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["recommended_action"],
        serde_json::json!("pass_explicit_lsp_command")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["file_extension"],
        serde_json::json!("unknown")
    );
    assert!(payload["data"]["daemon_binary_drift"]["status"].is_string());
}

#[tokio::test]
async fn codex_session_uses_lean_tools_list_contract_by_default() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
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
    assert!(body.contains("\"client_profile\":\"codex\""));
    assert!(body.contains("\"default_contract_mode\":\"lean\""));
    assert!(body.contains("\"include_output_schema\":false"));
    assert!(body.contains("\"include_annotations\":false"));
    assert!(!body.contains("\"outputSchema\""));
    assert!(!body.contains("\"annotations\""));
    assert!(!body.contains("\"visible_namespaces\""));
}

#[tokio::test]
async fn claude_session_uses_full_tools_list_contract_by_default() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Claude Code","version":"1.0.0"}}}"#,
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
    assert!(body.contains("\"client_profile\":\"claude\""));
    assert!(body.contains("\"default_contract_mode\":\"full\""));
    assert!(body.contains("\"include_output_schema\":true"));
    assert!(body.contains("\"include_annotations\":true"));
    assert!(body.contains("\"outputSchema\""));
    assert!(body.contains("\"annotations\""));
    assert!(body.contains("\"visible_namespaces\""));
}

#[tokio::test]
async fn codex_session_can_restore_tool_annotations_explicitly() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
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

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"includeAnnotations":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"include_annotations\":true"));
    assert!(body.contains("\"annotations\""));
}
