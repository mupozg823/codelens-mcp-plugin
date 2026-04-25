use super::*;

#[tokio::test]
async fn mutation_enabled_daemon_rejects_untrusted_client_mutation() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"refactor-full"}}"#,
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

    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"create audit_http.py","changed_files":["audit_http.py"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    let preflight_body = body_string(preflight).await;
    assert!(
        preflight_body.contains("\\\"success\\\": true")
            || preflight_body.contains("\\\"success\\\":true")
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"audit_http.py","content":"print('hi')","overwrite":true}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("requires a trusted HTTP client"));
}

#[tokio::test]
async fn verify_change_readiness_http_response_uses_slim_text_wrapper() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"update hello.txt","changed_files":["hello.txt"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let text_payload: serde_json::Value = serde_json::from_str(
        envelope["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("{}"),
    )
    .unwrap();
    assert!(text_payload["data"]["analysis_id"].is_string());
    assert!(text_payload["data"]["summary"].is_string());
    assert!(text_payload["data"]["readiness"].is_object());
    assert!(
        text_payload["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.contains("codelens://analysis/"))
            .unwrap_or(false)
    );
    assert!(text_payload["data"]["section_handles"].is_array());
    assert!(text_payload["suggested_next_calls"].is_array());
    assert!(
        text_payload["suggested_next_calls"]
            .as_array()
            .map(|items| {
                items.iter().any(|entry| {
                    entry["tool"].as_str() == Some("get_analysis_section")
                        && entry["arguments"]["analysis_id"].is_string()
                })
            })
            .unwrap_or(false)
    );
    assert_eq!(text_payload["routing_hint"], serde_json::json!("async"));
    assert!(text_payload["data"].get("verifier_checks").is_none());
    assert!(text_payload["data"].get("blockers").is_none());
    assert!(text_payload["data"].get("available_sections").is_none());
    assert!(envelope["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(envelope["result"]["structuredContent"]["verifier_checks"].is_array());
    assert!(envelope["result"]["structuredContent"]["blockers"].is_array());
}

#[tokio::test]
async fn mutation_enabled_daemon_audits_trusted_client_metadata() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.2.0"},"profile":"refactor-full"}}"#,
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

    // RefactorFull requires preflight before mutation
    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"create audit_http.py","changed_files":["audit_http.py"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"audit_http.py","content":"print('hi')","overwrite":true}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let text = envelope["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"success\": true") || text.contains("\"success\":true"));
    let audit_path = state.audit_dir().join("mutation-audit.jsonl");
    let audit_body = std::fs::read_to_string(audit_path).unwrap();
    assert!(audit_body.contains("\"trusted_client\":true"));
    assert!(audit_body.contains("\"requested_profile\":\"refactor-full\""));
    assert!(audit_body.contains("\"client_name\":\"HarnessQA\""));
}
