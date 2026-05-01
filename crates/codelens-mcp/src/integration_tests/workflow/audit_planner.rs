use super::*;

#[test]
fn audit_planner_session_is_not_applicable_for_non_planner_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "builder-minimal"}),
        "builder-session",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "builder-session"}),
    );
    assert_eq!(audit["data"]["status"], json!("not_applicable"));
}

#[cfg(feature = "http")]
#[cfg(feature = "http")]
#[test]
fn audit_planner_session_passes_for_happy_path_reviewer() {
    let project = project_root();
    fs::write(project.as_path().join("planner_pass.py"), "print('ok')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_pass.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_pass.py"], "task": "review planner path"}),
        &session_id,
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("pass"));
}

#[test]
fn audit_planner_session_warns_when_bootstrap_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("planner_warn.py"), "print('ok')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_warn.py"}),
        "planner-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_warn.py"], "task": "review planner path"}),
        "planner-warn",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-warn"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("bootstrap_order")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_warns_when_change_evidence_is_missing() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-change-evidence",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "planner-change-evidence",
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "review change evidence"}),
        "planner-change-evidence",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-change-evidence"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("change_evidence")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_warns_when_workflow_first_guidance_is_missed() {
    let project = project_root();
    fs::write(project.as_path().join("planner_chain.py"), "print('ok')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "planner-readonly", "detail": "compact"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_chain.py"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "find_symbol",
        json!({"name": "missing_symbol", "include_body": true}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "planner_chain.py"}),
        "planner-chain",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-chain"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("workflow_first")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_fails_on_mutation_attempt() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-fail",
    );
    let payload = call_tool_with_session(
        &state,
        "create_text_file",
        json!({"relative_path": "planner_fail.py", "content": "print('fail')\n"}),
        "planner-fail",
    );
    assert_eq!(payload["success"], json!(false));

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-fail"}),
    );
    assert_eq!(audit["data"]["status"], json!("fail"));
}

#[cfg(feature = "http")]
#[cfg(feature = "http")]
#[test]
fn audit_planner_session_isolated_by_session_id() {
    let project = project_root();
    fs::write(project.as_path().join("planner_a.py"), "print('a')\n").unwrap();
    fs::write(project.as_path().join("planner_b.py"), "print('b')\n").unwrap();
    let state = make_http_state(&project);
    let session_a = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );
    let session_b = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_a.py"], "task": "review a"}),
        &session_a,
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &session_b,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_b.py"}),
        &session_b,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_b.py"], "task": "review b"}),
        &session_b,
    );

    let metrics_a = call_tool(
        &state,
        "get_tool_metrics",
        json!({"session_id": session_a.as_str()}),
    );
    assert_eq!(metrics_a["data"]["session_id"], json!(session_a.as_str()));
    let audit_a = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": session_a.as_str()}),
    );
    assert_eq!(audit_a["data"]["status"], json!("warn"));
    assert!(
        audit_a["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("bootstrap_order")))
            .unwrap_or(false)
    );
}

#[test]
fn export_session_markdown_appends_planner_audit_summary() {
    let project = project_root();
    fs::write(project.as_path().join("planner_md.py"), "print('md')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_md.py"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_md.py"], "task": "review markdown"}),
        "planner-md",
    );

    let markdown = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "planner-md", "name": "planner-md"}),
    );
    let body = markdown["data"]["markdown"].as_str().unwrap_or("");
    assert!(body.contains("## Planner Audit"));
}
