use super::*;

#[test]
fn prepare_harness_session_warns_when_daemon_binary_is_stale() {
    // Two layers of flake-resistance already in place from prior PRs:
    //
    //  1. `TEST_ENV_LOCK` (PRs #174/#177/#184/#185/#187/#292) — prevents
    //     other env-touching tests from racing this one's
    //     `set_var(CODELENS_EXECUTABLE_PATH_OVERRIDE)` and `call_tool`.
    //  2. A 1.5 s `thread::sleep` — tried to push the override file's
    //     `mtime` into a later wall-clock second than `daemon_started_at`.
    //
    // Layer 2 was still racy under heavy CI scheduler load and contributed
    // to recurrences observed during the PR-K..#334 sweep (single-shot
    // ubuntu fail with the rest of the matrix green). The mtime guarantee
    // depended on `SystemTime::now()` advancing through a full second
    // boundary during the sleep, which on a contended CI runner is not
    // bounded by the requested sleep duration. Same root-cause class as
    // #332 (`subsec_nanos`-only paths colliding under parallel load): the
    // fixture relied on wall-clock side-effects rather than a deterministic
    // assignment.
    //
    // Fix: set the override file's `mtime` explicitly via `filetime` to
    // `daemon_started_at + 10 s`. The `mtime > daemon_started_seconds`
    // relationship is now a property of the assignment, not of the
    // runner's scheduling — race window collapsed to zero, sleep removed
    // (~1.5 s reclaimed per run).
    let _env_guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let project = project_root();
    fs::write(
        project.as_path().join("stale_daemon.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let override_path = std::env::temp_dir().join(format!(
        "codelens-stale-daemon-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&override_path, "newer-binary-marker").unwrap();

    // Pin the override file's mtime explicitly to a point well after
    // `daemon_started_at` (which `make_state` snapshots from
    // `SystemTime::now()` a moment ago). The 10 s headroom comfortably
    // clears any plausible second-boundary issue without depending on
    // the runner advancing wall-clock time.
    let future_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    filetime::set_file_mtime(
        &override_path,
        filetime::FileTime::from_system_time(future_mtime),
    )
    .unwrap();

    let previous = std::env::var_os("CODELENS_EXECUTABLE_PATH_OVERRIDE");
    // SAFETY: this test mutates a process env var while holding
    // `TEST_ENV_LOCK`; no other env-touching test can run concurrently.
    unsafe {
        std::env::set_var("CODELENS_EXECUTABLE_PATH_OVERRIDE", &override_path);
    }

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "full"}),
    );

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EXECUTABLE_PATH_OVERRIDE", value),
            None => std::env::remove_var("CODELENS_EXECUTABLE_PATH_OVERRIDE"),
        }
    }
    let _ = fs::remove_file(&override_path);

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
        json!({"profile": "builder-minimal", "file_path": "diagnose.unknown", "detail": "full"}),
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

// 2026-07 tool-surface diet: semantic_search left the reviewer-graph
// curated core surface, so the "does not nag" assertion now targets
// builder-minimal, which still lists semantic_search. The behaviour under
// test — a surface that exposes semantic_search must not emit the
// "switch surfaces" warning — is unchanged.
#[cfg(feature = "semantic")]
#[test]
fn prepare_harness_session_builder_minimal_does_not_report_semantic_surface_gap() {
    let project = project_root();
    fs::write(
        project.as_path().join("review_surface.rs"),
        "fn alpha() -> i32 {\n    1\n}\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "full"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_ne!(
        payload["data"]["capabilities"]["semantic_search_status"],
        json!("not_in_active_surface"),
        "builder-minimal should expose semantic_search; any semantic warning should be about assets or index state"
    );
    assert!(
        !payload["data"]["warnings"]
            .as_array()
            .map(|warnings| {
                warnings
                    .iter()
                    .any(|warning| warning["code"] == "semantic_not_in_active_surface")
            })
            .unwrap_or(false),
        "prepare_harness_session must not tell builder-minimal users to switch surfaces for semantic_search"
    );
}

