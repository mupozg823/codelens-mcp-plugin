use super::*;

#[test]
fn unresolved_reference_check_blocks_missing_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("references.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "unresolved_reference_check",
        json!({"file_path": "references.py", "symbol": "missing_symbol"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["readiness"]["reference_safety"],
        json!("blocked")
    );
    assert_eq!(
        payload["data"]["readiness"]["mutation_ready"],
        json!("blocked")
    );
    assert!(
        payload["data"]["blocker_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn safe_rename_report_blocked_preflight_blocks_rename_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_guard.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "safe_rename_report",
        json!({
            "file_path": "rename_guard.py",
            "symbol": "missing_symbol",
            "new_name": "renamed_symbol"
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    assert_eq!(
        preflight["data"]["readiness"]["mutation_ready"],
        json!("blocked")
    );

    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_guard.py",
            "symbol_name": "missing_symbol",
            "new_name": "renamed_symbol",
            "dry_run": true
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("blocked by verifier readiness")
    );
}

#[test]
fn rename_symbol_requires_symbol_aware_preflight() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_need_preflight.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "rename old_name in rename_need_preflight.py",
            "changed_files": ["rename_need_preflight.py"]
        }),
    );
    assert_eq!(preflight["success"], json!(true));

    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_need_preflight.py",
            "symbol_name": "old_name",
            "new_name": "new_name",
            "dry_run": true
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("symbol-aware preflight")
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["rename_without_symbol_preflight_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}
