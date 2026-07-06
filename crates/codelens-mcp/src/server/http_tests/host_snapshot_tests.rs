use super::*;

fn compact_host_note_contains(payload: &serde_json::Value, needle: &str) -> bool {
    payload["data"]["host_environment"]["adaptation_notes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|note| note.as_str().is_some_and(|text| text.contains(needle)))
}

#[tokio::test]
async fn prepare_harness_session_uses_claude_initialize_host_snapshot() -> anyhow::Result<()> {
    let state = test_state();
    let memory_root = state.project().as_path().join("claude-memory");
    std::fs::create_dir_all(&memory_root)?;
    std::fs::write(memory_root.join("memory_summary.md"), "summary\n")?;
    std::fs::write(memory_root.join("MEMORY.md"), "registry\n")?;
    std::fs::create_dir_all(memory_root.join("skills"))?;
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"Claude Code","version":"2.1.94"}},"hostContext":"claude-code","availableMcpServers":["codelens","github"],"availableMcpTools":["mcp__codelens__review_changes","mcp__github__get_pull_request"],"memoryRoots":["{}"],"hostSettingKeys":["managed_settings","permissions.deny","mcp.servers"],"harnessProfile":"planner-readonly"}}}}"#,
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
    assert_eq!(metadata.client_name.as_deref(), Some("Claude Code"));
    assert_eq!(metadata.host_context.as_deref(), Some("claude-code"));
    assert_eq!(metadata.available_mcp_servers.len(), 2);
    assert_eq!(metadata.available_mcp_tools.len(), 2);
    assert_eq!(metadata.memory_roots.len(), 1);
    assert_eq!(metadata.host_setting_keys.len(), 3);
    assert_eq!(
        metadata.harness_profile.as_deref(),
        Some("planner-readonly")
    );

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","task":"review managed Claude Code host","task_overlay":"review","detail":"compact"}}}}}}"#,
                    state.project().as_path().display()
                )))?,
        )
        .await?;

    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(bootstrap).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        serde_json::json!("claude")
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_context"],
        serde_json::json!("claude-code")
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
        serde_json::json!(0)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_root_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoint_count"],
        serde_json::json!(3)
    );
    let memory_entrypoints = payload["data"]["host_environment"]["memory_entrypoints"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing memory entrypoints"))?;
    assert!(
        memory_entrypoints.iter().any(
            |entry| entry["relative_path"] == "memory_summary.md" && entry["kind"] == "summary"
        )
    );
    assert!(
        memory_entrypoints
            .iter()
            .any(|entry| entry["relative_path"] == "MEMORY.md" && entry["kind"] == "registry")
    );
    assert!(
        memory_entrypoints
            .iter()
            .any(|entry| entry["relative_path"] == "skills" && entry["kind"] == "skills_dir")
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_setting_key_count"],
        serde_json::json!(3)
    );
    assert_eq!(payload["data"]["skill_hints"], serde_json::Value::Null);
    assert!(compact_host_note_contains(
        &payload,
        "Claude Code host_context"
    ));
    assert!(compact_host_note_contains(
        &payload,
        "Host-observed MCP tool inventory"
    ));
    assert!(compact_host_note_contains(&payload, "managed"));
    Ok(())
}

#[tokio::test]
async fn prepare_harness_session_uses_generic_initialize_tool_snapshot() -> anyhow::Result<()> {
    let state = test_state();
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Generic MCP Client","version":"1.0.0"},"availableMcpTools":["mcp__codelens__prepare_harness_session"]}}"#,
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
    assert_eq!(metadata.client_name.as_deref(), Some("Generic MCP Client"));
    assert_eq!(metadata.host_context.as_deref(), None);
    assert_eq!(metadata.available_mcp_servers.len(), 0);
    assert_eq!(metadata.available_mcp_tools.len(), 1);
    assert_eq!(metadata.skill_roots.len(), 0);
    assert_eq!(metadata.memory_roots.len(), 0);

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","detail":"compact"}}}}}}"#,
                    state.project().as_path().display()
                )))?,
        )
        .await?;

    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(bootstrap).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["client_profile"],
        serde_json::json!("generic")
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_context"],
        serde_json::Value::Null
    );
    assert_eq!(
        payload["data"]["host_environment"]["snapshot_source"],
        serde_json::json!("explicit_host_snapshot")
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_tool_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["skill_root_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_root_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoint_count"],
        serde_json::json!(0)
    );
    assert_eq!(payload["data"]["skill_hints"], serde_json::Value::Null);
    assert!(compact_host_note_contains(
        &payload,
        "Host-observed MCP tool inventory"
    ));
    Ok(())
}