#[cfg(feature = "semantic")]
#[test]
fn prepare_harness_session_reports_existing_embedding_index_as_ready() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    if let Err(err) = fs::write(
        project.as_path().join("semantic_ready.py"),
        "def alpha():\n    return 1\n",
    ) {
        panic!("failed to write semantic fixture: {err}");
    }
    let _bootstrap = make_state(&project);
    let engine = match codelens_engine::EmbeddingEngine::new(&project) {
        Ok(engine) => engine,
        Err(err) => panic!("failed to create embedding engine: {err}"),
    };
    let indexed = match engine.index_from_project(&project) {
        Ok(indexed) => indexed,
        Err(err) => panic!("failed to index semantic fixture: {err}"),
    };
    assert!(indexed > 0);
    drop(engine);
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "full"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["project"]["embedding_ready"],
        json!(true),
        "prepare_harness_session should not report semantic embeddings missing when a usable on-disk embedding index exists"
    );
    assert_eq!(
        payload["data"]["capabilities"]["embedding_indexed"],
        json!(true)
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
fn prepare_harness_session_warns_when_active_project_differs_without_explicit_project() {
    let default_project = project_root();
    let other_project = project_root();
    fs::write(
        other_project.as_path().join("other.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&default_project);
    state
        .switch_project(other_project.as_path().to_str().expect("utf8 path"))
        .unwrap();

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"detail": "compact"}),
    );

    assert_eq!(payload["success"], json!(true));
    let warnings = payload["data"]["warnings"].as_array().expect("warnings");
    let warning = warnings
        .iter()
        .find(|warning| warning["code"] == "active_project_differs_from_daemon_default")
        .expect("active project warning");
    assert_eq!(
        warning["recommended_action"],
        json!("verify_or_activate_explicit_project")
    );
    assert_eq!(warning["action_target"], json!("active_project"));
    assert_eq!(warning["restart_recommended"], json!(false));
    assert_eq!(
        warning["remediation"]["tool"],
        json!("prepare_harness_session")
    );
    assert_eq!(
        warning["remediation"]["args"]["project"],
        json!(default_project.as_path().to_string_lossy().to_string())
    );
    assert_eq!(warning["native_fallback_recommended"], json!(false));
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
        json!({"profile": "builder-minimal", "detail": "full"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["health_summary"].is_object());
    assert_eq!(
        payload["data"]["health_summary"],
        payload["data"]["capabilities"]["health_summary"]
    );
    assert!(payload["data"]["health_summary"]["status"].is_string());
    // T3: the full response is passed through `strip_empty_fields`, so an empty
    // `warnings` array is dropped for token economy. When present it must still
    // be an array; when absent the health summary was "ok" with zero warnings.
    if let Some(warnings) = payload["data"]["health_summary"].get("warnings") {
        assert!(warnings.is_array());
    }
}

#[test]
fn prepare_harness_session_warns_when_client_tool_schema_fingerprint_is_stale() {
    let project = project_root();
    fs::write(
        project.as_path().join("schema_stale.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "builder-minimal",
            "detail": "compact",
            "known_tool_schema_fingerprint": "stale-client-fingerprint",
        }),
    );

    assert_eq!(payload["success"], json!(true));
    let generation = &payload["data"]["surface_generation"];
    assert_eq!(
        generation["refresh_action"],
        json!("reissue_tools_list_or_reconnect")
    );
    let server_fingerprint = generation["tool_schema_fingerprint"]
        .as_str()
        .expect("server fingerprint");
    assert_eq!(server_fingerprint.len(), 64);

    let warning = payload["data"]["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .find(|warning| warning["code"] == "tool_schema_cache_stale")
        .expect("stale tool schema warning");
    assert_eq!(warning["restart_recommended"], json!(true));
    assert_eq!(
        warning["recommended_action"],
        json!("reissue_tools_list_or_reconnect")
    );
    assert_eq!(warning["action_target"], json!("tool_schema_cache"));
    assert_eq!(
        warning["client_tool_schema_fingerprint"],
        json!("stale-client-fingerprint")
    );
    assert_eq!(
        warning["server_tool_schema_fingerprint"],
        json!(server_fingerprint)
    );
}

#[test]
fn prepare_harness_session_auto_refreshes_small_stale_index() {
    let project = project_root();
    let path = project.as_path().join("stale_bootstrap.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();
    let state = make_state(&project);

    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let parent = path.parent().unwrap();
    if !parent.exists() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, "def alpha():\n    return 2\n").unwrap();

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "full"}),
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
fn prepare_harness_session_auto_refreshes_large_stale_index_by_default() {
    let project = project_root();
    let stale_count = 40usize;
    for idx in 0..stale_count {
        fs::write(
            project.as_path().join(format!("large_stale_{idx}.py")),
            format!("def stale_{idx}():\n    return 1\n"),
        )
        .unwrap();
    }
    let state = make_state(&project);

    let future_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    for idx in 0..stale_count {
        let path = project.as_path().join(format!("large_stale_{idx}.py"));
        fs::write(&path, format!("def stale_{idx}():\n    return 2\n")).unwrap();
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(future_mtime))
            .unwrap();
    }

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "full"}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["index_recovery"]["threshold"],
        serde_json::Value::Null
    );
    assert_eq!(
        payload["data"]["index_recovery"]["status"],
        json!("refreshed")
    );
    assert_eq!(
        payload["data"]["index_recovery"]["before"]["stale_files"],
        json!(stale_count)
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
                .any(|warning| warning["code"] == "index_refresh_skipped"))
            .unwrap_or(false)
    );
}

