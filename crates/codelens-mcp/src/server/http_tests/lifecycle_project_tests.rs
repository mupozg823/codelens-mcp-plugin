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
        serde_json::json!("builder"),
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
        serde_json::json!("review"),
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
    assert!(list_a_body.contains("\"active_surface\":\"builder\""));

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
    assert!(list_b_body.contains("\"active_surface\":\"review\""));
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
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","detail":"full","preferred_entrypoints":["explore_codebase","plan_safe_refactor"]}}}}}}"#,
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
        serde_json::json!("builder")
    );
    assert_eq!(
        payload["data"]["active_surface"],
        serde_json::json!("builder")
    );
    assert_eq!(payload["data"]["token_budget"], serde_json::json!(6000));
    assert_eq!(
        payload["data"]["http_session"]["default_tools_list_contract_mode"],
        serde_json::json!("lean")
    );
    // The request pinned ["explore_codebase", "plan_safe_refactor"];
    // explore_codebase left the builder bootstrap slice in the Phase-2
    // verb consolidation (overview mode=explore), so the first VISIBLE
    // requested entrypoint is plan_safe_refactor.
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        serde_json::json!("plan_safe_refactor")
    );
    let tool_names = payload["data"]["visible_tools"]["default_listed_tool_names"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        tool_names
            .iter()
            .any(|value| value == "prepare_harness_session")
    );
}

#[tokio::test]
async fn prepare_harness_session_project_binding_suppresses_followup_hint() {
    let daemon_default = temp_project_dir("prepare-bind-default");
    let workspace = temp_project_dir("prepare-bind-workspace");
    std::fs::write(
        workspace.join("prepared_fixture.py"),
        "def prepared_marker_symbol():\n    return 1\n",
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
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","detail":"compact"}}}}}}"#,
                    workspace.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), StatusCode::OK);
    assert!(
        state.session_project_binding_explicit(&sid),
        "prepare_harness_session(project=...) must mark the HTTP session binding explicit"
    );

    let find = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"prepared_marker_symbol","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(find).await;
    assert!(
        body.contains("prepared_marker_symbol") && body.contains("prepared_fixture.py"),
        "prepared session must target the explicitly prepared workspace: {body}"
    );
    assert!(
        !body.contains("\"project_binding\""),
        "prepare_harness_session(project=...) must suppress the unbound-session hint on later calls: {body}"
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
        body.contains("implicit_session_project_binding")
            && body.contains("active_project_matches_session_project"),
        "hint must explain that this is an implicit binding, not necessarily a wrong active project: {body}"
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

/// #351: eviction + lenient resurrect must not drop an explicit header
/// binding. The same request that resurrects the session re-binds it, so
/// reads keep targeting the caller's workspace (not the daemon default)
/// and no `project_binding` nag appears.
#[tokio::test]
async fn resurrected_session_keeps_header_project_binding() {
    let daemon_default = temp_project_dir("resurrect-default");
    let workspace = temp_project_dir("resurrect-workspace");
    std::fs::write(
        workspace.join("resurrect_fixture.py"),
        "def resurrect_marker_symbol():\n    return 1\n",
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
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"resurrect-qa"}}}"#,
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

    // Simulate the 30-min idle sweep: the session vanishes from the store.
    state
        .session_store
        .as_ref()
        .expect("session store")
        .remove(&sid);

    // Same session id, same header — hosts attach the header to EVERY
    // request. The lenient gate resurrects, the seed re-binds.
    let find = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-project", workspace.to_str().unwrap())
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"resurrect_marker_symbol"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        find.headers()
            .get("x-codelens-session-resurrected")
            .and_then(|value| value.to_str().ok()),
        Some("1"),
        "the eviction simulation must actually exercise the resurrect path"
    );
    let body = body_string(find).await;
    assert!(
        body.contains("resurrect_marker_symbol") && body.contains("resurrect_fixture.py"),
        "resurrected session must still read the bound workspace: {body}"
    );
    assert!(
        !body.contains("\"project_binding\""),
        "header re-bind keeps the session explicit — no nag: {body}"
    );
}

/// #351: a session that never declared a workspace at initialize is
/// re-bound by the first request that carries the header — per-request
/// capture, not initialize-only.
#[tokio::test]
async fn header_rebinds_unbound_session_per_request() {
    let daemon_default = temp_project_dir("rebind-default");
    let workspace = temp_project_dir("rebind-workspace");
    std::fs::write(
        workspace.join("rebind_fixture.py"),
        "def rebind_marker_symbol():\n    return 1\n",
    )
    .unwrap();

    let project = ProjectRoot::new(daemon_default.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state.clone());

    // Initialize WITHOUT the header — unbound, seeded to the daemon default.
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"rebind-qa"}}}"#,
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

    let find = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-project", workspace.to_str().unwrap())
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"rebind_marker_symbol"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(find).await;
    assert!(
        body.contains("rebind_marker_symbol") && body.contains("rebind_fixture.py"),
        "header on a later request must bind the workspace: {body}"
    );
    assert!(
        !body.contains("\"project_binding\""),
        "per-request capture makes the binding explicit — no nag: {body}"
    );
}

