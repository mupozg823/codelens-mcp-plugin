use super::*;

#[tokio::test]
async fn deferred_tools_list_uses_preferred_namespaces_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
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
    assert!(body.contains("\"deferred_loading_active\":true"));
    assert!(
        body.contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]")
    );
    assert!(body.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(body.contains("\"loaded_namespaces\":[]"));
    assert!(body.contains("\"loaded_tiers\":[]"));
    assert!(body.contains("\"review_architecture\""));
    assert!(body.contains("\"review_changes\""));
    assert!(body.contains("\"cleanup_duplicate_logic\""));
    assert!(!body.contains("\"analyze_change_impact\""));
    assert!(!body.contains("\"audit_security_context\""));
    assert!(!body.contains("\"find_symbol\""));
    assert!(!body.contains("\"read_file\""));
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tool_names = envelope["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names.iter().take(3).cloned().collect::<Vec<_>>(),
        vec![
            "review_architecture".to_owned(),
            "review_changes".to_owned(),
            "cleanup_duplicate_logic".to_owned(),
        ]
    );
}

#[tokio::test]
async fn refactor_deferred_tools_list_starts_preview_first_for_session() {
    let state = test_state();
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
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"refactor-full","deferredToolLoading":true}}"#,
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
    assert!(body.contains("\"deferred_loading_active\":true"));
    assert!(body.contains("\"preferred_namespaces\":[\"reports\",\"session\"]"));
    assert!(body.contains("\"tool_count\":"));
    assert!(body.contains("\"plan_safe_refactor\""));
    assert!(body.contains("\"review_changes\""));
    assert!(body.contains("\"trace_request_path\""));
    assert!(!body.contains("\"analyze_change_impact\""));
    assert!(body.contains("\"activate_project\""));
    assert!(body.contains("\"set_profile\""));
    assert!(!body.contains("\"name\":\"rename_symbol\""));
    assert!(!body.contains("\"name\":\"replace_symbol_body\""));
    assert!(!body.contains("\"name\":\"refactor_extract_function\""));
    assert!(!body.contains("\"name\":\"verify_change_readiness\""));
    assert!(!body.contains("\"name\":\"refactor_safety_report\""));
    assert!(!body.contains("\"name\":\"safe_rename_report\""));
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tool_names = envelope["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names.iter().take(3).cloned().collect::<Vec<_>>(),
        vec![
            "plan_safe_refactor".to_owned(),
            "review_changes".to_owned(),
            "trace_request_path".to_owned(),
        ]
    );
}

#[tokio::test]
async fn deferred_session_blocks_hidden_tool_calls_until_namespace_is_loaded() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let file_path = state.project().as_path().join("deferred-hidden.py");
    std::fs::write(&file_path, "def alpha():\n    return 1\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
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

    // get_file_diagnostics is in reviewer-graph but namespace "lsp" is not preferred,
    // so it should be hidden by deferred namespace loading.
    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}"}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(blocked.status(), StatusCode::OK);
    let blocked_body = body_string(blocked).await;
    assert!(blocked_body.contains("hidden by deferred loading"));
}

#[tokio::test]
async fn deferred_namespace_load_allows_listed_graph_tool_call() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
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

    let graph_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"namespace":"graph"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(graph_list.status(), StatusCode::OK);
    let graph_body = body_string(graph_list).await;
    assert!(graph_body.contains("\"selected_namespace\":\"graph\""));
    assert!(graph_body.contains("\"get_callers\""));

    let callers = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_callers","arguments":{"function_name":"missing_smoke_target","max_results":1}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(callers.status(), StatusCode::OK);
    let callers_body = body_string(callers).await;
    assert!(!callers_body.contains("hidden by deferred loading"));
}