#[test]
fn prepare_harness_session_respects_explicit_stale_threshold() {
    let project = project_root();
    for idx in 0..3 {
        fs::write(
            project.as_path().join(format!("threshold_stale_{idx}.py")),
            format!("def threshold_stale_{idx}():\n    return 1\n"),
        )
        .unwrap();
    }
    let state = make_state(&project);

    let future_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    for idx in 0..3 {
        let path = project.as_path().join(format!("threshold_stale_{idx}.py"));
        fs::write(
            &path,
            format!("def threshold_stale_{idx}():\n    return 2\n"),
        )
        .unwrap();
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(future_mtime))
            .unwrap();
    }

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "builder-minimal",
            "auto_refresh_stale_threshold": 2,
            "detail": "full",
        }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["index_recovery"]["status"],
        json!("skipped")
    );
    assert_eq!(payload["data"]["index_recovery"]["threshold"], json!(2));
    assert!(
        payload["data"]["warnings"]
            .as_array()
            .map(|warnings| warnings
                .iter()
                .any(|warning| warning["code"] == "index_refresh_skipped"))
            .unwrap_or(false)
    );
}

#[test]
fn prepare_harness_session_schema_matches_payload_shape() {
    let tool =
        crate::tool_defs::tool_definition("prepare_harness_session").expect("prepare_harness tool");
    let input_properties = tool.input_schema["properties"]
        .as_object()
        .expect("input schema properties");
    assert!(input_properties.contains_key("agent_role"));
    let schema = tool
        .output_schema
        .as_ref()
        .expect("prepare_harness_session schema");

    let properties = schema["properties"].as_object().expect("schema properties");
    assert!(properties.contains_key("project"));
    assert!(properties.contains_key("capabilities"));
    assert!(properties.contains_key("health_summary"));
    assert!(properties.contains_key("warnings"));
    assert!(properties.contains_key("skill_hints"));
    assert!(properties.contains_key("surface_generation"));
    assert!(properties.contains_key("host_environment"));
    assert!(!properties.contains_key("overlay"));
    assert!(properties.contains_key("index_recovery"));
    assert!(properties.contains_key("visible_tools"));
    assert!(properties.contains_key("routing"));
    assert!(properties.contains_key("harness"));
    let host_environment = schema["properties"]["host_environment"]["properties"]
        .as_object()
        .expect("host_environment properties");
    assert!(host_environment.contains_key("host_context"));
    let http_session = schema["properties"]["http_session"]["properties"]
        .as_object()
        .expect("http_session properties");
    assert!(http_session.contains_key("health_summary"));
    assert!(http_session.contains_key("daemon_binary_drift"));
    assert!(http_session.contains_key("supported_files"));
    assert!(http_session.contains_key("stale_files"));
    let routing = schema["properties"]["routing"]["properties"]
        .as_object()
        .expect("routing properties");
    assert!(routing.contains_key("preferred_entrypoints_omitted"));
    assert!(routing.contains_key("preferred_entrypoints_with_executors"));
    assert!(routing.contains_key("agent_role"));
    assert!(routing.contains_key("recommended_entrypoint_preferred_executor"));
    assert!(input_properties.contains_key("available_mcp_servers"));
    assert!(input_properties.contains_key("available_mcp_tools"));
    assert!(input_properties.contains_key("skill_roots"));
    assert!(input_properties.contains_key("memory_roots"));
    assert!(input_properties.contains_key("host_setting_keys"));
}

