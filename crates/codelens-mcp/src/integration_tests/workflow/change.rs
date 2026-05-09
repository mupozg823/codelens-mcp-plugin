use super::*;

#[test]
fn analyze_change_request_returns_handle_and_section() {
    let project = project_root();
    fs::write(
        project.as_path().join("workflow.py"),
        "def search_users(query):\n    return []\n\ndef delete_user(uid):\n    return uid\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update search users flow"}),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id");
    assert!(analysis_id.starts_with("analysis-"));
    assert!(matches!(
        payload["data"]["risk_level"].as_str(),
        Some("low" | "medium" | "high")
    ));
    assert!(payload["data"]["quality_focus"].is_array());
    assert!(payload["data"]["recommended_checks"].is_array());
    assert!(payload["data"]["performance_watchpoints"].is_array());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["blocker_count"].is_number());
    assert!(payload["data"]["readiness"]["diagnostics_ready"].is_string());
    assert!(payload["data"]["readiness"]["reference_safety"].is_string());
    assert!(payload["data"]["readiness"]["test_readiness"].is_string());
    assert!(payload["data"]["readiness"]["mutation_ready"].is_string());
    assert!(payload["data"]["verifier_checks"].is_array());

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );
    assert_eq!(section["success"], json!(true));
    assert_eq!(section["data"]["analysis_id"], json!(analysis_id));
    assert!(
        state
            .analysis_dir()
            .join(analysis_id)
            .join("ranked_files.json")
            .exists()
    );
}

#[test]
fn orchestrate_change_dry_run_returns_run_contract_and_evidence() {
    let project = project_root();
    let original = "def checkout(cart):\n    return cart\n";
    fs::write(project.as_path().join("orchestrated_checkout.py"), original).unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "orchestrate_change",
        json!({
            "task": "update checkout orchestration flow",
            "mode": "planner_builder",
            "target_paths": ["orchestrated_checkout.py"],
            "acceptance": ["preflight evidence exists", "no files are mutated"]
        }),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id");
    let sections = payload["data"]["available_sections"]
        .as_array()
        .expect("available_sections");
    for expected in [
        "orchestration_run",
        "plan",
        "preflight",
        "evidence_handles",
        "audit_events",
        "role_bindings",
        "approval_policy",
        "data_ownership",
        "dispatch_plan",
    ] {
        assert!(
            sections.iter().any(|section| section == expected),
            "{expected}"
        );
    }

    let run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    assert_eq!(run["success"], json!(true));
    assert_eq!(run["data"]["content"]["dry_run"], json!(true));
    assert_eq!(run["data"]["content"]["execution_performed"], json!(false));
    assert_eq!(run["data"]["content"]["state"], json!("approval_required"));
    assert_eq!(run["data"]["content"]["mode"], json!("planner_builder"));
    assert_eq!(
        run["data"]["content"]["target_paths"],
        json!(["orchestrated_checkout.py"])
    );

    let audit = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "audit_events"}),
    );
    assert_eq!(audit["success"], json!(true));
    let events = audit["data"]["content"]["events"]
        .as_array()
        .expect("audit events");
    assert!(events.iter().any(|event| event["event"] == "run_created"));
    assert!(
        events
            .iter()
            .any(|event| event["event"] == "approval_requested")
    );
    assert!(events.iter().all(|event| event["audit_required"] == true));
    assert_eq!(
        fs::read_to_string(project.as_path().join("orchestrated_checkout.py")).unwrap(),
        original
    );
}

