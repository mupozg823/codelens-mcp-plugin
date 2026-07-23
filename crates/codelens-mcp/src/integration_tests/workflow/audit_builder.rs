use super::*;

// 2026-07 tool-surface diet: audit_builder_session left the reviewer-graph
// core surface, and set_profile sets the active surface, so a reviewer-graph
// session can no longer call it. planner-readonly is a non-builder surface that
// retains audit_builder_session, so it exercises the "not applicable for a
// non-builder session" path with the same derived status. The builder-surface
// case uses cleanup_duplicate_logic (now builder-only). See
// docs/operations/tool-surface-diet-2026-07.md.
#[test]
fn audit_builder_session_is_not_applicable_for_non_builder_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
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
fn audit_builder_session_applies_to_builder_surface_tool() {
    let project = project_root();
    fs::write(
        project.as_path().join("builder_preferred.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "builder-minimal"}),
        "builder-surface-tool",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "compact"}),
        "builder-surface-tool",
    );
    let _ = call_tool_with_session(
        &state,
        "cleanup_duplicate_logic",
        json!({"scope": "builder_preferred.py", "max_results": 1}),
        "builder-surface-tool",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "builder-surface-tool"}),
    );
    assert_ne!(audit["data"]["status"], json!("not_applicable"));
    assert_eq!(
        audit["data"]["checks"][0]["evidence"]["has_builder_lane_tool"],
        json!(true)
    );
}

#[test]
fn audit_builder_session_applies_to_build_phase_tool_without_bootstrap() {
    let project = project_root();
    fs::write(
        project.as_path().join("build_phase_only.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "cleanup_duplicate_logic",
        json!({"scope": "build_phase_only.py", "max_results": 1}),
        "build-phase-only",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "build-phase-only"}),
    );
    assert_ne!(audit["data"]["status"], json!("not_applicable"));
    assert_eq!(
        audit["data"]["checks"][0]["evidence"]["has_builder_lane_tool"],
        json!(true)
    );
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
        "replace_symbol_body",
        json!({
            "relative_path": "bootstrap_warn.py",
            "symbol_name": "old",
            "new_body": "    return 2"
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

    // Mutation gate test: use builder-minimal profile + a symbolic edit
    // tool to test that stale preflight blocks mutations.
    let _ = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));

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
        "replace_symbol_body",
        json!({
            "relative_path": "audit_fail.py",
            "symbol_name": "old",
            "new_body": "    return 2"
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
    fs::write(
        project.as_path().join("http_builder.py"),
        "def old():\n    return 1\n",
    )
    .unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "compact"}),
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
        "replace_symbol_body",
        json!({
            "relative_path": "http_builder.py",
            "symbol_name": "old",
            "new_body": "    return 2"
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

// 2026-07 tool-surface diet step 2 moved the agent-coordination tools
// (register_agent_work / claim_files / release_files) off every default preset
// surface because multi-agent coordination is a host-harness-owned capability,
// not a CodeLens duplicate (docs/operations/tool-surface-diet-2026-07.md
// "결정 확정"). A profile-bound HTTP builder therefore cannot emit coordination
// evidence, so its absence must NOT downgrade the builder audit. This test is
// the reflection of that prior decision, not a reduction of the audit contract:
// the mutation-preflight and diagnostics axes stay strict (this session runs
// verifier + pre/post diagnostics), and only the coordination axis is reported
// host-owned. It replaces the former
// `audit_builder_session_warns_when_http_coordination_is_missing`, which encoded
// the now-obsolete "missing coordination => warn" contract.
#[cfg(feature = "http")]
#[test]
fn audit_builder_session_treats_missing_http_coordination_as_host_owned() {
    let project = project_root();
    fs::write(
        project.as_path().join("http_host_owned.py"),
        "def old():\n    return 1\n",
    )
    .unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "http_host_owned.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "http_host_owned.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "update http host-owned file", "changed_files": ["http_host_owned.py"]}),
        &session_id,
    );
    // Intentionally NO register_agent_work / claim_files / release_files:
    // coordination is host-owned, so its absence must not degrade the audit.
    let payload = call_tool_with_session(
        &state,
        "replace_symbol_body",
        json!({
            "relative_path": "http_host_owned.py",
            "symbol_name": "old",
            "new_body": "    return 2"
        }),
        &session_id,
    );
    assert_eq!(payload["success"], json!(true));
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "http_host_owned.py"}),
        &session_id,
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": session_id}),
    );

    // Missing coordination evidence does not degrade the audit.
    assert_eq!(audit["data"]["status"], json!("pass"));

    // The coordination axis is reported host-owned (not_applicable), never WARN.
    let checks = audit["data"]["checks"]
        .as_array()
        .expect("audit checks array");
    for code in ["coordination_registration", "coordination_claim"] {
        let check = checks
            .iter()
            .find(|check| check["code"] == json!(code))
            .unwrap_or_else(|| panic!("missing {code} check"));
        assert_eq!(
            check["status"],
            json!("not_applicable"),
            "{code} must not downgrade when coordination is host-owned"
        );
        assert_eq!(
            check["evidence"]["coordination"],
            json!("host-owned"),
            "{code} must be marked host-owned"
        );
    }

    // No coordination code leaks into findings.
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings.iter().all(|finding| {
                finding["code"] != json!("coordination_registration")
                    && finding["code"] != json!("coordination_claim")
                    && finding["code"] != json!("coordination_release")
            }))
            .unwrap_or(true),
        "coordination must not appear in findings once it is host-owned"
    );
}
