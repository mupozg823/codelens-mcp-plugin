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
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"insert_after_symbol","arguments":{"relative_path":"audit_http.py","symbol_name":"old","content":"\ndef new():\n    return 2\n"}}}"#,
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
    // #347: mutations require an explicit project binding — bind at initialize.
    let project_path = state.project().as_path().to_string_lossy().into_owned();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .header("x-codelens-project", project_path.as_str())
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

    // Seed a symbol target so the symbolic edit core has something to mutate.
    std::fs::write(
        state.project().as_path().join("audit_http.py"),
        "def old():\n    return 1\n",
    )
    .unwrap();

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
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"insert_after_symbol","arguments":{"relative_path":"audit_http.py","symbol_name":"old","content":"\ndef new():\n    return 2\n"}}}"#,
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
    // Phase 2 close part 4: jsonl intent log retired — the same
    // metadata now lives in the audit_sink session_metadata column.
    let sink = state.audit_sink().expect("audit_sink available");
    let rows = sink.query(None, None, 100).expect("query");
    let row = rows
        .iter()
        .find(|r| r.tool == "insert_after_symbol")
        .expect("insert_after_symbol row in audit_sink");
    let metadata = row
        .session_metadata
        .as_ref()
        .expect("session_metadata captured");
    assert_eq!(metadata["trusted_client"], serde_json::json!(true));
    assert_eq!(
        metadata["requested_profile"],
        serde_json::json!("refactor-full")
    );
    assert_eq!(metadata["client_name"], serde_json::json!("HarnessQA"));
}

// ── #347 hard gate: unbound shared-daemon sessions cannot mutate ─────

async fn init_session(app: &axum::Router, project_header: Option<&str>) -> String {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json");
    if let Some(project) = project_header {
        builder = builder.header("x-codelens-project", project);
    }
    let init = app
        .clone()
        .oneshot(
            builder
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"binding-gate-qa"}}}"#,
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

async fn call_write_memory(app: &axum::Router, sid: &str, memory_name: &str) -> String {
    // 2026-07 tool-surface diet, step 2: write_memory left every default preset
    // surface (Balanced included) and is now callable only on the Full surface.
    // Switch this session to Full first so the project-binding gate under test
    // (#347) — not the surface gate — is what governs the mutation. set_preset
    // is not a content mutation, so it is never blocked on an unbound session.
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"set_preset","arguments":{"preset":"full"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"write_memory","arguments":{{"memory_name":"{memory_name}","content":"gate probe"}}}}}}"#,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_string(resp).await
}

#[tokio::test]
async fn unbound_session_mutation_blocked_with_recovery_hint() {
    // The gate reads CODELENS_ALLOW_UNBOUND_MUTATION per call — hold the
    // env lock so the escape-hatch test cannot interleave.
    let _env_guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = test_state();
    let app = build_router(state.clone());

    let sid = init_session(&app, None).await;
    let body = call_write_memory(&app, &sid, "unbound_gate_probe").await;
    let payload = first_tool_payload(&body);
    assert_eq!(payload["success"], json!(false), "{body}");
    let error = payload["error"].as_str().unwrap_or_default();
    assert!(error.contains("project_binding_required"), "{body}");
    assert_eq!(payload["recovery_hint"]["kind"], json!("fallback_tool"));
    assert_eq!(
        payload["recovery_hint"]["tool"],
        json!("prepare_harness_session")
    );
    // Pre-execution block: the memory file must never be created.
    assert!(
        !state
            .project()
            .as_path()
            .join(".codelens/memories/unbound_gate_probe.md")
            .exists(),
        "blocked mutation must not touch the filesystem"
    );
}

#[tokio::test]
async fn bound_session_mutation_passes() {
    let state = test_state();
    let project_path = state.project().as_path().to_string_lossy().into_owned();
    let app = build_router(state.clone());

    let sid = init_session(&app, Some(project_path.as_str())).await;
    let body = call_write_memory(&app, &sid, "bound_gate_probe").await;
    let payload = first_tool_payload(&body);
    assert_eq!(payload["success"], json!(true), "{body}");
    assert!(!body.contains("project_binding_required"), "{body}");
}

#[tokio::test]
async fn allow_unbound_mutation_env_restores_advisory_behavior() {
    let _env_guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::var("CODELENS_ALLOW_UNBOUND_MUTATION").ok();
    // SAFETY: env mutation serialized under TEST_ENV_LOCK.
    unsafe {
        std::env::set_var("CODELENS_ALLOW_UNBOUND_MUTATION", "1");
    }

    let state = test_state();
    let app = build_router(state.clone());
    let sid = init_session(&app, None).await;
    let body = call_write_memory(&app, &sid, "override_gate_probe").await;

    // SAFETY: env mutation serialized under TEST_ENV_LOCK.
    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_ALLOW_UNBOUND_MUTATION", value),
            None => std::env::remove_var("CODELENS_ALLOW_UNBOUND_MUTATION"),
        }
    }

    let payload = first_tool_payload(&body);
    assert_eq!(payload["success"], json!(true), "{body}");
    // Advisory behavior restored: the mutation succeeds but the unbound
    // `project_binding` hint still nags on the success payload.
    assert_eq!(
        payload["data"]["project_binding"]["reason"],
        json!("implicit_session_project_binding"),
        "{body}"
    );
}
