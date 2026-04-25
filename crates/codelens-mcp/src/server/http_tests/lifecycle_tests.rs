use super::*;

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
async fn initialize_advertises_tools_list_changed_in_http_mode() {
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
    let body = body_string(resp).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("initialize json");
    assert_eq!(
        value["result"]["capabilities"]["tools"]["listChanged"],
        serde_json::json!(true)
    );
    assert_eq!(
        value["result"]["capabilities"]["resources"]["listChanged"],
        serde_json::json!(false)
    );
}

#[tokio::test]
async fn anthropic_remote_compat_initialize_advertises_tools_only() {
    let app = build_router(test_state_with_compat(ServerCompatMode::AnthropicRemote));
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
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let capabilities = &value["result"]["capabilities"];
    assert!(capabilities.get("tools").is_some());
    assert!(capabilities.get("resources").is_none(), "body: {body}");
    assert!(capabilities.get("prompts").is_none(), "body: {body}");
}

#[tokio::test]
async fn anthropic_remote_compat_hides_resources_and_prompts_methods() {
    let app = build_router(test_state_with_compat(ServerCompatMode::AnthropicRemote));
    for method in ["resources/list", "prompts/list"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":1,"method":"{method}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["error"]["code"], -32601, "body: {body}");
    }
}

#[tokio::test]
async fn anthropic_remote_compat_tools_list_uses_connector_safe_shape() {
    let app = build_router(test_state_with_compat(ServerCompatMode::AnthropicRemote));
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
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let result = value["result"].as_object().expect("result object");
    assert!(result.contains_key("tools"), "body: {body}");
    assert_eq!(
        result.len(),
        1,
        "connector-safe result should only expose tools: {body}"
    );
    let first_tool = result["tools"]
        .as_array()
        .and_then(|tools| tools.first())
        .and_then(|tool| tool.as_object())
        .expect("first tool object");
    assert!(first_tool.contains_key("name"));
    assert!(first_tool.contains_key("description"));
    assert!(first_tool.contains_key("inputSchema"));
    assert!(!first_tool.contains_key("_meta"));
}

#[tokio::test]
async fn initialize_persists_client_metadata() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_name.as_deref(), Some("HarnessQA"));
    assert_eq!(metadata.client_version.as_deref(), Some("2.1.0"));
    assert_eq!(
        metadata.requested_profile.as_deref(),
        Some("reviewer-graph")
    );
    assert_eq!(metadata.trusted_client, Some(true));
    assert_eq!(metadata.deferred_tool_loading, None);
    assert!(metadata.loaded_namespaces.is_empty());
    assert!(metadata.loaded_tiers.is_empty());
    assert_eq!(metadata.full_tool_exposure, None);
}

#[tokio::test]
async fn initialize_profile_sets_http_session_surface_and_tools_list() {
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
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph"}}"#,
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

    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    assert_eq!(
        session.surface(),
        crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::ReviewerGraph)
    );
    assert_eq!(
        session.token_budget(),
        crate::tool_defs::default_budget_for_profile(crate::tool_defs::ToolProfile::ReviewerGraph)
    );

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
    assert!(body.contains("\"active_surface\":\"reviewer-graph\""));
    assert!(body.contains("\"get_ranked_context\""));
    assert!(body.contains("\"get_callers\""));
    assert!(body.contains("\"start_analysis_job\""));
    assert!(!body.contains("\"review_architecture\""));
    assert!(!body.contains("\"review_changes\""));
    assert!(!body.contains("\"cleanup_duplicate_logic\""));
    assert!(!body.contains("\"analyze_change_impact\""));
    assert!(!body.contains("\"audit_security_context\""));
    assert!(!body.contains("\"assess_change_readiness\""));
}

#[tokio::test]
async fn initialize_persists_deferred_loading_preference() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(true));
    assert!(metadata.loaded_namespaces.is_empty());
    assert!(metadata.loaded_tiers.is_empty());
    assert_eq!(metadata.full_tool_exposure, None);
}

#[tokio::test]
async fn initialize_codex_defaults_to_deferred_loading() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
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

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(true));
}

#[tokio::test]
async fn initialize_claude_defaults_to_full_contract() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
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

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(false));
}

#[tokio::test]
async fn codex_session_client_name_affects_activate_project_budget() {
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

    let primitive_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(primitive_list.status(), StatusCode::OK);

    let activate = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"activate_project","arguments":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(activate.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(activate).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["auto_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(payload["data"]["auto_budget"], serde_json::json!(6000));
}
