use super::*;

#[tokio::test]
async fn session_binding_rebinds_project_per_request() {
    let project_a = temp_project_dir("project-a");
    let project_b = temp_project_dir("project-b");
    std::fs::write(
        project_a.join("first.py"),
        "def first_only():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        project_b.join("second.py"),
        "def second_only():\n    return 2\n",
    )
    .unwrap();

    let project = ProjectRoot::new(project_a.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state.clone());

    let init_a = app
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
    let sid_a = init_a
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let init_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_b = init_b
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let activate_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"activate_project","arguments":{{"project":"{}"}}}}}}"#,
                    project_b.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activate_b.status(), StatusCode::OK);

    let find_second = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"second_only","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let second_body = body_string(find_second).await;
    assert!(second_body.contains("second_only"));
    assert!(second_body.contains("second.py"));

    let find_first = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"first_only","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let first_body = body_string(find_first).await;
    assert!(first_body.contains("first_only"));
    assert!(first_body.contains("first.py"));
}

#[tokio::test]
async fn session_bound_missing_project_fails_closed() {
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

    let missing = temp_project_dir("missing").join("gone");
    state
        .session_store
        .as_ref()
        .unwrap()
        .get(&sid)
        .unwrap()
        .set_project_path(missing.to_string_lossy().to_string());

    let find = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"hello","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_string(find).await;
    assert!(body.contains("automatic rebind failed"));
}

#[tokio::test]
async fn session_profiles_are_isolated_across_tools_list() {
    let state = test_state();
    let app = build_router(state.clone());

    let init_a = app
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
    let sid_a = init_a
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let init_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_b = init_b
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let set_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"builder-minimal"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_a.status(), StatusCode::OK);
    let set_a_body = body_string(set_a).await;
    let set_a_payload = first_tool_payload(&set_a_body);
    assert_eq!(
        set_a_payload["success"],
        serde_json::json!(true),
        "set_profile(session A) failed: {set_a_body}"
    );
    assert_eq!(
        set_a_payload["data"]["current_profile"],
        serde_json::json!("builder-minimal"),
        "unexpected set_profile(session A) payload: {set_a_body}"
    );

    let set_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_b.status(), StatusCode::OK);
    let set_b_body = body_string(set_b).await;
    let set_b_payload = first_tool_payload(&set_b_body);
    assert_eq!(
        set_b_payload["success"],
        serde_json::json!(true),
        "set_profile(session B) failed: {set_b_body}"
    );
    assert_eq!(
        set_b_payload["data"]["current_profile"],
        serde_json::json!("reviewer-graph"),
        "unexpected set_profile(session B) payload: {set_b_body}"
    );

    let list_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let list_a_body = body_string(list_a).await;
    assert!(list_a_body.contains("\"active_surface\":\"builder-minimal\""));

    let list_b = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let list_b_body = body_string(list_b).await;
    assert!(list_b_body.contains("\"active_surface\":\"reviewer-graph\""));
}

#[tokio::test]
async fn codex_session_prepare_harness_session_bootstraps_without_tools_list() {
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

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","preferred_entrypoints":["explore_codebase","plan_safe_refactor"]}}}}}}"#,
                    state.project().as_path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(bootstrap).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["project"]["auto_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(
        payload["data"]["active_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(payload["data"]["token_budget"], serde_json::json!(6000));
    assert_eq!(
        payload["data"]["http_session"]["default_tools_list_contract_mode"],
        serde_json::json!("lean")
    );
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        serde_json::json!("explore_codebase")
    );
    let tool_names = payload["data"]["visible_tools"]["tool_names"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        tool_names
            .iter()
            .any(|value| value == "prepare_harness_session")
    );
}

// ── Session project binding at initialize (#347: shared-daemon default
//    project trap) ──────────────────────────────────────────────────────

/// The `x-codelens-project` header binds the session to the caller's
/// workspace at initialize — no activate_project round trip — and the
/// first tool call auto-switches + auto-indexes that project.
#[tokio::test]
async fn initialize_header_binds_session_project_and_indexes() {
    let daemon_default = temp_project_dir("bind-default");
    let workspace = temp_project_dir("bind-workspace");
    std::fs::write(
        workspace.join("bound_fixture.py"),
        "def bound_marker_symbol():\n    return 1\n",
    )
    .unwrap();

    let project = ProjectRoot::new(daemon_default.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-project", workspace.to_str().unwrap())
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"binding-qa"}}}"#,
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

    // No activate_project call — the binding must come from the header.
    let find = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"bound_marker_symbol","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(find).await;
    assert!(
        body.contains("bound_marker_symbol") && body.contains("bound_fixture.py"),
        "header-bound workspace must be auto-switched and auto-indexed: {body}"
    );
}

/// `params.project` in the initialize request works the same for
/// programmatic clients that cannot set transport headers.
#[tokio::test]
async fn initialize_params_project_binds_session() {
    let daemon_default = temp_project_dir("bind-params-default");
    let workspace = temp_project_dir("bind-params-workspace");
    std::fs::write(
        workspace.join("params_fixture.py"),
        "def params_marker_symbol():\n    return 1\n",
    )
    .unwrap();

    let project = ProjectRoot::new(daemon_default.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"binding-qa"}},"project":"{}"}}}}"#,
                    workspace.display()
                )))
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

    let find = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"params_marker_symbol","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(find).await;
    assert!(
        body.contains("params_marker_symbol") && body.contains("params_fixture.py"),
        "params-bound workspace must be auto-switched and auto-indexed: {body}"
    );
}

/// Sessions that never declared a workspace operate on the daemon's
/// default project — the response must say so loudly so an agent can
/// self-correct instead of silently reading the wrong repo.
#[tokio::test]
async fn unbound_session_responses_carry_project_binding_hint() {
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
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"binding-qa"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let init_body = body_string(init).await;
    // Serena-style onboarding directive must be in the initialize
    // instructions so every host sees it regardless of repo-local docs.
    assert!(
        init_body.contains("x-codelens-project"),
        "initialize instructions must teach the project binding path: {init_body}"
    );

    let init2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"binding-qa"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init2
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_memories","arguments":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(list).await;
    assert!(
        body.contains("project_binding"),
        "unbound session must carry a project_binding hint: {body}"
    );
    assert!(
        body.contains("prepare_harness_session"),
        "hint must name the remediation tool: {body}"
    );
}

/// A header-bound session must NOT carry the hint — binding is explicit.
#[tokio::test]
async fn bound_session_responses_omit_project_binding_hint() {
    let workspace = temp_project_dir("bind-quiet-workspace");
    let state = test_state();
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-project", workspace.to_str().unwrap())
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"binding-qa"}}}"#,
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

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_memories","arguments":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(list).await;
    assert!(
        !body.contains("\"project_binding\""),
        "explicitly bound session must not nag: {body}"
    );
}
