use super::*;

fn host_note_contains(payload: &serde_json::Value, needle: &str) -> bool {
    payload["data"]["host_environment"]["adaptation_notes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|note| note.as_str().is_some_and(|text| text.contains(needle)))
}

fn session_id_from_initialize(init: axum::response::Response) -> anyhow::Result<String> {
    init.headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing mcp-session-id"))
}

#[tokio::test]
async fn prepare_harness_session_uses_cline_initialize_host_snapshot() -> anyhow::Result<()> {
    let state = test_state();
    let memory_root = state.project().as_path().join("missing-cline-memory");
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
                            "clientInfo": {"name": "Cline", "version": "4.0.0"},
                            "hostContext": "cline",
                            "availableMcpServers": ["codelens", "github", "filesystem"],
                            "availableMcpTools": [
                                "mcp__codelens__review_changes",
                                "mcp__codelens__get_file_diagnostics"
                            ],
                            "memoryRoots": [memory_root.to_string_lossy()],
                            "hostSettingKeys": ["cline.mcpServers", "cline.rules.locked"],
                            "harnessProfile": "review-diagnostics"
                        }
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    let sid = session_id_from_initialize(init)?;
    let session = state
        .session_store
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing session store"))?
        .get(&sid)
        .ok_or_else(|| anyhow::anyhow!("missing initialized session"))?;
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_name.as_deref(), Some("Cline"));
    assert_eq!(metadata.host_context.as_deref(), Some("cline"));
    assert_eq!(metadata.available_mcp_servers.len(), 3);
    assert_eq!(metadata.available_mcp_tools.len(), 2);
    assert_eq!(metadata.memory_roots.len(), 1);
    assert_eq!(metadata.host_setting_keys.len(), 2);
    assert_eq!(
        metadata.harness_profile.as_deref(),
        Some("review-diagnostics")
    );

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","task":"review Cline diagnostics host","task_overlay":"review","detail":"compact"}}}}}}"#,
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
        serde_json::json!("cline")
    );
    assert_eq!(
        payload["data"]["host_environment"]["snapshot_source"],
        serde_json::json!("explicit_host_snapshot")
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_server_count"],
        serde_json::json!(3)
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_tool_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_root_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoint_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        payload["data"]["host_environment"]["host_setting_key_count"],
        serde_json::json!(2)
    );
    assert!(host_note_contains(&payload, "host_context hint"));
    assert!(host_note_contains(
        &payload,
        "Host-observed MCP tool inventory"
    ));
    assert!(host_note_contains(&payload, "managed or locked"));
    Ok(())
}

#[tokio::test]
async fn prepare_harness_session_uses_windsurf_initialize_host_snapshot() -> anyhow::Result<()> {
    let state = test_state();
    let memory_root = state.project().as_path().join("windsurf-memory");
    std::fs::create_dir_all(memory_root.join("rollout_summaries"))?;
    std::fs::write(memory_root.join("MEMORY.md"), "registry\n")?;
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
                            "clientInfo": {"name": "Windsurf", "version": "1.12.0"},
                            "hostContext": "windsurf",
                            "availableMcpServers": [
                                "codelens",
                                "github",
                                "filesystem",
                                "browser",
                                "context7",
                                "supabase"
                            ],
                            "availableMcpTools": [
                                "mcp__codelens__prepare_harness_session",
                                "mcp__codelens__explore_codebase",
                                "mcp__context7__query-docs"
                            ],
                            "memoryRoots": [memory_root.to_string_lossy()],
                            "hostSettingKeys": ["windsurf.mcp_config", "workspace.policy.locked"],
                            "harnessProfile": "compact-workflow"
                        }
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    let sid = session_id_from_initialize(init)?;
    let session = state
        .session_store
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing session store"))?
        .get(&sid)
        .ok_or_else(|| anyhow::anyhow!("missing initialized session"))?;
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_name.as_deref(), Some("Windsurf"));
    assert_eq!(metadata.host_context.as_deref(), Some("windsurf"));
    assert_eq!(metadata.available_mcp_servers.len(), 6);
    assert_eq!(metadata.available_mcp_tools.len(), 3);
    assert_eq!(metadata.memory_roots.len(), 1);
    assert_eq!(
        metadata.harness_profile.as_deref(),
        Some("compact-workflow")
    );

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","task":"bootstrap Windsurf bounded context","task_overlay":"onboarding","detail":"compact"}}}}}}"#,
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
        serde_json::json!("windsurf")
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_server_count"],
        serde_json::json!(6)
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_tool_count"],
        serde_json::json!(3)
    );
    assert_eq!(
        payload["data"]["host_environment"]["memory_entrypoint_count"],
        serde_json::json!(2)
    );
    let memory_entrypoints = payload["data"]["host_environment"]["memory_entrypoints"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing memory entrypoints"))?;
    assert!(
        memory_entrypoints
            .iter()
            .any(|entry| entry["relative_path"] == "MEMORY.md" && entry["kind"] == "registry")
    );
    assert!(
        memory_entrypoints
            .iter()
            .any(|entry| entry["relative_path"] == "rollout_summaries"
                && entry["kind"] == "rollout_summaries_dir")
    );
    assert!(host_note_contains(&payload, "host_context hint"));
    assert!(host_note_contains(&payload, "managed or locked"));
    Ok(())
}
