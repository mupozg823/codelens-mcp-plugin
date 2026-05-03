use super::*;

#[test]
fn prepare_harness_session_warns_when_daemon_binary_is_stale() {
    let project = project_root();
    fs::write(
        project.as_path().join("stale_daemon.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let mut state = make_state(&project);
    state.set_daemon_started_at_for_test("1970-01-01T00:00:00Z");

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["capabilities"]["daemon_binary_drift"]["status"],
        json!("stale")
    );
    assert_eq!(
        payload["data"]["capabilities"]["daemon_binary_drift"]["stale_daemon"],
        json!(true)
    );
    assert_eq!(
        payload["data"]["capabilities"]["daemon_binary_drift"]["reason_code"],
        json!("stale_daemon_binary")
    );
    assert_eq!(
        payload["data"]["capabilities"]["daemon_binary_drift"]["recommended_action"],
        json!("restart_mcp_server")
    );
    assert!(
        payload["data"]["capabilities"]["health_summary"]["warnings"]
            .as_array()
            .map(|warnings| {
                warnings
                    .iter()
                    .any(|warning| warning["code"] == "stale_daemon_binary")
            })
            .unwrap_or(false)
    );
    assert_eq!(
        payload["data"]["health_summary"],
        payload["data"]["capabilities"]["health_summary"]
    );
    assert!(
        payload["data"]["warnings"]
            .as_array()
            .map(|warnings| {
                warnings.iter().any(|warning| {
                    warning["code"] == "stale_daemon_binary"
                        && warning["restart_recommended"] == json!(true)
                        && warning["recommended_action"] == json!("restart_mcp_server")
                        && warning["action_target"] == json!("daemon")
                })
            })
            .unwrap_or(false)
    );
}

#[test]
fn prepare_harness_session_warns_when_diagnostics_recipe_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("diagnose.unknown"), "hello\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "file_path": "diagnose.unknown"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["capabilities"]["diagnostics_guidance"]["status"],
        json!("unsupported_extension")
    );
    assert!(
        payload["data"]["warnings"]
            .as_array()
            .map(|warnings| {
                warnings.iter().any(|warning| {
                    warning["code"] == "diagnostics_unsupported_extension"
                        && warning["restart_recommended"] == json!(false)
                        && warning["recommended_action"] == json!("pass_explicit_lsp_command")
                        && warning["action_target"] == json!("file_extension")
                })
            })
            .unwrap_or(false)
    );
}

#[test]
fn prepare_harness_session_warning_codes_are_unique() {
    let project = project_root();
    fs::write(
        project.as_path().join("unique.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "file_path": "unique.py"}),
    );

    assert_eq!(payload["success"], json!(true));
    let codes = payload["data"]["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|warning| {
            warning
                .get("code")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    let unique = codes
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(codes.len(), unique.len());
}

#[test]
fn prepare_harness_session_surfaces_top_level_health_summary() {
    let project = project_root();
    fs::write(
        project.as_path().join("bootstrap.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["health_summary"].is_object());
    assert_eq!(
        payload["data"]["health_summary"],
        payload["data"]["capabilities"]["health_summary"]
    );
    assert!(payload["data"]["health_summary"]["status"].is_string());
    assert!(payload["data"]["health_summary"]["warnings"].is_array());
}

#[test]
fn prepare_harness_session_auto_refreshes_small_stale_index() {
    let project = project_root();
    let path = project.as_path().join("stale_bootstrap.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();
    let state = make_state(&project);

    let parent = path.parent().unwrap();
    if !parent.exists() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, "def alpha():\n    return 2\n").unwrap();
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(future)).unwrap();

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["index_recovery"]["status"],
        json!("refreshed")
    );
    assert_eq!(
        payload["data"]["index_recovery"]["before"]["stale_files"],
        json!(1)
    );
    assert_eq!(
        payload["data"]["index_recovery"]["after"]["stale_files"],
        json!(0)
    );
    assert!(
        !payload["data"]["warnings"]
            .as_array()
            .map(|warnings| warnings
                .iter()
                .any(|warning| warning["code"] == "stale_index"))
            .unwrap_or(false)
    );
}

#[test]
fn prepare_harness_session_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("prepare_harness_session")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("prepare_harness_session schema");

    let properties = schema["properties"].as_object().expect("schema properties");
    assert!(properties.contains_key("project"));
    assert!(properties.contains_key("capabilities"));
    assert!(properties.contains_key("health_summary"));
    assert!(properties.contains_key("warnings"));
    assert!(properties.contains_key("overlay"));
    assert!(properties.contains_key("index_recovery"));
    assert!(properties.contains_key("visible_tools"));
    assert!(properties.contains_key("routing"));
    assert!(properties.contains_key("harness"));
    let http_session = schema["properties"]["http_session"]["properties"]
        .as_object()
        .expect("http_session properties");
    assert!(http_session.contains_key("health_summary"));
    assert!(http_session.contains_key("daemon_binary_drift"));
    assert!(http_session.contains_key("supported_files"));
    assert!(http_session.contains_key("stale_files"));
    let overlay = schema["properties"]["overlay"]["properties"]
        .as_object()
        .expect("overlay properties");
    assert!(overlay.contains_key("host_context"));
    assert!(overlay.contains_key("task_overlay"));
    assert!(overlay.contains_key("preferred_entrypoints_visible"));
    let routing = schema["properties"]["routing"]["properties"]
        .as_object()
        .expect("routing properties");
    assert!(routing.contains_key("preferred_entrypoints_with_executors"));
    assert!(routing.contains_key("recommended_entrypoint_preferred_executor"));
}

#[test]
fn prepare_harness_session_defaults_to_surface_bootstrap_entrypoints() {
    let project = project_root();
    fs::write(
        project.as_path().join("bootstrap.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["routing"]["preferred_entrypoints_source"],
        json!("surface_default")
    );
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        json!("explore_codebase")
    );
    assert!(
        payload["data"]["routing"]["preferred_entrypoints"]
            .as_array()
            .map(|items| items.iter().any(|value| value == "trace_request_path"))
            .unwrap_or(false)
    );
}

#[test]
fn prepare_harness_session_overlay_can_override_bootstrap_routing() {
    let project = project_root();
    fs::write(
        project.as_path().join("overlay.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "refactor-full",
            "host_context": "claude-code",
            "task_overlay": "review"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["overlay"]["applied"], json!(true));
    assert_eq!(
        payload["data"]["overlay"]["host_context"],
        json!("claude-code")
    );
    assert_eq!(payload["data"]["overlay"]["task_overlay"], json!("review"));
    assert_eq!(
        payload["data"]["routing"]["preferred_entrypoints_source"],
        json!("overlay")
    );
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        json!("review_changes")
    );
    assert!(
        payload["data"]["overlay"]["avoid_tools"]
            .as_array()
            .map(|items| items.iter().any(|value| value == "rename_symbol"))
            .unwrap_or(false)
    );
    assert!(
        payload["data"]["overlay"]["routing_notes"]
            .as_array()
            .map(|items| items.iter().any(|value| {
                value
                    .as_str()
                    .map(|text| text.contains("Review overlay"))
                    .unwrap_or(false)
            }))
            .unwrap_or(false)
    );
}
