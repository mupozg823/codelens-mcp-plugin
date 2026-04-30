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
            id: Some(json!(3102_3)),
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
