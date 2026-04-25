use super::*;

#[tokio::test]
async fn deferred_resources_read_tracks_loaded_namespaces_for_session() {
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

    let summary = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"codelens://tools/list"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(summary.status(), StatusCode::OK);
    let summary_body = body_string(summary).await;
    let summary_text = first_resource_text(&summary_body);
    assert!(summary_text.contains("\"loaded_namespaces\": []"));
    assert!(summary_text.contains("\"loaded_tiers\": []"));
    assert!(!summary_text.contains("\"filesystem\":"));
    assert!(!summary_text.contains("\"find_symbol\""));

    let expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    // Use lsp namespace — get_file_diagnostics is in reviewer-graph but lsp is not preferred
                    r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"codelens://tools/list","namespace":"lsp"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(expand.status(), StatusCode::OK);
    let expand_body = body_string(expand).await;
    let expand_text = first_resource_text(&expand_body);
    assert!(expand_text.contains("\"selected_namespace\": \"lsp\""));
    assert!(expand_text.contains("\"get_file_diagnostics\""));

    let tier_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"codelens://tools/list","tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(tier_expand.status(), StatusCode::OK);
    let tier_expand_body = body_string(tier_expand).await;
    let tier_expand_text = first_resource_text(&tier_expand_body);
    assert!(tier_expand_text.contains("\"selected_tier\": \"primitive\""));
    assert!(tier_expand_text.contains("\"find_symbol\""));

    let summary_after = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"codelens://tools/list"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(summary_after.status(), StatusCode::OK);
    let summary_after_body = body_string(summary_after).await;
    let summary_after_text = first_resource_text(&summary_after_body);
    assert!(summary_after_text.contains("\"loaded_namespaces\": ["));
    assert!(summary_after_text.contains("\"lsp\""));
    assert!(summary_after_text.contains("\"loaded_tiers\": ["));
    assert!(summary_after_text.contains("\"primitive\""));
    assert!(summary_after_text.contains("\"effective_namespaces\": ["));
    assert!(summary_after_text.contains("\"effective_tiers\": ["));
    assert!(summary_after_text.contains("\"get_file_diagnostics\""));
    assert!(summary_after_text.contains("\"find_symbol\""));

    let session_resource = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"codelens://session/http"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(session_resource.status(), StatusCode::OK);
    let session_body = body_string(session_resource).await;
    let session_text = first_resource_text(&session_body);
    assert!(session_text.contains("\"loaded_namespaces\": ["));
    assert!(session_text.contains("\"lsp\""));
    assert!(session_text.contains("\"loaded_tiers\": ["));
    assert!(session_text.contains("\"primitive\""));
    assert!(session_text.contains("\"full_tool_exposure\": false"));
    assert!(session_text.contains("\"preferred_tiers\": ["));
    assert!(session_text.contains("\"workflow\""));
    assert!(session_text.contains("\"client_profile\": \"generic\""));
    assert!(session_text.contains("\"default_tools_list_contract_mode\": \"full\""));
    assert!(session_text.contains("\"semantic_search_status\":"));
    assert!(session_text.contains("\"supported_files\":"));
    assert!(session_text.contains("\"stale_files\":"));
    assert!(session_text.contains("\"daemon_binary_drift\":"));
    assert!(session_text.contains("\"health_summary\":"));
    assert!(session_text.contains("\"deferred_tier_gate\": true"));
    assert!(session_text.contains("\"requires_tier_listing_before_tool_call\": true"));
}

#[tokio::test]
async fn deferred_namespace_load_expands_default_surface_and_allows_calls() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
        "    elif m == 'textDocument/diagnostic':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let unique_name = format!("deferred-open-{}.py", std::process::id());
    let file_path = state.project().as_path().join(&unique_name);
    let mock_path = state.project().as_path().join("mock_lsp.py");
    std::fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

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

    // Load tier "primitive" first — then namespace "lsp" becomes visible
    let tier_expand = app
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

    assert_eq!(tier_expand.status(), StatusCode::OK);
    let tier_body = body_string(tier_expand).await;
    assert!(tier_body.contains("\"selected_tier\":\"primitive\""));
    assert!(tier_body.contains("\"find_symbol\""));

    // Now load namespace "lsp" — get_file_diagnostics should appear
    let ns_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"namespace":"lsp"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(ns_expand.status(), StatusCode::OK);
    let ns_body = body_string(ns_expand).await;
    assert!(ns_body.contains("\"selected_namespace\":\"lsp\""));
    assert!(ns_body.contains("\"get_file_diagnostics\""));

    // Verify expanded namespace allows the tool call using a deterministic mock LSP.
    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}","command":"python3","args":["{}"]}}}}}}"#,
                    file_path.display(),
                    mock_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_body = body_string(allowed).await;
    assert!(
        allowed_body.contains("\\\"success\\\": true")
            || allowed_body.contains("\\\"success\\\":true"),
        "deferred_namespace body: {allowed_body}"
    );
}

#[tokio::test]
async fn deferred_tier_load_expands_default_surface_and_allows_calls() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let file_path = state.project().as_path().join("deferred-tier.py");
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

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

    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"find_symbol","arguments":{{"name":"beta","file_path":"{}","include_body":false}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(blocked.status(), StatusCode::OK);
    let blocked_body = body_string(blocked).await;
    assert!(blocked_body.contains("hidden by deferred loading in tier `primitive`"));

    let expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(expand.status(), StatusCode::OK);
    let expand_body = body_string(expand).await;
    assert!(expand_body.contains("\"selected_tier\":\"primitive\""));
    assert!(expand_body.contains("\"find_symbol\""));

    let default_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(default_list.status(), StatusCode::OK);
    let default_body = body_string(default_list).await;
    assert!(default_body.contains("\"loaded_tiers\":[\"primitive\"]"));
    assert!(default_body.contains("\"find_symbol\""));

    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"find_symbol","arguments":{{"name":"beta","file_path":"{}","include_body":false}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_body = body_string(allowed).await;
    assert!(
        allowed_body.contains("\\\"success\\\": true")
            || allowed_body.contains("\\\"success\\\":true")
    );
}