#[test]
fn prepare_harness_session_codex_context_exposes_skill_hints() {
    let project = project_root();
    fs::write(
        project.as_path().join("codex_skill_hint.rs"),
        "fn alpha() -> i32 { 1 }\n",
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
            "detail": "compact"
        }),
    );

    assert_eq!(
        payload["data"]["skill_hints"]["target_host"],
        json!("codex")
    );
    assert_eq!(
        payload["data"]["skill_hints"]["catalog_resource"],
        json!(crate::skill_catalog::CODEX_SKILL_CATALOG_RESOURCE_URI)
    );
    assert_eq!(payload["data"]["skill_hints"]["selection_limit"], json!(3));
    assert!(payload["data"]["skill_hints"]["roots"].is_array());
    assert!(payload["data"]["skill_hints"]["candidate_skills"].is_array());
}

#[test]
fn prepare_harness_session_uses_host_observed_skill_roots() {
    let project = project_root();
    fs::write(
        project.as_path().join("codex_host_skill_hint.rs"),
        "fn alpha() -> i32 { 1 }\n",
    )
    .unwrap();
    let skill_root = project.as_path().join("host-skills");
    let skill_path = skill_root.join("rust/SKILL.md");
    fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
    fs::write(
        &skill_path,
        r#"---
name: rust-host-skill
description: Use for Rust MCP semantic embedding and CodeLens harness debugging.
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
            "task": "러스트 MCP semantic embedding 문제",
            "file_path": "crates/codelens-mcp/src/main.rs",
            "skill_roots": [skill_root.to_string_lossy().to_string()],
            "available_mcp_servers": ["codelens", "context7", "github"],
            "available_mcp_tools": ["mcp__codelens__prepare_harness_session", "mcp__context7__query-docs"],
            "memory_roots": [project.as_path().join("memory").to_string_lossy().to_string()],
            "host_setting_keys": ["sandbox_mode", "approval_policy"],
            "detail": "compact"
        }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["host_environment"]["snapshot_source"],
        json!("explicit_host_snapshot")
    );
    assert_eq!(
        payload["data"]["host_environment"]["skill_root_count"],
        json!(1)
    );
    assert_eq!(
        payload["data"]["host_environment"]["available_mcp_tool_count"],
        json!(2)
    );
    assert_eq!(
        payload["data"]["skill_hints"]["total_skill_count"],
        json!(1)
    );
    assert!(
        payload["data"]["skill_hints"]["candidate_skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|skill| skill["name"] == "rust-host-skill")
    );
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
        json!({"profile": "builder-minimal", "detail": "full"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["routing"]["preferred_entrypoints_source"],
        json!("surface_default")
    );
    // Phase-2: builder bootstrap routes through the verb facades —
    // overview leads, and request tracing rides graph(mode=trace).
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        json!("overview")
    );
    assert!(
        payload["data"]["routing"]["preferred_entrypoints"]
            .as_array()
            .map(|items| items.iter().any(|value| value == "graph"))
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
            "profile": "builder-minimal",
            "host_context": "claude-code",
            "task_overlay": "review",
            "detail": "full"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"].get("overlay").is_none());
    assert_eq!(
        payload["data"]["routing"]["preferred_entrypoints_source"],
        json!("overlay")
    );
    // analyze_change_request joined tools.toml in #346 (ghost resolution),
    // so the claude-code host sequence now resolves it as the first visible
    // entrypoint ahead of the review-overlay audit lane.
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        json!("analyze_change_request")
    );
}

