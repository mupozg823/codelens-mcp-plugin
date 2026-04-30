use super::*;

#[test]
fn schema_tools_return_structured_content_payload() {
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3101)),
            method: "tools/call".to_owned(),
            params: Some(
                json!({ "name": "get_symbols_overview", "arguments": { "path": "sample.py" } }),
            ),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert!(value["result"]["structuredContent"].is_object());
    assert!(value["result"]["structuredContent"]["symbols"].is_array());

    let text_payload = extract_tool_text(&response);
    let wrapped = parse_tool_payload(&text_payload);
    assert!(wrapped["data"]["symbols"].is_array());
}

#[test]
fn output_schema_workflow_tools_return_structured_content() {
    let project = project_root();
    fs::write(
        project.as_path().join("flow.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3102)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "analyze_change_request",
                "arguments": { "task": "improve alpha in flow.py" }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert!(value["result"]["structuredContent"].is_object());
    assert!(value["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(value["result"]["structuredContent"]["summary"].is_string());
    assert!(value["result"]["structuredContent"]["readiness"].is_object());
    assert!(value["result"]["structuredContent"]["verifier_checks"].is_array());

    let bootstrap_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31026)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "prepare_harness_session",
                "arguments": { "profile": "builder-minimal" }
            })),
        },
    )
    .unwrap();
    let bootstrap_value = serde_json::to_value(&bootstrap_response).unwrap();
    assert!(bootstrap_value["result"]["structuredContent"].is_object());
    assert_eq!(
        bootstrap_value["result"]["structuredContent"]["active_surface"],
        json!("builder-minimal")
    );
    assert!(bootstrap_value["result"]["structuredContent"]["health_summary"].is_object());
    assert!(bootstrap_value["result"]["structuredContent"]["capabilities"].is_object());
    assert!(
        bootstrap_value["result"]["structuredContent"]["capabilities"]["diagnostics_guidance"]
            .is_object()
    );
    assert!(
        bootstrap_value["result"]["structuredContent"]["visible_tools"]["tool_names"].is_array()
    );
    assert!(
        bootstrap_value["result"]["structuredContent"]["visible_tools"]["preferred_executors"]
            .is_object()
    );
    assert!(bootstrap_value["result"]["structuredContent"]["routing"].is_object());
    assert!(bootstrap_value["result"]["structuredContent"]["routing"]
        ["preferred_entrypoints_with_executors"]
        .is_array());
    assert!(bootstrap_value["result"]["structuredContent"]["routing"]
        ["recommended_entrypoint_preferred_executor"]
        .is_string());
    assert!(bootstrap_value["result"]["structuredContent"]["warnings"].is_array());
    let bootstrap_text = parse_tool_payload(&extract_tool_text(&bootstrap_response));
    assert_eq!(
        bootstrap_text["data"]["active_surface"],
        json!("builder-minimal")
    );
    assert!(bootstrap_text["data"]["capabilities"]["indexed_files"].is_u64());
    assert!(bootstrap_text["data"]["capabilities"]["stale_files"].is_u64());
    assert!(
        bootstrap_text["data"]["visible_tools"]["tool_names"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or_default()
            <= 3
    );
    assert!(bootstrap_text["data"]["routing"]["recommended_entrypoint"].is_string());
}

#[test]
fn workflow_alias_tools_return_structured_content_and_delegate() {
    let project = project_root();
    fs::write(
        project.as_path().join("workflow_alias.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31025)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "explore_codebase",
                "arguments": { "query": "alpha in workflow_alias.py", "max_tokens": 1200 }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        value["result"]["structuredContent"]["workflow"],
        json!("explore_codebase")
    );
    assert_eq!(
        value["result"]["structuredContent"]["delegated_tool"],
        json!("get_ranked_context")
    );
    assert!(
        value["result"]["structuredContent"]
            .get("deprecated")
            .is_none()
    );
    assert!(value["result"]["structuredContent"]["symbols"].is_array());
}

#[test]
fn verifier_tools_return_structured_content_payload() {
    let project = project_root();
    fs::write(
        project.as_path().join("verify.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let readiness_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31021)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "verify_change_readiness",
                "arguments": { "task": "update alpha in verify.py", "changed_files": ["verify.py"] }
            })),
        },
    )
    .unwrap();
    let readiness_value = serde_json::to_value(&readiness_response).unwrap();
    assert!(readiness_value["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(readiness_value["result"]["structuredContent"]["readiness"].is_object());
    assert!(readiness_value["result"]["structuredContent"]["verifier_checks"].is_array());
    let readiness_text = parse_tool_payload(&extract_tool_text(&readiness_response));
    assert!(readiness_text["data"]["analysis_id"].is_string());
    assert!(readiness_text["data"]["summary"].is_string());
    assert!(readiness_text["data"]["readiness"].is_object());
    assert!(
        readiness_text["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.contains("codelens://analysis/"))
            .unwrap_or(false)
    );
    assert!(
        readiness_text["data"]["section_handles"]
            .as_array()
            .map(|items| !items.is_empty() && items.len() <= 3)
            .unwrap_or(false)
    );
    assert!(readiness_text["suggested_next_calls"].is_array());
    assert!(
        readiness_text["suggested_next_calls"]
            .as_array()
            .map(|items| {
                items.iter().any(|entry| {
                    entry["tool"].as_str() == Some("get_analysis_section")
                        && entry["arguments"]["analysis_id"].is_string()
                })
            })
            .unwrap_or(false)
    );
    assert_eq!(readiness_text["routing_hint"], json!("async"));
    assert!(readiness_text["data"].get("verifier_checks").is_none());
    assert!(readiness_text["data"].get("blockers").is_none());
    assert!(readiness_text["data"].get("available_sections").is_none());

    let unresolved_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31022)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "unresolved_reference_check",
                "arguments": { "file_path": "verify.py", "symbol": "missing_symbol" }
            })),
        },
    )
    .unwrap();
    let unresolved_value = serde_json::to_value(&unresolved_response).unwrap();
    assert!(unresolved_value["result"]["structuredContent"]["blockers"].is_array());
    assert_eq!(
        unresolved_value["result"]["structuredContent"]["readiness"]["reference_safety"],
        json!("blocked")
    );
}

