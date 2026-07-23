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
    // Phase-2: reviewer bootstrap routes through the verb facades.
    // Assert on the `name` key — bare substrings collide with
    // `"phase":"review"` / `"namespace":"graph"` scaffold values.
    assert!(body.contains("\"name\":\"review\""));
    assert!(body.contains("\"name\":\"graph\""));
    assert!(body.contains("\"name\":\"diagnose\""));
    // 2026-07 tool-surface diet (stage 1): cleanup_duplicate_logic left the
    // reviewer-graph core surface, mirroring the non-http twin
    // `deferred_tools_list_defaults_to_preferred_namespaces_only`. The retained
    // bootstrap member prepare_harness_session anchors the deferred slice.
    assert!(body.contains("\"name\":\"prepare_harness_session\""));
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
            "review".to_owned(),
            "graph".to_owned(),
            "diagnose".to_owned(),
        ]
    );
}

#[tokio::test]
async fn codex_builder_deferred_tools_list_exposes_configured_five_tool_surface() {
    let state = test_state();
    let app = build_router(state);
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"},"profile":"builder-minimal","deferredToolLoading":true}}"#,
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
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tool_names = envelope["result"]["tools"]
        .as_array()
        .expect("tools/list result")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();

    for tool in [
        "prepare_harness_session",
        "get_capabilities",
        "graph",
        "diagnose",
        "review",
    ] {
        assert!(
            tool_names.contains(&tool),
            "Codex five-tool allowlist entry `{tool}` must be advertised by the builder bootstrap: {tool_names:?}"
        );
    }
    assert!(
        tool_names.len() <= 20,
        "Codex builder bootstrap must stay within the static-surface budget"
    );
}

#[tokio::test]
async fn refactor_deferred_tools_list_uses_canonical_builder_preview_for_session() {
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
    // Phase-2: symbols/graph namespaces added for the search/graph verbs.
    assert!(
        body.contains("\"preferred_namespaces\":[\"reports\",\"symbols\",\"graph\",\"session\"]")
    );
    assert!(body.contains("\"tool_count\":"));
    assert!(body.contains("\"plan_safe_refactor\""));
    // Phase-2: request tracing rides the graph verb (mode=trace).
    assert!(body.contains("\"graph\""));
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
            "review".to_owned(),
            "graph".to_owned(),
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
    // 2026-07 tool-surface diet (stage 1): the raw call-graph primitive
    // get_callers left the reviewer-graph core surface and is now a builder
    // tool. This test exercises "load a namespace -> call a listed graph tool",
    // which only holds on a profile that still lists the raw primitives, so it
    // runs on the builder surface (refactor-full canonicalizes to builder).
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