#[test]
fn prepare_harness_session_agent_role_compiles_worker_routing() {
    let project = project_root();
    fs::write(
        project.as_path().join("worker_overlay.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "builder-minimal",
            "agent_role": "subagent",
            "detail": "full"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"].get("overlay").is_none());
    assert_eq!(payload["data"]["routing"]["agent_role"], json!("subagent"));
    assert_eq!(
        payload["data"]["routing"]["preferred_entrypoints_source"],
        json!("overlay")
    );
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        json!("trace_request_path")
    );
    assert!(
        payload["data"]["routing"]["preferred_entrypoints"]
            .as_array()
            .map(|items| items.iter().any(|value| value == "get_ranked_context"))
            .unwrap_or(false)
    );
}

// Issue #199-B-1: compact-mode response trims `tool_names` to the first 5
// and `preferred_entrypoints_visible` to whatever the routing layer can see,
// but historically gave the caller no signal of how much was dropped. Both
// blocks must now expose `*_omitted_count` so callers can budget a follow-up
// without re-issuing `detail=full` just to learn the surface size.
#[test]
fn prepare_harness_session_compact_exposes_visible_tools_omitted_count() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_visible.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal", "detail": "compact"}),
    );
    assert_eq!(payload["success"], json!(true));

    let visible_tools = &payload["data"]["visible_tools"];
    let tool_count = visible_tools["tool_count"]
        .as_u64()
        .expect("tool_count present in compact response");
    let default_listed_tool_count = visible_tools["default_listed_tool_count"]
        .as_u64()
        .expect("default_listed_tool_count present in compact response");
    assert!(
        (5..=9).contains(&default_listed_tool_count),
        "default tools/list slice should stay in the 5-9 range, got {default_listed_tool_count}"
    );
    let default_listed_names = visible_tools["default_listed_tool_names"]
        .as_array()
        .expect("default_listed_tool_names present in compact response")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    for name in &default_listed_names {
        assert!(
            crate::tool_defs::default_listed_tool_names().contains(name),
            "{name} must come from tools.toml default_visible_rank"
        );
    }
    let trimmed_names = visible_tools["tool_names"]
        .as_array()
        .expect("tool_names array present in compact response");
    assert!(
        trimmed_names.len() <= 5,
        "compact response must cap tool_names at 5, got {}",
        trimmed_names.len()
    );
    let omitted = visible_tools["tool_names_omitted_count"]
        .as_u64()
        .expect("tool_names_omitted_count present in compact response");
    assert_eq!(
        omitted,
        tool_count.saturating_sub(trimmed_names.len() as u64),
        "tool_names_omitted_count must match tool_count - len(tool_names)"
    );
    // refactor-full surface ships well over five tools, so the omitted
    // count is necessarily positive — guards against the field collapsing
    // back to a no-op constant.
    assert!(
        omitted > 0,
        "builder-minimal compact response should report a positive omitted count, got {omitted}"
    );
}

#[test]
fn prepare_harness_session_compact_exposes_routing_omitted_count() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_routing.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "reviewer-graph",
            "detail": "compact",
            "preferred_entrypoints": [
                "review_changes",
                "find_tests",
                "this_tool_does_not_exist_xyz",
            ],
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let routing = &payload["data"]["routing"];
    let visible = routing["preferred_entrypoints_visible"]
        .as_array()
        .expect("preferred_entrypoints_visible array present in compact response");
    let omitted = routing["preferred_entrypoints_visible_omitted_count"]
        .as_u64()
        .expect("preferred_entrypoints_visible_omitted_count present in compact response");
    let omitted_entrypoints = routing["preferred_entrypoints_omitted"]
        .as_array()
        .expect("preferred_entrypoints_omitted array present in compact response");
    // One requested entrypoint is valid but hidden from reviewer-graph, and
    // one is invalid. The compact response must name both cases so a host can
    // distinguish "switch surface" from "fix the requested entrypoint".
    assert_eq!(
        omitted,
        (3u64).saturating_sub(visible.len() as u64),
        "routing omitted count must equal requested - visible"
    );
    assert!(
        omitted >= 2,
        "two synthetic invalid entrypoints must surface as omitted, got {omitted}"
    );
    assert_eq!(omitted_entrypoints.len() as u64, omitted);
    assert_eq!(
        omitted_entrypoints,
        &vec![
            json!({
                "tool": "find_tests",
                "reason": "not_in_active_surface",
                "recommended_action": "switch_tool_surface",
                "preferred_executor": "any",
                "tool_tier": "primitive",
                "recommended_profile": "builder",
                "included_in": [
                    "preset:balanced",
                    "preset:full",
                    "builder",
                ],
            }),
            json!({
                "tool": "this_tool_does_not_exist_xyz",
                "reason": "unknown_tool",
                "recommended_action": "fix_preferred_entrypoint",
            }),
        ]
    );
}