fn tools_list_names(body: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("result").cloned())
        .and_then(|result| result.get("tools").cloned())
        .and_then(|tools| tools.as_array().cloned())
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    tool.get("name")
                        .and_then(|name| name.as_str())
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn init_session(app: &axum::Router) -> String {
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
    init.headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned()
}

async fn call_tool(app: &axum::Router, sid: &str, id: u64, name: &str, arguments: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{arguments}}}}}"#,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    body_string(response).await
}

async fn list_tools(app: &axum::Router, sid: &str, id: u64) -> Vec<String> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{}}}}"#,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    tools_list_names(&body_string(response).await)
}

/// #357 regression guard: two sessions bound to DIFFERENT projects issue
/// tool calls concurrently. Each must read its own project's index, and the
/// daemon-global override must stay untouched (the pre-#357 design switched
/// a global singleton under a daemon-wide mutex on every call, so parallel
/// sessions serialized and clobbered each other's runtime state).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sessions_on_different_projects_stay_isolated() {
    let project_a = temp_project_dir("concurrent-a");
    let project_b = temp_project_dir("concurrent-b");
    std::fs::write(
        project_a.join("alpha.py"),
        "def alpha_only():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        project_b.join("beta.py"),
        "def beta_only():\n    return 2\n",
    )
    .unwrap();

    let default_project = temp_project_dir("concurrent-default");
    let project = ProjectRoot::new(default_project.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state.clone());

    let sid_a = init_session(&app).await;
    let sid_b = init_session(&app).await;

    call_tool(
        &app,
        &sid_a,
        2,
        "activate_project",
        &format!(r#"{{"project":"{}"}}"#, project_a.display()),
    )
    .await;
    call_tool(
        &app,
        &sid_b,
        2,
        "activate_project",
        &format!(r#"{{"project":"{}"}}"#, project_b.display()),
    )
    .await;

    for round in 0..4u64 {
        let find_a = call_tool(
            &app,
            &sid_a,
            10 + round,
            "find_symbol",
            r#"{"name":"alpha_only","include_body":false,"max_matches":5}"#,
        );
        let find_b = call_tool(
            &app,
            &sid_b,
            10 + round,
            "find_symbol",
            r#"{"name":"beta_only","include_body":false,"max_matches":5}"#,
        );
        let (body_a, body_b) = tokio::join!(find_a, find_b);
        assert!(
            body_a.contains("alpha_only") && body_a.contains("alpha.py"),
            "session A must read project A (round {round}): {body_a}"
        );
        assert!(
            !body_a.contains("beta.py"),
            "session A must not leak project B files (round {round}): {body_a}"
        );
        assert!(
            body_b.contains("beta_only") && body_b.contains("beta.py"),
            "session B must read project B (round {round}): {body_b}"
        );
        assert!(
            !body_b.contains("alpha.py"),
            "session B must not leak project A files (round {round}): {body_b}"
        );
    }

    // Session-scoped activation must never mutate the daemon-global override.
    assert!(
        !state.has_explicit_active_project(),
        "session-bound calls must leave the daemon default project untouched"
    );
}

/// #357 regression guard: the compact bootstrap tools/list must expand to
/// the full surface after `prepare_harness_session` — standard MCP clients
/// never send the `full`/`namespace` expansion params, so without this flip
/// symbol tools (find_referencing_symbols, get_symbols_overview) stayed
/// permanently invisible.
#[tokio::test]
async fn prepare_harness_session_expands_tools_list_surface() {
    let project = temp_project_dir("expand-surface");
    std::fs::write(project.join("main.py"), "def entry():\n    return 1\n").unwrap();

    let state = test_state();
    let app = build_router(state.clone());
    let sid = init_session(&app).await;

    let bootstrap_names = list_tools(&app, &sid, 2).await;
    assert!(
        !bootstrap_names.is_empty(),
        "bootstrap listing must not be empty"
    );
    assert!(
        !bootstrap_names
            .iter()
            .any(|name| name == "find_referencing_symbols"),
        "bootstrap listing is expected to be collapsed: {bootstrap_names:?}"
    );

    let prepare_body = call_tool(
        &app,
        &sid,
        3,
        "prepare_harness_session",
        &format!(
            r#"{{"project":"{}","profile":"reviewer-graph"}}"#,
            project.display()
        ),
    )
    .await;
    assert!(
        prepare_body.contains("\"result\""),
        "prepare_harness_session must succeed: {prepare_body}"
    );

    let expanded_names = list_tools(&app, &sid, 4).await;
    assert!(
        expanded_names
            .iter()
            .any(|name| name == "find_referencing_symbols"),
        "post-bootstrap listing must expose symbol tools: {expanded_names:?}"
    );
    assert!(
        expanded_names
            .iter()
            .any(|name| name == "get_symbols_overview"),
        "post-bootstrap listing must expose overview tooling: {expanded_names:?}"
    );
    assert!(
        expanded_names.len() > bootstrap_names.len(),
        "surface must expand past the bootstrap subset ({} -> {})",
        bootstrap_names.len(),
        expanded_names.len()
    );
}