#[test]
fn orchestrated_mutation_requires_recorded_approval() {
    let project = project_root();
    let original = "print('old')\n";
    fs::write(project.as_path().join("needs_approval.py"), original).unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let orchestration = call_tool(
        &state,
        "orchestrate_change",
        json!({
            "task": "update approved flow",
            "target_paths": ["needs_approval.py"]
        }),
    );
    assert_eq!(orchestration["success"], json!(true));
    let analysis_id = orchestration["data"]["analysis_id"].as_str().unwrap();
    let run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    let run_id = run["data"]["content"]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let denied = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "needs_approval.py",
            "old_text": "old",
            "new_text": "new",
            "orchestration_run_id": run_id
        }),
    );
    assert_eq!(denied["success"], json!(false));
    assert!(
        denied["error"]
            .as_str()
            .unwrap_or("")
            .contains("recorded approval")
    );
    assert_eq!(
        fs::read_to_string(project.as_path().join("needs_approval.py")).unwrap(),
        original
    );
}

#[test]
fn orchestrate_change_granted_approval_allows_orchestrated_mutation() {
    let project = project_root();
    fs::write(project.as_path().join("approved_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let orchestration = call_tool(
        &state,
        "orchestrate_change",
        json!({
            "task": "update approved gate file",
            "target_paths": ["approved_gate.py"],
            "approval": {
                "decision": "granted",
                "actor": "integration-test",
                "reason": "bounded test mutation",
                "approved_actions": ["mutation"]
            }
        }),
    );
    assert_eq!(orchestration["success"], json!(true));
    let analysis_id = orchestration["data"]["analysis_id"].as_str().unwrap();
    let run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    let run_id = run["data"]["content"]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(run["data"]["content"]["state"], json!("executing"));
    let dispatch = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "dispatch_plan"}),
    );
    assert_eq!(dispatch["data"]["content"]["dispatch_allowed"], json!(true));
    let audit = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "audit_events"}),
    );
    assert!(
        audit["data"]["content"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "approval_granted")
    );

    let payload = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "approved_gate.py",
            "old_text": "old",
            "new_text": "new",
            "orchestration_run_id": run_id
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["orchestration_event"]["event"],
        json!("mutation_applied")
    );
    assert!(
        fs::read_to_string(project.as_path().join("approved_gate.py"))
            .unwrap()
            .contains("new")
    );

    let after_mutation_events = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "audit_events"}),
    );
    assert!(
        after_mutation_events["data"]["content"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "mutation_applied")
    );

    let verify = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "verify approved gate mutation",
            "changed_files": ["approved_gate.py"],
            "orchestration_run_id": run_id
        }),
    );
    assert_eq!(verify["success"], json!(true));
    assert_eq!(
        verify["data"]["orchestration_event"]["event"],
        json!("verification_passed")
    );

    let final_run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    assert_eq!(final_run["data"]["content"]["state"], json!("completed"));
    let final_events = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "audit_events"}),
    );
    assert!(
        final_events["data"]["content"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "verification_passed")
    );
}

