use super::*;

#[test]
fn audit_builder_session_is_not_applicable_for_non_builder_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "reviewer-session",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "reviewer-session"}),
    );
    assert_eq!(audit["data"]["status"], json!("not_applicable"));
}

#[test]
fn audit_builder_session_warns_when_bootstrap_is_missing() {
    let project = project_root();
    fs::write(
        project.as_path().join("bootstrap_warn.py"),
        "print('old')\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "builder-minimal"}),
        "builder-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "bootstrap_warn.py"}),
        "builder-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update bootstrap warn file",
            "changed_files": ["bootstrap_warn.py"]
        }),
        "builder-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "replace",
        json!({
            "relative_path": "bootstrap_warn.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "builder-warn",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "builder-warn"}),
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
fn audit_builder_session_fails_when_gate_failure_was_recorded() {
    let project = project_root();
    fs::write(project.as_path().join("audit_fail.py"), "print('old')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "builder-fail",
    );
    let preflight = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update audit fail file",
            "changed_files": ["audit_fail.py"]
        }),
        "builder-fail",
    );
    assert_eq!(preflight["success"], json!(true));
    state.set_recent_preflight_timestamp_for_test("builder-fail", 0);

    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "audit_fail.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "builder-fail",
    );
    assert_eq!(payload["success"], json!(false));

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "builder-fail"}),
    );
    assert_eq!(audit["data"]["status"], json!("fail"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("mutation_gate")))
            .unwrap_or(false)
    );
}

#[cfg(feature = "http")]
#[test]
fn audit_builder_session_passes_for_happy_path_http_builder() {
    let project = project_root();
    fs::write(project.as_path().join("http_builder.py"), "print('old')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "refactor-full", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "http_builder.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "http_builder.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "update http builder file", "changed_files": ["http_builder.py"]}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "builder-http",
            "branch": "audit/http-pass",
            "worktree": project.as_path().to_string_lossy().to_string(),
            "intent": "happy path builder audit"
        }),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "claim_files",
        json!({"paths": ["http_builder.py"], "reason": "happy path builder audit"}),
        &session_id,
    );
    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "http_builder.py",
            "old_text": "old",
            "new_text": "new"
        }),
        &session_id,
    );
    assert_eq!(payload["success"], json!(true));
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "http_builder.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "release_files",
        json!({"paths": ["http_builder.py"]}),
        &session_id,
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("pass"));
}

#[cfg(feature = "http")]
#[cfg(feature = "http")]
#[test]
fn audit_builder_session_warns_when_http_coordination_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("http_warn.py"), "print('old')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "refactor-full", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "http_warn.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "update http warn file", "changed_files": ["http_warn.py"]}),
        &session_id,
    );
    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "http_warn.py",
            "old_text": "old",
            "new_text": "new"
        }),
        &session_id,
    );
    assert_eq!(payload["success"], json!(true));

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| {
                findings
                    .iter()
                    .any(|finding| finding["code"] == json!("coordination_registration"))
                    && findings
                        .iter()
                        .any(|finding| finding["code"] == json!("coordination_claim"))
            })
            .unwrap_or(false)
    );
}