#[test]
fn prepare_harness_session_omitted_entrypoints_include_executor_and_tier() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_routing_executor.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "reviewer-graph",
            "detail": "compact",
            "preferred_entrypoints": [
                "review_changes",
                "plan_safe_refactor",
                "this_tool_does_not_exist_xyz",
            ],
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let omitted = payload["data"]["routing"]["preferred_entrypoints_omitted"]
        .as_array()
        .expect("preferred_entrypoints_omitted array");
    let known = omitted
        .iter()
        .find(|entry| entry["tool"] == "plan_safe_refactor")
        .expect("known hidden entrypoint");
    assert_eq!(
        known["preferred_executor"],
        json!("claude"),
        "known omitted entrypoints must keep executor routing metadata"
    );
    assert_eq!(
        known["tool_tier"],
        json!("workflow"),
        "known omitted entrypoints must keep tier routing metadata"
    );

    let unknown = omitted
        .iter()
        .find(|entry| entry["tool"] == "this_tool_does_not_exist_xyz")
        .expect("unknown hidden entrypoint");
    assert!(
        unknown.get("preferred_executor").is_none(),
        "unknown entrypoints must not invent executor metadata"
    );
    assert!(
        unknown.get("tool_tier").is_none(),
        "unknown entrypoints must not invent tier metadata"
    );
}

#[test]
fn prepare_harness_session_omitted_entrypoints_distinguish_deferred_tools() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_routing_deferred.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "reviewer-graph",
            "detail": "compact",
            "_session_client_name": "codex-mcp-client",
            "_session_deferred_tool_loading": true,
            "_session_loaded_namespaces": [],
            "_session_loaded_tiers": [],
            "_session_full_tool_exposure": false,
            "preferred_entrypoints": [
                "review_changes",
                "diff_aware_references",
                "find_tests",
            ],
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let omitted = payload["data"]["routing"]["preferred_entrypoints_omitted"]
        .as_array()
        .expect("preferred_entrypoints_omitted array");
    let deferred = omitted
        .iter()
        .find(|entry| entry["tool"] == "diff_aware_references")
        .expect("known active-surface tool hidden by deferred loading");
    assert_eq!(
        deferred["reason"],
        json!("deferred_tool_not_loaded"),
        "active-surface tools hidden by deferred loading must not be reported as surface mismatches"
    );
    assert_eq!(
        deferred["recommended_action"],
        json!("load_deferred_tool_namespace"),
        "deferred tools must tell hosts to expand the deferred tool surface"
    );
    assert_eq!(
        deferred["tool_namespace"],
        json!("reports"),
        "deferred recovery must name the namespace to load"
    );
    assert_eq!(
        deferred["tool_loading_request"],
        json!({
            "method": "tools/list",
            "params": {
                "namespace": "reports",
                "tier": "workflow",
            },
        }),
        "deferred recovery must expose a replayable namespace expansion request"
    );
    assert_eq!(deferred["tool_tier"], json!("workflow"));
    assert!(
        deferred["included_in"]
            .as_array()
            .expect("included_in")
            .iter()
            .any(|value| value == "review"),
        "active profile should still be visible in recovery metadata"
    );

    let hidden_surface_tool = omitted
        .iter()
        .find(|entry| entry["tool"] == "find_tests")
        .expect("tool outside reviewer-graph");
    assert_eq!(
        hidden_surface_tool["reason"],
        json!("not_in_active_surface"),
        "tools outside the active profile should keep surface-switch guidance"
    );
}