#[test]
fn list_orchestration_runs_returns_ui_ready_summaries() {
    let project = project_root();
    fs::write(
        project.as_path().join("run_list.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let orchestration = call_tool(
        &state,
        "orchestrate_change",
        json!({
            "task": "track run list summary",
            "target_paths": ["run_list.py"],
            "mode": "solo"
        }),
    );
    assert_eq!(orchestration["success"], json!(true));
    let analysis_id = orchestration["data"]["analysis_id"].as_str().unwrap();
    let run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    let run_id = run["data"]["content"]["run_id"].as_str().unwrap();

    let list = call_tool(&state, "list_orchestration_runs", json!({"limit": 10}));
    assert_eq!(list["success"], json!(true));
    assert!(list["data"]["count"].as_u64().unwrap() >= 1);
    let runs = list["data"]["runs"].as_array().unwrap();
    let item = runs
        .iter()
        .find(|item| item["run_id"] == run_id)
        .expect("run summary");
    assert_eq!(item["analysis_id"], json!(analysis_id));
    assert_eq!(item["state"], json!("approval_required"));
    assert_eq!(
        item["resume"]["next_required_event"],
        json!("approval_granted")
    );
    assert!(
        item["section_handles"]
            .as_array()
            .map(|handles| !handles.is_empty())
            .unwrap_or(false)
    );
}

#[test]
fn get_orchestration_run_replays_events_and_resume_hint() {
    let project = project_root();
    fs::write(
        project.as_path().join("run_get.py"),
        "def beta():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let orchestration = call_tool(
        &state,
        "orchestrate_change",
        json!({
            "task": "inspect resumable run",
            "target_paths": ["run_get.py"],
            "mode": "planner_builder"
        }),
    );
    assert_eq!(orchestration["success"], json!(true));
    let analysis_id = orchestration["data"]["analysis_id"].as_str().unwrap();
    let run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    let run_id = run["data"]["content"]["run_id"].as_str().unwrap();

    let payload = call_tool(
        &state,
        "get_orchestration_run",
        json!({"run_id": run_id, "include_sections": true}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["analysis_id"], json!(analysis_id));
    assert_eq!(payload["data"]["state"], json!("approval_required"));
    assert_eq!(payload["data"]["event_count"], json!(4));
    assert!(
        payload["data"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "approval_requested")
    );
    assert_eq!(
        payload["data"]["resume"]["recommended_next_tools"][0],
        json!("orchestrate_change")
    );
    assert!(payload["data"]["sections"]["plan"].is_object());
}

#[test]
fn cancel_orchestration_run_appends_event_and_revokes_approval() {
    let project = project_root();
    fs::write(project.as_path().join("cancel_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let orchestration = call_tool(
        &state,
        "orchestrate_change",
        json!({
            "task": "cancel approved run",
            "target_paths": ["cancel_gate.py"],
            "approval": {
                "decision": "granted",
                "actor": "integration-test",
                "reason": "test cancellation",
                "approved_actions": ["mutation"]
            }
        }),
    );
    assert_eq!(orchestration["success"], json!(true));
    let analysis_id = orchestration["data"]["analysis_id"].as_str().unwrap();
    let run = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "orchestration_run"}),
    );
    let run_id = run["data"]["content"]["run_id"].as_str().unwrap();

    let cancelled = call_tool(
        &state,
        "cancel_orchestration_run",
        json!({
            "run_id": run_id,
            "actor": "integration-test",
            "reason": "no longer needed"
        }),
    );
    assert_eq!(cancelled["success"], json!(true));
    assert_eq!(cancelled["data"]["state"], json!("cancelled"));
    assert_eq!(cancelled["data"]["event"]["event"], json!("run_cancelled"));
    assert_eq!(cancelled["data"]["revoked_approvals"], json!(1));

    let denied = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "cancel_gate.py",
            "old_text": "old",
            "new_text": "new",
            "orchestration_run_id": run_id
        }),
    );
    assert_eq!(denied["success"], json!(false));
    assert!(
        denied["error"]
            .as_str()
            .unwrap_or("")
            .contains("recorded approval")
    );

    let fetched = call_tool(
        &state,
        "get_orchestration_run",
        json!({"analysis_id": analysis_id}),
    );
    assert_eq!(fetched["data"]["state"], json!("cancelled"));
    assert!(
        fetched["data"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "run_cancelled")
    );
}

#[test]
fn verify_change_readiness_returns_verifier_contract() {
    let project = project_root();
    fs::write(
        project.as_path().join("readiness_modal_ssr.py"),
        "def render_modal():\n    return 'ok'\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update modal render flow",
            "changed_files": ["readiness_modal_ssr.py"]
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["analysis_id"].is_string());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["readiness"].is_object());
    assert!(payload["data"]["verifier_checks"].is_array());
    assert_eq!(
        payload["data"]["readiness"]["test_readiness"],
        json!("caution")
    );
}

