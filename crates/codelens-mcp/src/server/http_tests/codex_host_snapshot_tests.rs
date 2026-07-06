use super::*;

#[tokio::test]
async fn prepare_harness_session_uses_codex_initialize_host_snapshot() -> anyhow::Result<()> {
    let state = test_state();
    let skill_root = state.project().as_path().join("host-skills");
    let skill_path = skill_root.join("rust/SKILL.md");
    let skill_parent = skill_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("skill path has no parent"))?;
    std::fs::create_dir_all(skill_parent)?;
    std::fs::write(
        &skill_path,
        r#"---
name: http-rust-host-skill
description: Use for Rust MCP semantic embedding and CodeLens harness debugging.
---
"#,
    )?;
    let memory_root = state.project().as_path().join("host-memory");
    std::fs::create_dir_all(&memory_root)?;
    std::fs::write(memory_root.join("AGENTS.md"), "# Memory routing\n")?;
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"CodexHarness","version":"1.0.0"}},"hostContext":"codex","availableMcpServers":["codelens","context7"],"availableMcpTools":["mcp__codelens__prepare_harness_session","mcp__context7__query-docs"],"skillRoots":["{}"],"memoryRoots":["{}"],"hostSettingKeys":["sandbox_mode","approval_policy"],"harnessProfile":"builder-minimal"}}}}"#,
                    skill_root.display(),
                    memory_root.display()
                )))?,
        )
        .await?;

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing mcp-session-id"))?
        .to_owned();

    let session = state
        .session_store
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing session store"))?
        .get(&sid)
        .ok_or_else(|| anyhow::anyhow!("missing initialized session"))?;
    let metadata = session.client_metadata();
    assert_eq!(metadata.host_context.as_deref(), Some("codex"));
    assert_eq!(metadata.available_mcp_servers.len(), 2);
    assert_eq!(metadata.available_mcp_tools.len(), 2);
    assert_eq!(metadata.skill_roots.len(), 1);
    assert_eq!(metadata.memory_roots.len(), 1);
    assert_eq!(metadata.host_setting_keys.len(), 2);
    assert_eq!(metadata.harness_profile.as_deref(), Some("builder-minimal"));

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","task":"Rust MCP semantic embedding issue","detail":"compact"}}}}}}"#,
                    state.project().as_path().display()
                )))?,
        )
        .await?;

    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(bootstrap).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        serde_json::json!("codex")
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_context"],
        serde_json::json!("codex")
    );
    assert_eq!(
        payload["data"]["host_environment"]["snapshot_source"],
        serde_json::json!("explicit_host_snapshot")
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_server_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_tool_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        payload["data"]["host_environment"]["skill_root_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["skill_root_source"],
        serde_json::json!("host_snapshot")
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_root_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoint_count"],
        serde_json::json!(1)
    );
    let memory_entrypoints = payload["data"]["host_environment"]["memory_entrypoints"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing memory entrypoints"))?;
    assert!(
        memory_entrypoints
            .iter()
            .any(|entry| entry["relative_path"] == "AGENTS.md" && entry["kind"] == "host_policy")
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_setting_key_count"],
        serde_json::json!(2)
    );
    let candidate_skills = payload["data"]["skill_hints"]["candidate_skills"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing candidate skill hints"))?;
    assert!(
        candidate_skills
            .iter()
            .any(|skill| skill["name"] == "http-rust-host-skill")
    );
    Ok(())
}

#[tokio::test]
async fn prepare_harness_session_degrades_with_malformed_codex_memory_roots() -> anyhow::Result<()>
{
    let state = test_state();
    let skill_root = state.project().as_path().join("empty-host-skills");
    std::fs::create_dir_all(&skill_root)?;
    let file_root = state.project().as_path().join("memory-root-file");
    std::fs::write(&file_root, "not a directory\n")?;
    let missing_root = state.project().as_path().join("missing-memory-root");
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "initialize",
                        "params": {
                            "clientInfo": {"name": "CodexHarness", "version": "1.0.0"},
                            "hostContext": "codex",
                            "skillRoots": [skill_root.to_string_lossy().to_string()],
                            "memoryRoots": [
                                file_root.to_string_lossy().to_string(),
                                missing_root.to_string_lossy().to_string()
                            ],
                            "hostSettingKeys": ["sandbox_mode"]
                        }
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing mcp-session-id"))?
        .to_owned();
    let session = state
        .session_store
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing session store"))?
        .get(&sid)
        .ok_or_else(|| anyhow::anyhow!("missing initialized session"))?;
    let metadata = session.client_metadata();
    assert_eq!(metadata.host_context.as_deref(), Some("codex"));
    assert_eq!(metadata.skill_roots.len(), 1);
    assert_eq!(metadata.memory_roots.len(), 2);

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","task":"bootstrap malformed memory roots","detail":"compact"}}}}}}"#,
                    state.project().as_path().display()
                )))?,
        )
        .await?;

    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(bootstrap).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        serde_json::json!("codex")
    );
    assert_eq!(
        payload["data"]["host_environment"]["snapshot_source"],
        serde_json::json!("explicit_host_snapshot")
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_root_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoint_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoints"],
        serde_json::json!([])
    );
    let notes = payload["data"]["host_environment"]["adaptation_notes"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing adaptation notes"))?;
    assert!(notes.iter().any(|note| {
        note.as_str()
            .is_some_and(|text| text.contains("Codex host_context"))
    }));
    assert!(notes.iter().any(|note| {
        note.as_str()
            .is_some_and(|text| text.contains("Memory roots were observed"))
    }));
    Ok(())
}
