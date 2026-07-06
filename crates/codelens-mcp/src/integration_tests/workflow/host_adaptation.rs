use super::*;

fn note_contains(payload: &serde_json::Value, needle: &str) -> bool {
    payload["data"]["host_environment"]["adaptation_notes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|note| note.as_str().is_some_and(|text| text.contains(needle)))
}

#[test]
fn prepare_harness_session_codex_fixture_binds_skill_roots_from_host_context() {
    let project = project_root();
    fs::write(
        project.as_path().join("AGENTS.md"),
        "# CodeLens Routing\n\nLoad selected SKILL.md files only after metadata shortlist.\n",
    )
    .unwrap();
    let skill_root = project.as_path().join(".codex/skills");
    let skill = skill_root.join("rust/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(
        &skill,
        r#"---
name: rust-codelens
description: Use for Rust MCP semantic embedding and CodeLens harness work.
---
"#,
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "project": project.as_path(),
            "host_context": "codex",
            "task_overlay": "editing",
            "task": "러스트 MCP semantic embedding 문제 해결",
            "skill_roots": [skill_root.to_string_lossy().to_string()],
            "detail": "compact"
        }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        json!("codex")
    );
    assert!(note_contains(&payload, "Codex host_context"));
    assert_eq!(
        payload["data"]["skill_hints"]["load_policy"],
        json!("shortlist from metadata first, then read only selected SKILL.md files")
    );
    assert!(
        payload["data"]["skill_hints"]["candidate_skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|skill| skill["name"] == "rust-codelens")
    );
}

#[test]
fn prepare_harness_session_codex_context_drives_tool_contract() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "project": project.as_path(),
            "profile": "builder-minimal",
            "host_context": "codex",
            "detail": "full"
        }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        json!("codex")
    );
    assert_eq!(
        payload["data"]["http_session"]["client_profile"],
        json!("codex")
    );
    assert_eq!(payload["data"]["config"]["client_profile"], json!("codex"));
    assert_eq!(
        payload["data"]["http_session"]["default_tools_list_contract_mode"],
        json!("lean")
    );
    assert_eq!(
        payload["data"]["visible_tools"]["deferred_loading_active"],
        json!(true)
    );
}

#[test]
fn prepare_harness_session_codex_context_reports_default_skill_roots() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "project": project.as_path(),
            "host_context": "codex",
            "task_overlay": "editing",
            "task": "use installed Codex skills without loading every SKILL.md",
            "detail": "compact"
        }),
    );

    let skill_hint_root_count = payload["data"]["skill_hints"]["roots"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();

    assert_eq!(payload["success"], json!(true));
    assert!(skill_hint_root_count > 0);
    assert_eq!(
        payload["data"]["host_environment"]["skill_root_count"],
        json!(skill_hint_root_count)
    );
    assert_eq!(
        payload["data"]["host_environment"]["skill_root_source"],
        json!("codex_default_roots")
    );
    assert!(note_contains(&payload, "Codex default skill roots"));
}

#[test]
fn prepare_harness_session_claude_fixture_surfaces_policy_and_memory_bounds() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join(".claude/memory")).unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "project": project.as_path(),
            "host_context": "claude-code",
            "task_overlay": "review",
            "agent_role": "main",
            "memory_roots": [project.as_path().join(".claude/memory").to_string_lossy().to_string()],
            "host_setting_keys": ["managed_settings", "permissions.deny", "mcp.servers"],
            "harness_profile": "planner-readonly",
            "detail": "compact"
        }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        json!("claude")
    );
    assert_eq!(payload["data"]["skill_hints"], serde_json::Value::Null);
    assert_eq!(
        payload["data"]["host_environment"]["memory_root_count"],
        json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_setting_key_count"],
        json!(3)
    );
    assert_eq!(payload["data"]["routing"]["agent_role"], json!("main"));
    assert!(note_contains(&payload, "managed"));
    assert!(note_contains(&payload, "Memory roots"));
}

#[test]
fn prepare_harness_session_generic_fixture_degrades_without_skills_or_memory() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "project": project.as_path(),
            "detail": "compact"
        }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        json!("generic")
    );
    assert_eq!(
        payload["data"]["host_environment"]["snapshot_source"],
        json!("session_defaults")
    );
    assert_eq!(payload["data"]["skill_hints"], serde_json::Value::Null);
    assert!(note_contains(
        &payload,
        "No explicit host settings snapshot"
    ));
}