#[test]
fn refactor_safety_report_keeps_preview_payload_lean() {
    let project = project_root();
    fs::write(
        project.as_path().join("refactor_preview.py"),
        "def alpha(value):\n    return value + 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "refactor_safety_report",
        json!({
            "task": "refactor alpha safely",
            "symbol": "alpha",
            "path": "refactor_preview.py",
            "file_path": "refactor_preview.py"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();
    assert!(payload["data"]["summary"].is_string());
    assert!(payload["data"]["readiness"].is_object());
    assert!(payload["data"]["available_sections"].is_array());
    assert!(
        payload["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.ends_with("/summary"))
            .unwrap_or(false)
    );
    assert!(payload["data"]["section_handles"].is_array());
    assert!(payload["data"]["next_actions"].is_array());
    assert!(payload["data"].get("top_findings").is_none());
    assert!(payload["data"].get("verifier_checks").is_none());
    assert!(payload["data"].get("quality_focus").is_none());
    assert!(payload["data"].get("recommended_checks").is_none());
    assert!(payload["data"].get("performance_watchpoints").is_none());

    let summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31023)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&summary).unwrap();
    assert!(body.contains("top_findings"));
    assert!(body.contains("verifier_checks"));
    assert!(body.contains("quality_focus"));
    assert!(body.contains("recommended_checks"));
    assert!(body.contains("performance_watchpoints"));
}

#[test]
fn refactor_surface_requires_preflight_before_create_text_file() {
    let project = project_root();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "mutated.txt", "content": "hello"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("requires a fresh preflight")
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["mutation_without_preflight_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["mutation_preflight_gate_denied_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn verify_change_readiness_allows_same_file_mutation_and_tracks_caution() {
    let project = project_root();
    fs::write(project.as_path().join("gated.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update gated output",
            "changed_files": ["gated.py"]
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    assert_eq!(
        preflight["data"]["readiness"]["mutation_ready"],
        json!("caution")
    );

    let payload = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "gated.py",
            "old_text": "old",
            "new_text": "new"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(
        fs::read_to_string(project.as_path().join("gated.py"))
            .unwrap()
            .contains("new")
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["mutation_with_caution_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn builder_minimal_mutation_behavior_unchanged() {
    let project = project_root();
    fs::write(project.as_path().join("builder_import.py"), "print('hi')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));

    let payload = call_tool(
        &state,
        "add_import",
        json!({
            "file_path": "builder_import.py",
            "import_statement": "import os"
        }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn review_changes_returns_structured_content() {
    // review_changes with changed_files delegates to diff_aware_references.
    let project = project_root();
    fs::write(project.as_path().join("review_test.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "review_changes",
        json!({"changed_files": ["review_test.py"]}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("review_changes"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("diff_aware_references")
    );
    assert!(
        payload["suggested_next_calls"]
            .as_array()
            .map(|items| {
                items.iter().any(|entry| {
                    entry["tool"] == json!("impact_report")
                        && entry["arguments"]["changed_files"].is_array()
                }) && items.iter().any(|entry| {
                    entry["tool"] == json!("diagnose_issues")
                        && entry["arguments"]["path"] == json!("review_test.py")
                })
            })
            .unwrap_or(false),
        "expected forwarded impact/diagnose follow-ups: {payload}"
    );
    assert!(
        !payload["suggested_next_tools"]
            .as_array()
            .map(|items| items
                .iter()
                .any(|value| value == "delegate_to_codex_builder"))
            .unwrap_or(false),
        "non-builder follow-ups should not emit a codex-builder delegate scaffold: {payload}"
    );
}

#[test]
fn plan_safe_refactor_without_symbol_uses_safety_report() {
    let project = project_root();
    fs::write(project.as_path().join("ref.py"), "def old(): pass\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "plan_safe_refactor", json!({"task": "rename old"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("plan_safe_refactor"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("refactor_safety_report")
    );
}

#[test]
fn plan_safe_refactor_with_symbol_uses_rename_report() {
    let project = project_root();
    fs::write(project.as_path().join("ref.py"), "def old(): pass\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "plan_safe_refactor",
        json!({"file_path": "ref.py", "symbol": "old", "new_name": "new_name"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("plan_safe_refactor"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("safe_rename_report")
    );
}