#[test]
fn prepare_harness_session_normalizes_mcp_prefixed_entrypoints() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_routing_prefixed.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "reviewer-graph",
            "detail": "compact",
            "preferred_entrypoints": [
                "mcp__codelens__review_changes",
                "mcp__codelens__find_tests",
                "mcp__codelens__this_tool_does_not_exist_xyz",
            ],
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let routing = &payload["data"]["routing"];
    assert_eq!(
        routing["preferred_entrypoints_visible"],
        json!(["review_changes"]),
        "MCP-prefixed visible tools should resolve to canonical tool names"
    );
    assert_eq!(
        routing["preferred_entrypoints_visible_omitted_count"],
        json!(2),
        "prefixed hidden/unknown tools should still count as omitted diagnostics"
    );

    let omitted = routing["preferred_entrypoints_omitted"]
        .as_array()
        .expect("preferred_entrypoints_omitted array");
    let hidden_surface_tool = omitted
        .iter()
        .find(|entry| entry["tool"] == "find_tests")
        .expect("known hidden entrypoint should be normalized");
    assert_eq!(
        hidden_surface_tool["requested_tool"],
        json!("mcp__codelens__find_tests")
    );
    assert_eq!(
        hidden_surface_tool["reason"],
        json!("not_in_active_surface"),
        "prefixed known tools must not be misclassified as unknown_tool"
    );
    assert_eq!(
        hidden_surface_tool["recommended_action"],
        json!("switch_tool_surface")
    );

    let unknown = omitted
        .iter()
        .find(|entry| entry["tool"] == "this_tool_does_not_exist_xyz")
        .expect("unknown prefixed entrypoint");
    assert_eq!(
        unknown["requested_tool"],
        json!("mcp__codelens__this_tool_does_not_exist_xyz")
    );
    assert_eq!(unknown["reason"], json!("unknown_tool"));
}

#[test]
fn prepare_harness_session_text_payload_preserves_compact_routing_recovery_fields() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_text_routing.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = crate::server::router::handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "prepare_harness_session",
                "arguments": {
                    "_session_id": default_session_id(&state),
                    "profile": "reviewer-graph",
                    "detail": "compact",
                    "preferred_entrypoints": [
                        "review_changes",
                        "find_tests",
                        "this_tool_does_not_exist_xyz",
                    ],
                }
            })),
        },
    )
    .expect("tools/call should return a response");
    let raw = serde_json::to_value(response).expect("serialize response");
    let text = raw["result"]["content"][0]["text"]
        .as_str()
        .expect("text fallback");
    let payload = parse_tool_payload(text);
    let data = &payload["data"];

    assert!(
        data["visible_tools"]["tool_names_omitted_count"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "text fallback must keep compact visible tool omission metadata"
    );
    assert_eq!(
        data["routing"]["preferred_entrypoints_visible"],
        json!(["review_changes"]),
        "text fallback must keep visible routing entrypoints"
    );
    assert_eq!(
        data["routing"]["preferred_entrypoints_visible_omitted_count"],
        json!(2),
        "text fallback must keep routing omission count"
    );
    assert_eq!(
        data["routing"]["preferred_entrypoints_omitted"],
        json!([
            {
                "tool": "find_tests",
                "reason": "not_in_active_surface",
                "recommended_action": "switch_tool_surface",
                "preferred_executor": "any",
                "tool_tier": "primitive",
                "included_in": [
                    "preset:balanced",
                    "preset:full",
                    "builder",
                ],
                "recommended_profile": "builder",
            },
            {
                "tool": "this_tool_does_not_exist_xyz",
                "reason": "unknown_tool",
                "recommended_action": "fix_preferred_entrypoint",
            },
        ]),
        "text fallback must preserve actionable omitted-entrypoint records"
    );
}

#[test]
fn prepare_harness_session_omitted_entrypoint_surfaces_exclude_deprecated_profiles() {
    let project = project_root();
    fs::write(
        project.as_path().join("compact_deprecated_surfaces.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({
            "profile": "reviewer-graph",
            "detail": "compact",
            "preferred_entrypoints": [
                "review_changes",
                "plan_safe_refactor",
                "trace_request_path",
            ],
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let omitted = payload["data"]["routing"]["preferred_entrypoints_omitted"]
        .as_array()
        .expect("preferred_entrypoints_omitted array");
    let deprecated_profiles = [
        "evaluator-compact",
        "refactor-full",
        "ci-audit",
        "workflow-first",
    ];
    for entry in omitted {
        let included_in = entry["included_in"]
            .as_array()
            .expect("included_in array for known omitted tool");
        for profile in deprecated_profiles {
            assert!(
                !included_in.iter().any(|value| value == profile),
                "omitted entrypoint recovery metadata must not recommend deprecated profile {profile}: {entry}"
            );
        }
    }
}