#[test]
fn oversized_schema_tool_truncates_structured_content_too() {
    let project = project_root();
    let source = (0..40)
        .map(|index| format!("def alpha_{index}():\n    return {index}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(project.as_path().join("oversized.py"), source).unwrap();
    let state = make_state(&project);
    state.set_token_budget(1);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3103)),
            method: "tools/call".to_owned(),
            params: Some(
                json!({ "name": "get_symbols_overview", "arguments": { "path": "oversized.py" } }),
            ),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    let text_payload = parse_tool_payload(&extract_tool_text(&response));
    assert_eq!(text_payload["truncated"], json!(true));
    assert_eq!(
        value["result"]["structuredContent"]["truncated"],
        json!(true)
    );
    assert!(
        text_payload["data"]["symbols"]
            .as_array()
            .map(|symbols| symbols.len())
            .unwrap_or_default()
            <= 3
    );
    assert!(
        value["result"]["structuredContent"]["symbols"]
            .as_array()
            .map(|symbols| symbols.len())
            .unwrap_or_default()
            <= 3
    );
}

#[test]
fn diagnose_issues_returns_structured_content() {
    // diagnose_issues with a path delegates to get_file_diagnostics, which
    // needs an LSP server.  We create a minimal python3-based mock named
    // `pyright-langserver` (the default binary for .py files) in a temp bin
    // directory and prepend it to PATH for the duration of the test.
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

    let bin_dir = std::env::temp_dir().join(format!(
        "codelens-test-bin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(windows)]
    let mock_script = bin_dir.join("pyright-langserver.py");
    #[cfg(not(windows))]
    let mock_script = bin_dir.join("pyright-langserver");
    fs::write(&mock_script, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    {
        let wrapper = bin_dir.join("pyright-langserver.cmd");
        fs::write(
            &wrapper,
            format!("@echo off\r\npython \"{}\" %*\r\n", mock_script.display()),
        )
        .unwrap();
    }

    let project = project_root();
    fs::write(
        project.as_path().join("diag_test.py"),
        "def hello():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _guard = super::PATH_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
    let original_path = std::env::var("PATH").unwrap_or_default();
    let patched_path = super::prepend_path(&bin_dir, &original_path);
    // SAFETY: protected by PATH_MUTEX; no other thread modifies PATH concurrently.
    unsafe {
        std::env::set_var("PATH", &patched_path);
    }

    let payload = call_tool(&state, "diagnose_issues", json!({"path": "diag_test.py"}));

    // SAFETY: restoring PATH; still under PATH_MUTEX.
    unsafe {
        std::env::set_var("PATH", original_path);
    }

    assert_eq!(
        payload["success"],
        json!(true),
        "expected success but got error: {}",
        payload["error"].as_str().unwrap_or("unknown error")
    );
    assert_eq!(payload["data"]["workflow"], json!("diagnose_issues"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("get_file_diagnostics")
    );
    assert!(
        payload["suggested_next_tools"]
            .as_array()
            .map(|items| items.iter().any(|value| value == "review_changes"))
            .unwrap_or(false)
    );
    assert!(
        payload["suggested_next_calls"]
            .as_array()
            .map(|items| {
                items.iter().any(|entry| {
                    entry["tool"] == json!("review_changes")
                        && entry["arguments"]["path"] == json!("diag_test.py")
                })
            })
            .unwrap_or(false)
    );
}

#[test]
fn cleanup_duplicate_logic_returns_structured_content() {
    // cleanup_duplicate_logic without the semantic feature delegates to
    // dead_code_report (no required args).
    let project = project_root();
    fs::write(
        project.as_path().join("dup_test.py"),
        "def foo():\n    return 1\ndef bar():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "cleanup_duplicate_logic", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["workflow"],
        json!("cleanup_duplicate_logic")
    );
}
