use super::*;

// ── Composite / workflow tool tests ──────────────────────────────────

#[test]
fn onboard_project_returns_structure() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.py"),
        "class App:\n    def run(self):\n        pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "onboard_project", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["directory_structure"].is_array());
    assert!(payload["data"]["key_files"].is_array());
    assert!(payload["data"]["semantic"].get("status").is_some());
}

#[test]
fn onboard_project_uses_existing_embedding_index_without_loading_engine() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.py"),
        "class App:\n    def run(self):\n        return 'ok'\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);

    let engine = codelens_engine::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);

    let payload = call_tool(&state, "onboard_project", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["semantic"]["status"], json!("ready"));
    assert_eq!(
        payload["data"]["semantic"]["model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert_eq!(
        payload["data"]["semantic"]["indexed_symbols"],
        json!(indexed)
    );
    assert_eq!(payload["data"]["semantic"]["loaded"], json!(false));
}

#[cfg(feature = "semantic")]
#[test]
fn impact_report_surfaces_unavailable_semantic_status() {
    let project = project_root();
    fs::write(
        project.as_path().join("impact_semantic_missing.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "impact_report",
        json!({"path": "impact_semantic_missing.py"}),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id should be present");
    assert!(payload["data"]["available_sections"]
        .as_array()
        .map(|sections| sections.iter().any(|section| section == "semantic_status"))
        .unwrap_or(false));
    assert!(payload["data"]["next_actions"]
        .as_array()
        .map(|actions| {
            actions
                .iter()
                .filter_map(|value| value.as_str())
                .any(|value| value.contains("index_embeddings"))
        })
        .unwrap_or(false));

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "semantic_status"}),
    );
    assert_eq!(section["success"], json!(true));
    #[cfg(feature = "semantic")]
    let expected_status = "unavailable";
    #[cfg(not(feature = "semantic"))]
    let expected_status = "not_compiled";
    assert_eq!(section["data"]["content"]["status"], json!(expected_status));
    #[cfg(feature = "semantic")]
    let expected_reason_fragment = "index_embeddings";
    #[cfg(not(feature = "semantic"))]
    let expected_reason_fragment = "not compiled";
    assert!(section["data"]["content"]["reason"]
        .as_str()
        .unwrap_or("")
        .contains(expected_reason_fragment));
}

#[cfg(feature = "semantic")]
#[test]
fn impact_report_uses_existing_embedding_index_for_semantic_status() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    fs::write(
        project.as_path().join("impact_semantic_ready.py"),
        "def ember_archive_delta():\n    return 1\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);

    let engine = codelens_engine::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);
    assert!(state.embedding_ref().is_none());

    let payload = call_tool(
        &state,
        "impact_report",
        json!({"path": "impact_semantic_ready.py"}),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id should be present");

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "semantic_status"}),
    );
    assert_eq!(section["success"], json!(true));
    assert_eq!(section["data"]["content"]["status"], json!("ready"));
    assert_eq!(
        section["data"]["content"]["indexed_symbols"],
        json!(indexed)
    );
}

/// P1-C: dispatch envelope aliases `file_path` ↔ `path` so callers
/// may use either name. Verified end-to-end through `tools/call`:
/// the same tool accepts both and produces an equivalent response.
#[test]
fn dispatch_aliases_file_path_and_path_arguments() {
    let project = project_root();
    fs::write(project.as_path().join("alias_check.py"), "x = 1\n").unwrap();
    let state = make_state(&project);

    // Caller sends `file_path` — handler reads it normally.
    let with_file_path = call_tool(
        &state,
        "get_capabilities",
        json!({ "file_path": "alias_check.py", "detail": "compact" }),
    );
    assert_eq!(with_file_path["success"], json!(true));
    assert_eq!(
        with_file_path["data"]["language"],
        json!("py"),
        "file_path should resolve language"
    );

    // Caller sends `path` instead — alias lifts it into `file_path`
    // before dispatch, handler reads it equally well.
    let with_path = call_tool(
        &state,
        "get_capabilities",
        json!({ "path": "alias_check.py", "detail": "compact" }),
    );
    assert_eq!(with_path["success"], json!(true));
    assert_eq!(
        with_path["data"]["language"],
        json!("py"),
        "path alias should resolve language identically"
    );

    // The two responses should be equivalent on the language field;
    // we do not pin every byte because runtime introspection (binary
    // build sha, daemon start time) is identical here too.
    assert_eq!(
        with_file_path["data"]["language"],
        with_path["data"]["language"]
    );
}

/// P1-A: `detail=compact` returns the 12 core fields only and trims
/// response size meaningfully. Backward-compat: `detail=full` (and
/// the unset default) keeps the historical 38-field shape — covered by
/// the existing `get_capabilities_returns_features` test below.
#[test]
fn get_capabilities_compact_returns_core_fields_only() {
    let project = project_root();
    fs::write(project.as_path().join("check.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_capabilities",
        json!({"file_path": "check.py", "detail": "compact"}),
    );
    assert_eq!(payload["success"], json!(true));
    let data = &payload["data"];

    // Core fields the LLM consumes on a startup probe.
    for field in [
        "language",
        "lsp_attached",
        "intelligence_sources",
        "semantic_search_status",
        "embedding_model",
        "embedding_indexed",
        "embedding_indexed_symbols",
        "index_fresh",
        "available",
        "unavailable",
        "binary_version",
        "binary_git_sha",
    ] {
        assert!(
            data.get(field).is_some(),
            "compact mode must include core field {field}, payload={data}"
        );
    }
    assert_eq!(
        data["detail_available"],
        json!(["full"]),
        "compact mode must hint that detail=full unlocks the full shape"
    );

    // Fields that ONLY appear in detail=full must be absent from
    // compact. This is the budget-saving payoff.
    for excluded in [
        "diagnostics_guidance",
        "embeddings_loaded",
        "embedding_runtime_preference",
        "embedding_runtime_backend",
        "embedding_threads",
        "embedding_max_length",
        "embedding_coreml_model_format",
        "embedding_coreml_compute_units",
        "indexed_files",
        "supported_files",
        "stale_files",
        "health_summary",
        "binary_build_time",
        "daemon_started_at",
        "daemon_binary_drift",
        "binary_build_info",
        "scip_available",
        "scip_file_count",
        "scip_symbol_count",
    ] {
        assert!(
            data.get(excluded).is_none(),
            "compact mode must NOT include verbose field {excluded}, but got {:?}",
            data[excluded]
        );
    }

    // Size guard: compact payload should fit comfortably under 2 KB
    // serialised. Full mode is observed at ~5 KB — this test fails
    // closed if a future change accidentally re-bloats compact.
    // Size guard: compact payload should fit comfortably under the
    // full-mode shape (~5 KB observed). Local-no-SCIP measures ~1 KB,
    // CI-with-SCIP-index measures ~2.2 KB because intelligence_sources
    // includes scip + the unavailable[].reason strings expand on
    // indexed projects. 2.5 KB keeps a margin while still guarding
    // against accidental re-bloat into full-mode territory.
    let compact_size = serde_json::to_string(data).unwrap().len();
    assert!(
        compact_size < 2_560,
        "compact payload must stay under 2.5 KB, got {compact_size} bytes"
    );
}

#[test]
fn get_capabilities_returns_features() {
    let project = project_root();
    fs::write(project.as_path().join("check.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "get_capabilities", json!({"file_path": "check.py"}));
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["available"].is_array());
    assert!(payload["data"].get("lsp_attached").is_some());
    assert!(payload["data"]["diagnostics_guidance"].is_object());
    assert!(payload["data"]["diagnostics_guidance"]
        .get("recommended_action")
        .is_some());
    assert!(payload["data"]["diagnostics_guidance"]
        .get("reason_code")
        .is_some());
    assert!(payload["data"].get("embeddings_loaded").is_some());
    assert_eq!(
        payload["data"]["embedding_model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert!(payload["data"].get("semantic_search_status").is_some());
    assert!(payload["data"].get("embedding_indexed").is_some());
    assert!(payload["data"].get("embedding_indexed_symbols").is_some());
    assert!(payload["data"].get("index_fresh").is_some());
    assert!(payload["data"].get("supported_files").is_some());
    assert!(payload["data"].get("stale_files").is_some());
    assert!(payload["data"]["health_summary"].is_object());
    assert!(payload["data"]["health_summary"]["status"].is_string());
    assert!(payload["data"]["health_summary"]["warnings"].is_array());
    assert!(payload["data"]["daemon_binary_drift"].is_object());
    assert!(payload["data"]["daemon_binary_drift"]["status"].is_string());
    assert!(payload["data"]["daemon_binary_drift"]["stale_daemon"].is_boolean());
    assert!(payload["data"]["daemon_binary_drift"]
        .get("recommended_action")
        .is_some());
    assert!(payload["data"]["daemon_binary_drift"]
        .get("reason_code")
        .is_some());
}

#[test]
fn get_capabilities_reports_existing_embedding_index_without_loading_engine() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    fs::write(
        project.as_path().join("embed.py"),
        "def hello():\n    return 'world'\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);
    let engine = codelens_engine::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);

    let payload = call_tool(&state, "get_capabilities", json!({"file_path": "embed.py"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["embedding_model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert_eq!(payload["data"]["embedding_indexed"], json!(true));
    assert_eq!(payload["data"]["embedding_indexed_symbols"], json!(indexed));
}

#[test]
fn project_overview_resource_includes_health_summary() {
    let project = project_root();
    fs::write(
        project.as_path().join("overview.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(250)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://project/overview"})),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    let text = value["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    let payload: serde_json::Value = serde_json::from_str(text).expect("valid overview JSON");

    assert!(payload["symbol_index"].is_object() || payload["symbol_index"].is_null());
    assert!(payload["health_summary"].is_object());
    assert!(payload["health_summary"]["status"].is_string());
    assert!(payload["health_summary"]["warning_count"].is_u64());
    assert!(payload["health_summary"]["warnings"].is_array());
    assert!(payload["project_root"].is_string());
    assert!(payload["active_surface"].is_string());
}

#[test]
fn session_http_resource_includes_health_contract() {
    let project = project_root();
    fs::write(
        project.as_path().join("session.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(251)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://session/http"})),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    let text = value["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    let payload: serde_json::Value = serde_json::from_str(text).expect("valid session JSON");

    assert!(payload["active_surface"].is_string());
    assert!(payload["semantic_search_status"].is_string());
    assert!(payload["indexed_files"].is_u64());
    assert!(payload["supported_files"].is_u64());
    assert!(payload["stale_files"].is_u64());
    assert!(payload["daemon_binary_drift"].is_object());
    assert!(payload["health_summary"].is_object());
    assert!(payload["health_summary"]["status"].is_string());
    assert!(payload["health_summary"]["warnings"].is_array());
}

#[test]
fn get_capabilities_reports_diagnostics_guidance_without_file_path() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(&state, "get_capabilities", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["status"],
        json!("file_path_required")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["available"],
        json!(false)
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["reason_code"],
        json!("diagnostics_file_path_required")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["recommended_action"],
        json!("provide_file_path")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["action_target"],
        json!("file_path")
    );
}

#[test]
fn get_capabilities_reports_diagnostics_guidance_for_unsupported_extension() {
    let project = project_root();
    fs::write(project.as_path().join("notes.unknown"), "hello\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_capabilities",
        json!({"file_path": "notes.unknown"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["status"],
        json!("unsupported_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["reason_code"],
        json!("diagnostics_unsupported_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["recommended_action"],
        json!("pass_explicit_lsp_command")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["action_target"],
        json!("file_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["file_extension"],
        json!("unknown")
    );
}

#[test]
fn prepare_harness_session_warns_when_daemon_binary_is_stale() {
    let project = project_root();
    fs::write(
        project.as_path().join("stale_daemon.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    // `daemon_started_at` is second-granularity RFC3339. Sleep just over
    // one second so the override file's mtime is guaranteed to be newer.
    std::thread::sleep(std::time::Duration::from_millis(1_100));

    let override_path = std::env::temp_dir().join(format!(
        "codelens-stale-daemon-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&override_path, "newer-binary-marker").unwrap();

    let previous = std::env::var_os("CODELENS_EXECUTABLE_PATH_OVERRIDE");
    // SAFETY: this test mutates a process env var for the duration of a
    // synchronous tool call, then restores the previous value.
    unsafe {
        std::env::set_var("CODELENS_EXECUTABLE_PATH_OVERRIDE", &override_path);
    }

    let payload = call_tool(
        &state,
        "prepare_harness_session",
        json!({"profile": "builder-minimal"}),
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
    assert!(payload["data"]["warnings"]
        .as_array()
        .map(|warnings| {
            warnings.iter().any(|warning| {
                warning["code"] == "stale_daemon_binary"
                    && warning["restart_recommended"] == json!(true)
                    && warning["recommended_action"] == json!("restart_mcp_server")
                    && warning["action_target"] == json!("daemon")
            })
        })
        .unwrap_or(false));
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
    assert!(payload["data"]["warnings"]
        .as_array()
        .map(|warnings| {
            warnings.iter().any(|warning| {
                warning["code"] == "diagnostics_unsupported_extension"
                    && warning["restart_recommended"] == json!(false)
                    && warning["recommended_action"] == json!("pass_explicit_lsp_command")
                    && warning["action_target"] == json!("file_extension")
            })
        })
        .unwrap_or(false));
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

    std::thread::sleep(std::time::Duration::from_millis(1_100));
    fs::write(&path, "def alpha():\n    return 2\n").unwrap();

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
    assert!(!payload["data"]["warnings"]
        .as_array()
        .map(|warnings| warnings
            .iter()
            .any(|warning| warning["code"] == "stale_index"))
        .unwrap_or(false));
}

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
    assert!(state
        .analysis_dir()
        .join(analysis_id)
        .join("ranked_files.json")
        .exists());
}

#[test]
fn ci_audit_reports_use_fixed_machine_schema() {
    let project = project_root();
    fs::write(
        project.as_path().join("audit.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));

    let payload = call_tool(&state, "impact_report", json!({"path": "audit.py"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["profile"], json!("ci-audit"));
    assert_eq!(
        payload["data"]["schema_version"],
        json!("codelens-ci-audit-v1")
    );
    assert_eq!(payload["data"]["report_kind"], json!("impact_report"));
    assert!(payload["data"]["machine_summary"]["finding_count"].is_number());
    assert!(payload["data"]["machine_summary"]["blocker_count"].is_number());
    assert!(payload["data"]["machine_summary"]["verifier_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["ready_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["blocked_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["quality_focus_count"].is_number());
    assert!(payload["data"]["machine_summary"]["recommended_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["performance_watchpoint_count"].is_number());
    assert!(payload["data"]["evidence_handles"].is_array());
    assert!(payload["data"]["summary_resource"]["uri"]
        .as_str()
        .map(|uri| uri.ends_with("/summary"))
        .unwrap_or(false));
    assert!(payload["data"]["section_handles"].is_array());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["readiness"].is_object());
    assert!(payload["data"]["verifier_checks"].is_array());
    assert!(payload["data"]["quality_focus"].is_array());
    assert!(payload["data"]["recommended_checks"].is_array());
    assert!(payload["data"]["performance_watchpoints"].is_array());
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
    assert!(payload["data"]["summary_resource"]["uri"]
        .as_str()
        .map(|uri| uri.ends_with("/summary"))
        .unwrap_or(false));
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
fn start_analysis_job_returns_completed_handle() {
    let project = project_root();
    fs::write(
        project.as_path().join("impact.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let arguments =
        json!({"kind": "impact_report", "path": "impact.py", "profile_hint": "reviewer-graph"});
    // Store job without enqueuing to background worker — run synchronously to
    // eliminate timing dependency that causes flaky failures under parallel load.
    let job = state
        .store_analysis_job_for_current_scope(
            "impact_report",
            Some("reviewer-graph".to_owned()),
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    assert_eq!(job.status, crate::runtime_types::JobLifecycle::Queued);
    let job_id = job.id.clone();

    // Run synchronously on the test thread — same code path as the background worker.
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        job_id.clone(),
        "impact_report".to_owned(),
        arguments,
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let completed_job = state.get_analysis_job(&job_id).unwrap();
    assert_eq!(
        completed_job.status,
        crate::runtime_types::JobLifecycle::Completed
    );
    assert_eq!(completed_job.progress, 100);
    let analysis_id = completed_job.analysis_id.as_deref().unwrap();
    let poll = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    assert_eq!(poll["data"]["analysis_id"], json!(analysis_id));
    assert!(poll["data"]["summary_resource"]["uri"]
        .as_str()
        .map(|uri| uri.ends_with("/summary"))
        .unwrap_or(false));
    assert!(poll["data"]["section_handles"]
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false));

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "impact_rows"}),
    );
    assert_eq!(section["success"], json!(true));
}

#[cfg(feature = "http")]
#[test]
fn eval_session_audit_aggregates_across_tracked_sessions() {
    let project = project_root();
    fs::write(
        project.as_path().join("eval_builder_warn.py"),
        "print('old')\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("eval_planner_pass.py"),
        "print('ok')\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("eval_planner_warn.py"),
        "print('ok')\n",
    )
    .unwrap();
    let state = make_http_state(&project);
    let builder_session = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::BuilderMinimal,
    );
    let planner_pass_session = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );
    let planner_warn_session = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "eval_builder_warn.py"}),
        &builder_session,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update eval builder warn file",
            "changed_files": ["eval_builder_warn.py"]
        }),
        &builder_session,
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &planner_pass_session,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "eval_planner_pass.py"}),
        &planner_pass_session,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["eval_planner_pass.py"], "task": "review planner pass"}),
        &planner_pass_session,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "eval_planner_warn.py"}),
        &planner_warn_session,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["eval_planner_warn.py"], "task": "review planner warn"}),
        &planner_warn_session,
    );

    let arguments = json!({"kind": "eval_session_audit"});
    let job = state
        .store_analysis_job_for_current_scope(
            "eval_session_audit",
            None,
            vec!["audit_pass_rate".to_owned(), "session_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    let job_id = job.id.clone();
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        job_id.clone(),
        "eval_session_audit".to_owned(),
        arguments,
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let completed_job = state.get_analysis_job(&job_id).unwrap();
    let analysis_id = completed_job.analysis_id.as_deref().unwrap();

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "audit_pass_rate"}),
    );
    assert_eq!(section["success"], json!(true));
    let content = &section["data"]["content"];
    assert_eq!(content["tracked_session_count"], json!(3));
    assert_eq!(content["session_count"], json!(3));
    assert_eq!(content["builder_session_count"], json!(1));
    assert_eq!(content["planner_session_count"], json!(2));
    assert_eq!(content["builder_pass_rate"], json!(0.0));
    assert_eq!(content["planner_pass_rate"], json!(0.5));
    assert_eq!(
        content["top_failed_checks"][0]["code"],
        json!("bootstrap_order")
    );
    assert_eq!(content["top_failed_checks"][0]["count"], json!(2));

    let rows = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "session_rows"}),
    );
    assert_eq!(rows["success"], json!(true));
    assert_eq!(rows["data"]["content"]["count"], json!(3));
    assert!(rows["data"]["content"]["sessions"]
        .as_array()
        .map(|sessions| sessions.iter().any(|session| {
            session["role"] == json!("builder") && session["status"] == json!("warn")
        }))
        .unwrap_or(false));
}

#[test]
fn start_analysis_job_reports_running_progress() {
    let project = project_root();
    fs::write(
        project.as_path().join("progress_job.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "progress_job.py",
            "debug_step_delay_ms": 30
        }),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    assert!(payload["data"]["summary_resource"].is_null());
    assert_eq!(payload["data"]["section_handles"], json!([]));
    let mut saw_running = false;
    let mut saw_mid_progress = false;
    let mut saw_step = false;
    for _ in 0..100 {
        let job = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
        let status = job["data"]["status"].as_str().unwrap_or_default();
        let progress = job["data"]["progress"].as_u64().unwrap_or_default();
        if status == "running" {
            saw_running = true;
        }
        if (1..100).contains(&progress) {
            saw_mid_progress = true;
        }
        if job["data"]["current_step"].is_string() {
            saw_step = true;
        }
        if status == "completed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_running);
    assert!(saw_mid_progress);
    assert!(saw_step);
}

#[test]
fn analysis_job_text_payload_preserves_job_handle_fields() {
    let project = project_root();
    fs::write(
        project.as_path().join("job_text.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let start_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(4101)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "start_analysis_job",
                "arguments": { "kind": "impact_report", "path": "job_text.py", "debug_step_delay_ms": 20 }
            })),
        },
    )
    .unwrap();
    let start_payload = parse_tool_payload(&extract_tool_text(&start_response));
    assert!(start_payload["data"]["job_id"].is_string());
    assert_eq!(start_payload["routing_hint"], json!("async"));
    assert_eq!(start_payload["data"]["summary_resource"], json!(null));
    assert_eq!(start_payload["data"]["section_handles"], json!([]));

    let sync_job = state
        .store_analysis_job_for_current_scope(
            "impact_report",
            None,
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    let sync_job_id = sync_job.id.clone();
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        sync_job_id.clone(),
        "impact_report".to_owned(),
        json!({"kind": "impact_report", "path": "job_text.py"}),
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let poll_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(4102)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "get_analysis_job",
                "arguments": { "job_id": sync_job_id }
            })),
        },
    )
    .unwrap();
    let completed = parse_tool_payload(&extract_tool_text(&poll_response));
    assert!(completed["data"]["summary_resource"]["uri"]
        .as_str()
        .map(|uri| uri.ends_with("/summary"))
        .unwrap_or(false));
    assert!(completed["data"]["section_handles"]
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false));
}

#[test]
fn analysis_jobs_queue_when_worker_busy() {
    let project = project_root();
    fs::write(
        project.as_path().join("queue_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("queue_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "queue_first.py",
            "debug_step_delay_ms": 60
        }),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "queue_second.py",
            "debug_step_delay_ms": 20
        }),
    );
    let second_job_id = second["data"]["job_id"].as_str().unwrap();
    let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
    assert_eq!(second_job["data"]["status"], json!("queued"));
    assert_eq!(second_job["data"]["current_step"], json!("queued"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["analysis_jobs_enqueued"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert!(
        metrics["data"]["session"]["analysis_jobs_started"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["analysis_queue_max_depth"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_worker_limit"],
        json!(1)
    );
}

#[test]
fn reviewer_jobs_use_parallel_http_pool() {
    let project = project_root();
    fs::write(
        project.as_path().join("parallel_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("parallel_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.configure_transport_mode("http");

    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "parallel_first.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "parallel_second.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    assert_eq!(first["success"], json!(true), "first job failed: {first}");
    assert_eq!(
        second["success"],
        json!(true),
        "second job failed: {second}"
    );
    let first_job_id = first["data"]["job_id"]
        .as_str()
        .expect("first job_id should be present");
    let second_job_id = second["data"]["job_id"]
        .as_str()
        .expect("second job_id should be present");
    for _ in 0..100 {
        let metrics = call_tool(&state, "get_tool_metrics", json!({}));
        let peak_workers = metrics["data"]["session"]["peak_active_analysis_workers"]
            .as_u64()
            .unwrap_or_default();
        if peak_workers >= 2 {
            break;
        }
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
        if first_job["data"]["status"] == json!("completed")
            && second_job["data"]["status"] == json!("completed")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_worker_limit"],
        json!(2)
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_transport_mode"],
        json!("http")
    );
    assert!(
        metrics["data"]["session"]["peak_active_analysis_workers"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert_eq!(metrics["data"]["session"]["analysis_cost_budget"], json!(3));
}

#[test]
fn low_cost_jobs_bypass_heavy_jobs_in_http_queue() {
    let project = project_root();
    fs::write(
        project.as_path().join("priority_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("priority_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.configure_transport_mode("http");

    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "priority_first.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let heavy = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "dead_code_report",
            "scope": ".",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let heavy_job_id = heavy["data"]["job_id"].as_str().unwrap();

    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "priority_second.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let second_job_id = second["data"]["job_id"].as_str().unwrap();

    let mut saw_second_ahead_of_heavy = false;
    for _ in 0..100 {
        let heavy_job = call_tool(&state, "get_analysis_job", json!({"job_id": heavy_job_id}));
        let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
        if (second_job["data"]["status"] == json!("running")
            || second_job["data"]["status"] == json!("completed"))
            && heavy_job["data"]["status"] == json!("queued")
        {
            saw_second_ahead_of_heavy = true;
            break;
        }
        if heavy_job["data"]["status"] == json!("completed")
            && second_job["data"]["status"] == json!("completed")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(saw_second_ahead_of_heavy);
    assert!(
        metrics["data"]["session"]["analysis_queue_priority_promotions"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["analysis_queue_max_weighted_depth"]
            .as_u64()
            .unwrap_or_default()
            >= 4
    );
}

#[test]
fn cancel_analysis_job_marks_job_cancelled() {
    let project = project_root();
    fs::write(
        project.as_path().join("cancel_job.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("cancel_blocker.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "cancel_blocker.py", "debug_step_delay_ms": 60}),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "cancel_job.py", "debug_step_delay_ms": 50}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    let queued = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    assert_eq!(queued["data"]["status"], json!("queued"));
    let cancelled = call_tool(&state, "cancel_analysis_job", json!({"job_id": job_id}));
    assert_eq!(cancelled["data"]["status"], json!("cancelled"));
    assert!(cancelled["data"]["summary_resource"].is_null());
    assert_eq!(cancelled["data"]["section_handles"], json!([]));
}

#[test]
fn analysis_lists_expose_resource_handles_and_counts() {
    let project = project_root();
    fs::write(
        project.as_path().join("analysis_list.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let project_path = project.as_path().to_string_lossy().to_string();
    let artifact = state
        .store_analysis_for_current_scope(
            "impact_report",
            None,
            "impact summary".to_owned(),
            vec!["top finding".to_owned()],
            "medium".to_owned(),
            0.94,
            vec!["next action".to_owned()],
            Vec::new(),
            crate::runtime_types::AnalysisReadiness::default(),
            Vec::new(),
            std::collections::BTreeMap::from([("impact_rows".to_owned(), json!([]))]),
        )
        .unwrap();
    let job = state
        .store_analysis_job_for_current_scope(
            "impact_report",
            None,
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Completed,
            100,
            Some("completed".to_owned()),
            Some(artifact.id.clone()),
            None,
        )
        .unwrap();
    let job_id = job.id.as_str();

    let completed = call_tool(
        &state,
        "get_analysis_job",
        json!({"job_id": job_id, "_session_project_path": project_path.clone()}),
    );
    assert_eq!(
        completed["data"]["status"],
        json!("completed"),
        "expected job to complete before list verification: {completed}"
    );

    let jobs = state.list_analysis_jobs_for_scope(&state.current_project_scope(), None);
    assert!(!jobs.is_empty());
    let listed_job = jobs
        .iter()
        .find(|job| job.id == job_id)
        .expect("completed job should be visible in direct job list");
    assert_eq!(
        listed_job.status,
        crate::runtime_types::JobLifecycle::Completed
    );
    assert!(listed_job.analysis_id.is_some());
    assert!(!listed_job.estimated_sections.is_empty());

    let artifacts = call_tool(
        &state,
        "list_analysis_artifacts",
        json!({"_session_project_path": project_path.clone()}),
    );
    assert!(artifacts["data"]["count"].as_u64().unwrap_or_default() >= 1);
    assert!(artifacts["data"]["latest_created_at_ms"].is_u64());
    assert!(
        artifacts["data"]["tool_counts"]["impact_report"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(artifacts["data"]["artifacts"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["summary_resource"]["uri"].as_str())
        .map(|uri| uri.ends_with("/summary"))
        .unwrap_or(false));
}

#[test]
fn resources_include_profile_guides_and_analysis_summaries() {
    let project = project_root();
    fs::write(
        project.as_path().join("module.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "dead_code_report",
        json!({"scope": ".", "max_results": 5}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(21)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_response).unwrap();
    assert!(encoded.contains("codelens://profile/planner-readonly/guide"));
    assert!(encoded.contains("codelens://profile/planner-readonly/guide/full"));
    assert!(encoded.contains("codelens://tools/list/full"));
    assert!(encoded.contains("codelens://surface/manifest"));
    assert!(encoded.contains("codelens://harness/modes"));
    assert!(encoded.contains("codelens://harness/spec"));
    assert!(encoded.contains("codelens://harness/host-adapters"));
    assert!(encoded.contains("codelens://harness/host"));
    assert!(encoded.contains("codelens://design/agent-experience"));
    assert!(encoded.contains("codelens://host-adapters/claude-code"));
    assert!(encoded.contains("codelens://host-adapters/codex"));
    assert!(encoded.contains("codelens://host-adapters/cursor"));
    assert!(encoded.contains("codelens://host-adapters/windsurf"));
    assert!(encoded.contains("codelens://schemas/handoff-artifact/v1"));
    assert!(encoded.contains("codelens://session/http"));
    assert!(encoded.contains("codelens://analysis/recent"));
    assert!(encoded.contains("codelens://analysis/jobs"));
    assert!(encoded.contains(&format!("codelens://analysis/{analysis_id}/summary")));
    assert!(encoded.contains("symbiote://profile/planner-readonly/guide"));
    assert!(encoded.contains("symbiote://tools/list/full"));
    assert!(encoded.contains("symbiote://harness/host"));
    assert!(encoded.contains("symbiote://host-adapters/codex"));
    assert!(encoded.contains("symbiote://schemas/handoff-artifact/v1"));
    assert!(encoded.contains("symbiote://analysis/jobs"));
    assert!(encoded.contains(&format!("symbiote://analysis/{analysis_id}/summary")));

    let read_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&read_response).unwrap();
    assert!(body.contains("available_sections"));

    let recent_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22_1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://analysis/recent"})),
        },
    )
    .unwrap();
    let recent_body = serde_json::to_string(&recent_response).unwrap();
    assert!(recent_body.contains("summary_resource"));
    assert!(recent_body.contains("tool_counts"));

    let jobs_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22_2)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://analysis/jobs"})),
        },
    )
    .unwrap();
    let jobs_body = serde_json::to_string(&jobs_response).unwrap();
    assert!(jobs_body.contains("status_counts"));
    assert!(jobs_body.contains("active_count"));

    let tools_summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list"})),
        },
    )
    .unwrap();
    let tools_summary_body = serde_json::to_string(&tools_summary).unwrap();
    assert!(tools_summary_body.contains("recommended_tools"));
    assert!(tools_summary_body.contains("visible_namespaces"));
    assert!(tools_summary_body.contains("visible_tiers"));
    assert!(tools_summary_body.contains("all_namespaces"));
    assert!(tools_summary_body.contains("all_tiers"));
    assert!(tools_summary_body.contains("loaded_namespaces"));
    assert!(tools_summary_body.contains("loaded_tiers"));
    assert!(tools_summary_body.contains("effective_namespaces"));
    assert!(tools_summary_body.contains("effective_tiers"));
    assert!(!tools_summary_body.contains("\"description\""));
    assert!(tools_summary_body.contains("reports"));

    let tools_full = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(24)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list/full"})),
        },
    )
    .unwrap();
    let tools_full_body = serde_json::to_string(&tools_full).unwrap();
    assert!(tools_full_body.contains("description"));
    assert!(tools_full_body.contains("namespace"));
    assert!(tools_full_body.contains("tier"));
    assert!(tools_full_body.contains("loaded_namespaces"));
    assert!(tools_full_body.contains("loaded_tiers"));
    assert!(tools_full_body.contains("full_tool_exposure"));

    let session_resource = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(241)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://session/http"})),
        },
    )
    .unwrap();
    let session_resource_body = serde_json::to_string(&session_resource).unwrap();
    assert!(session_resource_body.contains("resume_supported"));
    assert!(session_resource_body.contains("active_sessions"));
    assert!(session_resource_body.contains("deferred_loading_supported"));
    assert!(session_resource_body.contains("loaded_namespaces"));
    assert!(session_resource_body.contains("loaded_tiers"));
    assert!(session_resource_body.contains("full_tool_exposure"));
    assert!(session_resource_body.contains("preferred_namespaces"));
    assert!(session_resource_body.contains("preferred_tiers"));
    assert!(session_resource_body.contains("deferred_namespace_gate"));

    let surface_manifest = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://surface/manifest"})),
        },
    )
    .unwrap();
    let surface_manifest_body = serde_json::to_string(&surface_manifest).unwrap();
    assert!(surface_manifest_body.contains("schema_version"));
    assert!(surface_manifest_body.contains("tool_registry"));
    assert!(surface_manifest_body.contains("surfaces"));
    assert!(surface_manifest_body.contains("harness_modes"));
    assert!(surface_manifest_body.contains("harness_spec"));
    assert!(surface_manifest_body.contains("host_adapters"));
    assert!(surface_manifest_body.contains("agent_experience"));
    assert!(surface_manifest_body.contains("harness_artifacts"));
    assert!(surface_manifest_body.contains("languages"));

    let harness_modes = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/modes"})),
        },
    )
    .unwrap();
    let harness_modes_body = serde_json::to_string(&harness_modes).unwrap();
    assert!(harness_modes_body.contains("planner-builder"));
    assert!(harness_modes_body.contains("reviewer-gate"));
    assert!(harness_modes_body.contains("explicit-only"));
    assert!(harness_modes_body.contains("asymmetric-handoff"));

    let harness_spec = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_2)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/spec"})),
        },
    )
    .unwrap();
    let harness_spec_body = serde_json::to_string(&harness_spec).unwrap();
    assert!(harness_spec_body.contains("planner-builder-handoff"));
    assert!(harness_spec_body.contains("reviewer-signoff"));
    assert!(harness_spec_body.contains("batch-analysis-artifact"));
    assert!(harness_spec_body.contains("planner_builder_dispatch"));
    assert!(harness_spec_body.contains("expected_duration_x_1_5"));

    let host_adapters = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_21)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/host-adapters"})),
        },
    )
    .unwrap();
    let host_adapters_body = serde_json::to_string(&host_adapters).unwrap();
    assert!(host_adapters_body.contains("codelens-host-adapters-v1"));
    assert!(host_adapters_body.contains("memory_only_routing"));
    assert!(host_adapters_body.contains("claude-code"));
    assert!(host_adapters_body.contains("codex"));
    assert!(host_adapters_body.contains("cursor"));
    assert!(host_adapters_body.contains("windsurf"));
    assert!(host_adapters_body.contains("handoff_id"));
    assert!(host_adapters_body.contains("delegate_handoff_id"));
    assert!(host_adapters_body.contains("replay_rule"));
    assert!(host_adapters_body.contains("native_primitives"));
    assert!(host_adapters_body.contains("preferred_codelens_use"));
    assert!(host_adapters_body.contains("routing_defaults"));

    let harness_host = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_215)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/host", "host": "claude-code"})),
        },
    )
    .unwrap();
    let harness_host_value = serde_json::to_value(&harness_host).unwrap();
    let harness_host_text = harness_host_value["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    let harness_host_payload: serde_json::Value =
        serde_json::from_str(harness_host_text).expect("valid harness host JSON");
    assert_eq!(
        harness_host_payload["schema_version"],
        json!("codelens-harness-host-v1")
    );
    assert_eq!(harness_host_payload["requested_host"], json!("claude-code"));
    assert_eq!(
        harness_host_payload["selection_source"],
        json!("request_param")
    );
    assert_eq!(
        harness_host_payload["adapter_resource"],
        json!("codelens://host-adapters/claude-code")
    );
    assert_eq!(
        harness_host_payload["default_profile"],
        json!("planner-readonly")
    );
    assert_eq!(
        harness_host_payload["default_task_overlay"],
        json!("planning")
    );
    assert_eq!(
        harness_host_payload["detected_host"]["host_id"],
        json!("claude-code")
    );
    assert!(harness_host_payload["detected_host"]["bootstrap_sequence"]
        .as_array()
        .map(|items| items.iter().any(|value| value == "analyze_change_request"))
        .unwrap_or(false));

    let agent_experience = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_211)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://design/agent-experience"})),
        },
    )
    .unwrap();
    let agent_experience_body = serde_json::to_string(&agent_experience).unwrap();
    assert!(agent_experience_body.contains("codelens-agent-experience-v1"));
    assert!(agent_experience_body.contains("blocked_pending_trademark_clearance"));
    assert!(agent_experience_body.contains("delegate_to_codex_builder"));
    assert!(agent_experience_body.contains("under_60_seconds_to_first_compressed_answer"));

    let codex_host_adapter = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_22)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://host-adapters/codex"})),
        },
    )
    .unwrap();
    let codex_host_adapter_body = serde_json::to_string(&codex_host_adapter).unwrap();
    assert!(codex_host_adapter_body.contains("builder-minimal"));
    assert!(codex_host_adapter_body.contains("~/.codex/config.toml"));
    assert!(codex_host_adapter_body.contains("AGENTS.md"));
    assert!(codex_host_adapter_body.contains("delegate_to_codex_builder"));
    assert!(codex_host_adapter_body.contains("handoff_id"));
    assert!(codex_host_adapter_body.contains("overlay_previews"));
    assert!(codex_host_adapter_body.contains("primary_bootstrap_sequence"));
    assert!(codex_host_adapter_body.contains("default_task_overlay"));
    assert!(codex_host_adapter_body.contains("editing"));
    assert!(codex_host_adapter_body.contains("## Compiled Routing Overlays"));

    let cursor_host_adapter = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://host-adapters/cursor"})),
        },
    )
    .unwrap();
    let cursor_host_adapter_body = serde_json::to_string(&cursor_host_adapter).unwrap();
    assert!(cursor_host_adapter_body.contains(".cursor/rules/codelens-routing.mdc"));
    assert!(cursor_host_adapter_body.contains("background agents"));
    assert!(cursor_host_adapter_body.contains("handoff_id"));
    assert!(cursor_host_adapter_body.contains("overlay_previews"));
    assert!(cursor_host_adapter_body.contains("primary_bootstrap_sequence"));
    assert!(cursor_host_adapter_body.contains("## Compiled Routing Overlays"));

    let windsurf_host_adapter = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_24)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://host-adapters/windsurf"})),
        },
    )
    .unwrap();
    let windsurf_host_adapter_body = serde_json::to_string(&windsurf_host_adapter).unwrap();
    assert!(windsurf_host_adapter_body.contains("~/.codeium/windsurf/mcp_config.json"));
    assert!(windsurf_host_adapter_body.contains("100-tool cap"));

    let handoff_schema = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_3)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://schemas/handoff-artifact/v1"})),
        },
    )
    .unwrap();
    let handoff_schema_body = serde_json::to_string(&handoff_schema).unwrap();
    assert!(handoff_schema_body.contains("codelens-handoff-artifact-v1"));
    assert!(handoff_schema_body.contains("planner_brief"));
    assert!(handoff_schema_body.contains("builder_result"));
    assert!(handoff_schema_body.contains("reviewer_verdict"));

    assert!(session_resource_body.contains("deferred_tier_gate"));
    assert!(session_resource_body.contains("mutation_preflight_required"));
    assert!(session_resource_body.contains("preflight_ttl_seconds"));
    assert!(session_resource_body.contains("rename_requires_symbol_preflight"));
    assert!(session_resource_body.contains("requires_namespace_listing_before_tool_call"));
    assert!(session_resource_body.contains("requires_tier_listing_before_tool_call"));
    assert!(session_resource_body.contains("client_profile"));
    assert!(session_resource_body.contains("default_tools_list_contract_mode"));

    let profile_summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(25)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://profile/reviewer-graph/guide"})),
        },
    )
    .unwrap();
    let profile_summary_body = serde_json::to_string(&profile_summary).unwrap();
    assert!(profile_summary_body.contains("preferred_namespaces"));
    assert!(profile_summary_body.contains("preferred_tiers"));
    assert!(tools_summary_body.contains("preferred_namespaces"));
    assert!(tools_summary_body.contains("preferred_tiers"));
}

#[test]
fn ci_audit_analysis_summary_resource_matches_machine_schema() {
    let project = project_root();
    fs::write(
        project.as_path().join("ci_audit.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));
    let payload = call_tool(&state, "impact_report", json!({"path": "ci_audit.py"}));
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(26)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&summary).unwrap();
    assert!(body.contains("codelens-ci-audit-v1"));
    assert!(body.contains("machine_summary"));
    assert!(body.contains("evidence_handles"));
    assert!(body.contains("summary_resource"));
    assert!(body.contains("section_handles"));
    assert!(body.contains("blocker_count"));
    assert!(body.contains("verifier_check_count"));
    assert!(body.contains("ready_check_count"));
    assert!(body.contains("blocked_check_count"));
    assert!(body.contains("readiness"));
    assert!(body.contains("verifier_checks"));
    assert!(body.contains("quality_focus"));
    assert!(body.contains("recommended_checks"));
    assert!(body.contains("performance_watchpoints"));
}

#[test]
fn tool_metrics_expose_kpis_and_chain_detection() {
    let project = project_root();
    fs::write(
        project.as_path().join("chain.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "chain.py", "include_body": false}),
    );
    let _ = call_tool(
        &state,
        "find_referencing_symbols",
        json!({"file_path": "chain.py", "symbol_name": "alpha", "max_results": 10}),
    );
    let _ = call_tool(&state, "read_file", json!({"relative_path": "chain.py"}));
    let report = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "improve alpha flow in chain.py"}),
    );
    let analysis_id = report["data"]["analysis_id"].as_str().unwrap();
    let _ = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(metrics["data"]["per_tool"].is_array());
    assert!(metrics["data"]["per_surface"].is_array());
    assert!(metrics["data"]["derived_kpis"]["composite_ratio"].is_number());
    assert!(metrics["data"]["session"]["quality_contract_emitted_count"].is_number());
    assert!(metrics["data"]["session"]["recommended_checks_emitted_count"].is_number());
    assert!(metrics["data"]["session"]["quality_focus_reuse_count"].is_number());
    assert!(metrics["data"]["session"]["verifier_contract_emitted_count"].is_number());
    assert!(metrics["data"]["session"]["blocker_emit_count"].is_number());
    assert!(metrics["data"]["session"]["verifier_followthrough_count"].is_number());
    assert!(metrics["data"]["session"]["composite_guidance_missed_count"].is_number());
    assert!(metrics["data"]["session"]["composite_guidance_missed_by_origin"].is_object());
    assert!(metrics["data"]["session"]["mutation_preflight_checked_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_without_preflight_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_preflight_gate_denied_count"].is_number());
    assert!(metrics["data"]["session"]["stale_preflight_reject_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_with_caution_count"].is_number());
    assert!(metrics["data"]["session"]["rename_without_symbol_preflight_count"].is_number());
    assert!(metrics["data"]["session"]["deferred_namespace_expansion_count"].is_number());
    assert!(metrics["data"]["session"]["deferred_hidden_tool_call_denied_count"].is_number());
    assert!(metrics["data"]["session"]["profile_switch_count"].is_number());
    assert!(metrics["data"]["session"]["preset_switch_count"].is_number());
    assert!(metrics["data"]["derived_kpis"]["quality_contract_present_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["recommended_check_followthrough_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["quality_focus_reuse_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["performance_watchpoint_emit_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["verifier_contract_present_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["blocker_emit_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["verifier_followthrough_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["mutation_preflight_gate_deny_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["deferred_hidden_tool_call_deny_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["composite_guidance_miss_rate"].is_number());
    assert!(
        metrics["data"]["session"]["repeated_low_level_chain_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(metrics["data"]["session"]["watcher_lock_contention_batches"].is_number());
    assert!(metrics["data"]["session"]["watcher_index_failures"].is_number());
    assert!(metrics["data"]["session"]["watcher_index_failures_total"].is_number());
    assert!(metrics["data"]["session"]["watcher_stale_index_failures"].is_number());
    assert!(metrics["data"]["session"]["watcher_persistent_index_failures"].is_number());
    assert!(metrics["data"]["session"]["watcher_pruned_missing_failures"].is_number());
    assert!(metrics["data"]["derived_kpis"]["watcher_lock_contention_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["watcher_recent_failure_share"].is_number());
}

#[test]
fn token_efficiency_resource_includes_watcher_metrics() {
    let project = project_root();
    let state = make_state(&project);

    let stats = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2501)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://stats/token-efficiency"})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&stats).unwrap();
    assert!(body.contains("watcher_lock_contention_batches"));
    assert!(body.contains("watcher_index_failures"));
    assert!(body.contains("watcher_index_failures_total"));
    assert!(body.contains("watcher_stale_index_failures"));
    assert!(body.contains("watcher_persistent_index_failures"));
    assert!(body.contains("watcher_pruned_missing_failures"));
    assert!(body.contains("watcher_lock_contention_rate"));
    assert!(body.contains("watcher_recent_failure_share"));
    assert!(body.contains("deferred_namespace_expansion_count"));
    assert!(body.contains("deferred_hidden_tool_call_denied_count"));
    assert!(body.contains("deferred_hidden_tool_call_deny_rate"));
    assert!(body.contains("mutation_preflight_checked_count"));
}

#[test]
fn schema_tools_return_structured_content_payload() {
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3101)),
            method: "tools/call".to_owned(),
            params: Some(
                json!({ "name": "get_symbols_overview", "arguments": { "path": "sample.py" } }),
            ),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert!(value["result"]["structuredContent"].is_object());
    assert!(value["result"]["structuredContent"]["symbols"].is_array());

    let text_payload = extract_tool_text(&response);
    let wrapped = parse_tool_payload(&text_payload);
    assert!(wrapped["data"]["symbols"].is_array());
}

#[test]
fn output_schema_workflow_tools_return_structured_content() {
    let project = project_root();
    fs::write(
        project.as_path().join("flow.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3102)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "analyze_change_request",
                "arguments": { "task": "improve alpha in flow.py" }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert!(value["result"]["structuredContent"].is_object());
    assert!(value["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(value["result"]["structuredContent"]["summary"].is_string());
    assert!(value["result"]["structuredContent"]["readiness"].is_object());
    assert!(value["result"]["structuredContent"]["verifier_checks"].is_array());

    let bootstrap_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31026)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "prepare_harness_session",
                "arguments": { "profile": "builder-minimal" }
            })),
        },
    )
    .unwrap();
    let bootstrap_value = serde_json::to_value(&bootstrap_response).unwrap();
    assert!(bootstrap_value["result"]["structuredContent"].is_object());
    assert_eq!(
        bootstrap_value["result"]["structuredContent"]["active_surface"],
        json!("builder-minimal")
    );
    assert!(bootstrap_value["result"]["structuredContent"]["health_summary"].is_object());
    assert!(bootstrap_value["result"]["structuredContent"]["capabilities"].is_object());
    assert!(
        bootstrap_value["result"]["structuredContent"]["capabilities"]["diagnostics_guidance"]
            .is_object()
    );
    assert!(
        bootstrap_value["result"]["structuredContent"]["visible_tools"]["tool_names"].is_array()
    );
    assert!(
        bootstrap_value["result"]["structuredContent"]["visible_tools"]["preferred_executors"]
            .is_object()
    );
    assert!(bootstrap_value["result"]["structuredContent"]["routing"].is_object());
    assert!(bootstrap_value["result"]["structuredContent"]["routing"]
        ["preferred_entrypoints_with_executors"]
        .is_array());
    assert!(bootstrap_value["result"]["structuredContent"]["routing"]
        ["recommended_entrypoint_preferred_executor"]
        .is_string());
    assert!(bootstrap_value["result"]["structuredContent"]["warnings"].is_array());
    let bootstrap_text = parse_tool_payload(&extract_tool_text(&bootstrap_response));
    assert_eq!(
        bootstrap_text["data"]["active_surface"],
        json!("builder-minimal")
    );
    assert!(bootstrap_text["data"]["capabilities"]["indexed_files"].is_u64());
    assert!(bootstrap_text["data"]["capabilities"]["stale_files"].is_u64());
    assert!(
        bootstrap_text["data"]["visible_tools"]["tool_names"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or_default()
            <= 3
    );
    assert!(bootstrap_text["data"]["routing"]["recommended_entrypoint"].is_string());
}

#[test]
fn workflow_alias_tools_return_structured_content_and_delegate() {
    let project = project_root();
    fs::write(
        project.as_path().join("workflow_alias.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31025)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "explore_codebase",
                "arguments": { "query": "alpha in workflow_alias.py", "max_tokens": 1200 }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        value["result"]["structuredContent"]["workflow"],
        json!("explore_codebase")
    );
    assert_eq!(
        value["result"]["structuredContent"]["delegated_tool"],
        json!("get_ranked_context")
    );
    assert!(value["result"]["structuredContent"]
        .get("deprecated")
        .is_none());
    assert!(value["result"]["structuredContent"]["symbols"].is_array());
}

#[test]
fn verifier_tools_return_structured_content_payload() {
    let project = project_root();
    fs::write(
        project.as_path().join("verify.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let readiness_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31021)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "verify_change_readiness",
                "arguments": { "task": "update alpha in verify.py", "changed_files": ["verify.py"] }
            })),
        },
    )
    .unwrap();
    let readiness_value = serde_json::to_value(&readiness_response).unwrap();
    assert!(readiness_value["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(readiness_value["result"]["structuredContent"]["readiness"].is_object());
    assert!(readiness_value["result"]["structuredContent"]["verifier_checks"].is_array());
    let readiness_text = parse_tool_payload(&extract_tool_text(&readiness_response));
    assert!(readiness_text["data"]["analysis_id"].is_string());
    assert!(readiness_text["data"]["summary"].is_string());
    assert!(readiness_text["data"]["readiness"].is_object());
    assert!(readiness_text["data"]["summary_resource"]["uri"]
        .as_str()
        .map(|uri| uri.contains("codelens://analysis/"))
        .unwrap_or(false));
    assert!(readiness_text["data"]["section_handles"]
        .as_array()
        .map(|items| !items.is_empty() && items.len() <= 3)
        .unwrap_or(false));
    assert!(readiness_text["suggested_next_calls"].is_array());
    assert!(readiness_text["suggested_next_calls"]
        .as_array()
        .map(|items| {
            items.iter().any(|entry| {
                entry["tool"].as_str() == Some("get_analysis_section")
                    && entry["arguments"]["analysis_id"].is_string()
            })
        })
        .unwrap_or(false));
    assert_eq!(readiness_text["routing_hint"], json!("async"));
    assert!(readiness_text["data"].get("verifier_checks").is_none());
    assert!(readiness_text["data"].get("blockers").is_none());
    assert!(readiness_text["data"].get("available_sections").is_none());

    let unresolved_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(31022)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "unresolved_reference_check",
                "arguments": { "file_path": "verify.py", "symbol": "missing_symbol" }
            })),
        },
    )
    .unwrap();
    let unresolved_value = serde_json::to_value(&unresolved_response).unwrap();
    assert!(unresolved_value["result"]["structuredContent"]["blockers"].is_array());
    assert_eq!(
        unresolved_value["result"]["structuredContent"]["readiness"]["reference_safety"],
        json!("blocked")
    );
}

#[test]
fn oversized_schema_tool_truncates_structured_content_too() {
    let project = project_root();
    let source = (0..40)
        .map(|index| format!("def alpha_{index}():\n    return {index}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(project.as_path().join("oversized.py"), source).unwrap();
    let state = make_state(&project);
    state.set_token_budget(1);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3103)),
            method: "tools/call".to_owned(),
            params: Some(
                json!({ "name": "get_symbols_overview", "arguments": { "path": "oversized.py" } }),
            ),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    let text_payload = parse_tool_payload(&extract_tool_text(&response));
    assert_eq!(text_payload["truncated"], json!(true));
    assert_eq!(
        value["result"]["structuredContent"]["truncated"],
        json!(true)
    );
    assert!(
        text_payload["data"]["symbols"]
            .as_array()
            .map(|symbols| symbols.len())
            .unwrap_or_default()
            <= 3
    );
    assert!(
        value["result"]["structuredContent"]["symbols"]
            .as_array()
            .map(|symbols| symbols.len())
            .unwrap_or_default()
            <= 3
    );
}

#[test]
fn oversized_analysis_handle_keeps_structured_content_schema_shape() {
    let project = project_root();
    fs::write(project.as_path().join("preflight.py"), "print('hello')\n").unwrap();
    let state = make_state(&project);
    state.set_token_budget(1);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3104)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "verify_change_readiness",
                "arguments": {
                    "task": "update preflight.py",
                    "changed_files": ["preflight.py"]
                }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        parse_tool_payload(&extract_tool_text(&response))["truncated"],
        json!(true)
    );
    assert_eq!(value["result"]["structuredContent"].get("truncated"), None);
    assert!(value["result"]["structuredContent"]["analysis_id"]
        .as_str()
        .is_some());
    assert!(
        value["result"]["structuredContent"]["readiness"]["mutation_ready"]
            .as_str()
            .is_some()
    );
}

#[test]
fn impact_analysis_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("get_impact_analysis")
        .and_then(|tool| tool.output_schema.as_ref())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(properties.contains_key("symbols"));
    assert!(properties.contains_key("direct_importers"));
}

#[test]
fn onboard_project_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("onboard_project")
        .and_then(|tool| tool.output_schema.as_ref())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(properties.contains_key("project_root"));
    assert!(properties.contains_key("suggested_next_tools"));
}

#[test]
fn get_capabilities_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("get_capabilities")
        .and_then(|tool| tool.output_schema.as_ref())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(properties.contains_key("diagnostics_guidance"));
    assert!(properties.contains_key("semantic_search_guidance"));
    assert!(properties.contains_key("daemon_binary_drift"));
    assert!(properties.contains_key("health_summary"));
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
fn analysis_handle_schema_exposes_resource_handles() {
    let schema = crate::tool_defs::tool_definition("impact_report")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("impact_report schema");

    let properties = schema["properties"].as_object().expect("schema properties");
    assert!(properties.contains_key("summary_resource"));
    assert!(properties.contains_key("section_handles"));
}

#[test]
fn analysis_job_schema_exposes_resource_handles() {
    let schema = crate::tool_defs::tool_definition("get_analysis_job")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("get_analysis_job schema");

    let properties = schema["properties"].as_object().expect("schema properties");
    assert!(properties.contains_key("summary_resource"));
    assert!(properties.contains_key("section_handles"));
}

#[test]
fn analysis_list_schemas_expose_machine_summary_fields() {
    let jobs_schema = crate::tool_defs::tool_definition("list_analysis_jobs")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("list_analysis_jobs schema");
    let job_properties = jobs_schema["properties"]
        .as_object()
        .expect("jobs schema properties");
    assert!(job_properties.contains_key("jobs"));
    assert!(job_properties.contains_key("active_count"));
    assert!(job_properties.contains_key("status_counts"));

    let artifacts_schema = crate::tool_defs::tool_definition("list_analysis_artifacts")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("list_analysis_artifacts schema");
    let artifact_properties = artifacts_schema["properties"]
        .as_object()
        .expect("artifacts schema properties");
    assert!(artifact_properties.contains_key("artifacts"));
    assert!(artifact_properties.contains_key("tool_counts"));
    assert!(artifact_properties.contains_key("latest_created_at_ms"));
}

#[test]
fn workflow_surfaces_prefer_canonical_bootstrap_entrypoints() {
    use crate::protocol::ToolTier;
    use crate::tool_defs::{
        preferred_bootstrap_tools, preferred_tiers, ToolPreset, ToolProfile, ToolSurface,
    };

    let builder_tiers = preferred_tiers(ToolSurface::Profile(ToolProfile::BuilderMinimal));
    assert!(matches!(builder_tiers.first(), Some(ToolTier::Workflow)));

    let balanced_bootstrap =
        preferred_bootstrap_tools(ToolSurface::Preset(ToolPreset::Balanced)).unwrap_or(&[]);
    assert!(balanced_bootstrap.contains(&"explore_codebase"));
    assert!(balanced_bootstrap.contains(&"review_architecture"));
    assert!(balanced_bootstrap.contains(&"review_changes"));
    assert!(!balanced_bootstrap.contains(&"analyze_change_impact"));
}

#[test]
fn visible_tools_order_workflow_surfaces_bootstrap_first() {
    use crate::tool_defs::{visible_tools, ToolProfile, ToolSurface};

    let builder_tools = visible_tools(ToolSurface::Profile(ToolProfile::BuilderMinimal))
        .into_iter()
        .map(|tool| tool.name)
        .take(4)
        .collect::<Vec<_>>();
    assert_eq!(
        builder_tools,
        vec![
            "explore_codebase",
            "trace_request_path",
            "plan_safe_refactor",
            "prepare_harness_session",
        ]
    );

    let reviewer_tools = visible_tools(ToolSurface::Profile(ToolProfile::ReviewerGraph))
        .into_iter()
        .map(|tool| tool.name)
        .take(4)
        .collect::<Vec<_>>();
    assert_eq!(
        reviewer_tools,
        vec![
            "review_architecture",
            "review_changes",
            "cleanup_duplicate_logic",
            "prepare_harness_session",
        ]
    );
}

#[test]
fn deprecated_aliases_are_hidden_from_non_full_visible_surfaces() {
    use crate::tool_defs::{visible_tools, ToolPreset, ToolProfile, ToolSurface};

    let reviewer_tools = visible_tools(ToolSurface::Profile(ToolProfile::ReviewerGraph))
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    assert!(!reviewer_tools.contains(&"analyze_change_impact"));
    assert!(!reviewer_tools.contains(&"audit_security_context"));
    assert!(!reviewer_tools.contains(&"assess_change_readiness"));
    assert!(reviewer_tools.contains(&"review_changes"));
    assert!(reviewer_tools.contains(&"cleanup_duplicate_logic"));

    let full_tools = visible_tools(ToolSurface::Preset(ToolPreset::Full))
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    assert!(full_tools.contains(&"analyze_change_impact"));
    assert!(full_tools.contains(&"audit_security_context"));
    assert!(full_tools.contains(&"assess_change_readiness"));
}

#[test]
fn deprecated_alias_direct_calls_still_work_when_replacement_is_in_surface() {
    let cases = [
        (
            "planner-readonly",
            "analyze_change_impact",
            json!({"path": "compat.py"}),
            "impact_report",
        ),
        (
            "reviewer-graph",
            "audit_security_context",
            json!({"changed_files": ["compat.py"]}),
            "semantic_code_review",
        ),
        (
            "refactor-full",
            "assess_change_readiness",
            json!({"task": "check compat.py", "changed_files": ["compat.py"]}),
            "verify_change_readiness",
        ),
    ];

    for (profile, alias_tool, arguments, replacement_tool) in cases {
        let project = project_root();
        fs::write(
            project.as_path().join("compat.py"),
            "def alpha():\n    return 1\n",
        )
        .unwrap();
        let state = make_state(&project);
        let _ = call_tool(&state, "set_profile", json!({"profile": profile}));

        let payload = call_tool(&state, alias_tool, arguments);
        assert_eq!(payload["success"], json!(true), "{profile} / {alias_tool}");
        assert_eq!(payload["data"]["workflow"], json!(alias_tool));
        assert_eq!(payload["data"]["deprecated"], json!(true));
        assert_eq!(payload["data"]["replacement_tool"], json!(replacement_tool));
        assert_eq!(payload["data"]["removal_target"], json!("v2.0"));
    }
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
    assert!(payload["data"]["routing"]["preferred_entrypoints"]
        .as_array()
        .map(|items| items.iter().any(|value| value == "trace_request_path"))
        .unwrap_or(false));
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
    assert!(payload["data"]["overlay"]["avoid_tools"]
        .as_array()
        .map(|items| items.iter().any(|value| value == "rename_symbol"))
        .unwrap_or(false));
    assert!(payload["data"]["overlay"]["routing_notes"]
        .as_array()
        .map(|items| items.iter().any(|value| {
            value
                .as_str()
                .map(|text| text.contains("Review overlay"))
                .unwrap_or(false)
        }))
        .unwrap_or(false));
}

#[test]
fn project_architecture_resource_recommends_canonical_workflows() {
    let project = project_root();
    fs::write(
        project.as_path().join("architecture.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(252)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://project/architecture"})),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    let text = value["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    let payload: serde_json::Value = serde_json::from_str(text).expect("valid architecture JSON");
    let notes = payload["notes"].as_array().expect("notes array");
    assert!(notes
        .iter()
        .filter_map(|value| value.as_str())
        .any(|note| note.contains("review_changes")));
    assert!(!notes
        .iter()
        .filter_map(|value| value.as_str())
        .any(|note| note.contains("analyze_change_impact")));
}

#[test]
fn suggest_next_prefers_canonical_workflows() {
    let explore = crate::tools::suggest_next("explore_codebase").expect("explore suggestions");
    assert!(explore.iter().any(|item| item == "review_changes"));
    assert!(!explore.iter().any(|item| item == "analyze_change_impact"));

    let alias = crate::tools::suggest_next("analyze_change_impact").expect("alias suggestions");
    assert!(alias.iter().any(|item| item == "review_changes"));
    assert!(!alias.iter().any(|item| item == "audit_security_context"));
}

#[test]
fn low_level_chain_emits_composite_guidance_and_tracks_followthrough() {
    let project = project_root();
    fs::write(
        project.as_path().join("guided.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "guided.py", "include_body": false}),
    );
    let _ = call_tool(
        &state,
        "find_referencing_symbols",
        json!({"file_path": "guided.py", "symbol_name": "alpha", "max_results": 10}),
    );
    let response = call_tool(&state, "read_file", json!({"relative_path": "guided.py"}));
    let suggested = response["suggested_next_tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let suggested_names = suggested
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(
        suggested_names.contains(&"explore_codebase")
            || suggested_names.contains(&"plan_safe_refactor")
            || suggested_names.contains(&"review_architecture")
            || suggested_names.contains(&"find_minimal_context_for_change")
            || suggested_names.contains(&"analyze_change_request"),
        "expected composite guidance, got {:?}",
        suggested_names
    );
    let budget_hint = response["budget_hint"].as_str().unwrap_or_default();
    assert!(budget_hint.contains("Repeated low-level chain detected"));

    let _ = call_tool(
        &state,
        "find_minimal_context_for_change",
        json!({"task": "update alpha safely"}),
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["composite_guidance_emitted_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["composite_guidance_followed_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert_eq!(
        metrics["data"]["session"]["composite_guidance_missed_count"],
        json!(0)
    );
}

#[test]
fn workflow_guidance_miss_tracks_origin_without_counting_profile_switch() {
    let project = project_root();
    fs::write(
        project.as_path().join("guided_miss.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "guided_miss.py", "include_body": false}),
    );
    let _ = call_tool(
        &state,
        "find_referencing_symbols",
        json!({"file_path": "guided_miss.py", "symbol_name": "alpha", "max_results": 10}),
    );
    let _ = call_tool(
        &state,
        "read_file",
        json!({"relative_path": "guided_miss.py"}),
    );
    let _ = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );

    let metrics_after_switch = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics_after_switch["data"]["session"]["composite_guidance_missed_count"],
        json!(0)
    );
    assert!(
        metrics_after_switch["data"]["session"]["profile_switch_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );

    let _ = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "guided_miss.py", "include_body": false}),
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["composite_guidance_missed_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["derived_kpis"]["composite_guidance_miss_rate"]
            .as_f64()
            .unwrap_or_default()
            > 0.0
    );
    let missed_by_origin = metrics["data"]["session"]["composite_guidance_missed_by_origin"]
        .as_object()
        .expect("missed-by-origin should be an object");
    assert!(
        missed_by_origin
            .get("read_file")
            .and_then(|value| value.as_u64())
            .unwrap_or_default()
            >= 1,
        "expected read_file miss origin, got {missed_by_origin:?}"
    );
}

#[test]
fn suggested_next_calls_forward_task_and_analysis_id() {
    let project = project_root();
    fs::write(
        project.as_path().join("next_calls.py"),
        "def widget():\n    return 7\n",
    )
    .unwrap();
    let state = make_state(&project);

    let analyze = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "refactor widget safely", "changed_files": ["next_calls.py"]}),
    );
    let analyze_next_calls = analyze["suggested_next_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !analyze_next_calls.is_empty(),
        "analyze_change_request should populate suggested_next_calls: {analyze}"
    );
    let verify_entry = analyze_next_calls
        .iter()
        .find(|call| call.get("tool").and_then(|v| v.as_str()) == Some("verify_change_readiness"))
        .expect("verify_change_readiness should be forwarded with args");
    assert_eq!(
        verify_entry["arguments"]["task"].as_str(),
        Some("refactor widget safely")
    );
    assert!(
        verify_entry["arguments"]["changed_files"].is_array(),
        "changed_files should be forwarded as array: {verify_entry}"
    );

    let verify = call_tool(
        &state,
        "verify_change_readiness",
        json!({"task": "refactor widget safely", "changed_files": ["next_calls.py"]}),
    );
    let analysis_id = verify["data"]["analysis_id"]
        .as_str()
        .expect("verify_change_readiness should return analysis_id");
    let verify_next_calls = verify["suggested_next_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(expand) = verify_next_calls
        .iter()
        .find(|call| call.get("tool").and_then(|v| v.as_str()) == Some("get_analysis_section"))
    {
        assert_eq!(
            expand["arguments"]["analysis_id"].as_str(),
            Some(analysis_id),
            "get_analysis_section should forward the fresh analysis_id: {expand}"
        );
    }
}

#[test]
fn safe_rename_report_emits_codex_builder_delegate_scaffold() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_delegate.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let payload = call_tool(
        &state,
        "safe_rename_report",
        json!({
            "file_path": "rename_delegate.py",
            "symbol": "old_name",
            "new_name": "new_name"
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let suggested = payload["suggested_next_tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        suggested
            .iter()
            .any(|value| value == "delegate_to_codex_builder"),
        "expected delegate_to_codex_builder suggestion, got {payload}"
    );

    let delegate_call = payload["suggested_next_calls"]
        .as_array()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.get("tool").and_then(|value| value.as_str())
                    == Some("delegate_to_codex_builder")
            })
        })
        .cloned()
        .expect("delegate_to_codex_builder should include a scaffold payload");

    assert_eq!(
        delegate_call["arguments"]["preferred_executor"],
        json!("codex-builder")
    );
    assert_eq!(
        delegate_call["arguments"]["trigger"],
        json!("preferred_executor_boundary")
    );
    assert_eq!(
        delegate_call["arguments"]["delegate_tool"],
        json!("rename_symbol")
    );
    let handoff_id = delegate_call["arguments"]["handoff_id"]
        .as_str()
        .expect("delegate scaffold should include handoff_id");
    assert_eq!(
        delegate_call["arguments"]["delegate_arguments"]["file_path"],
        json!("rename_delegate.py")
    );
    assert_eq!(
        delegate_call["arguments"]["delegate_arguments"]["symbol_name"],
        json!("old_name")
    );
    assert_eq!(
        delegate_call["arguments"]["delegate_arguments"]["new_name"],
        json!("new_name")
    );
    assert_eq!(
        delegate_call["arguments"]["delegate_arguments"]["handoff_id"],
        json!(handoff_id)
    );
    assert_eq!(
        delegate_call["arguments"]["carry_forward"]["handoff_id"],
        json!(handoff_id)
    );
}

#[test]
fn delegate_handoff_id_persists_across_planner_and_builder_sessions_in_telemetry() {
    struct TelemetryEnvGuard {
        prev_sym_enabled: Option<String>,
        prev_enabled: Option<String>,
        prev_sym_path: Option<String>,
        prev_path: Option<String>,
    }

    impl TelemetryEnvGuard {
        fn install(path: &std::path::Path) -> Self {
            let guard = Self {
                prev_sym_enabled: std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok(),
                prev_enabled: std::env::var("CODELENS_TELEMETRY_ENABLED").ok(),
                prev_sym_path: std::env::var("SYMBIOTE_TELEMETRY_PATH").ok(),
                prev_path: std::env::var("CODELENS_TELEMETRY_PATH").ok(),
            };
            unsafe {
                std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED");
                std::env::set_var("CODELENS_TELEMETRY_ENABLED", "1");
                std::env::remove_var("SYMBIOTE_TELEMETRY_PATH");
                std::env::set_var("CODELENS_TELEMETRY_PATH", path);
            }
            guard
        }
    }

    impl Drop for TelemetryEnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prev_sym_enabled {
                    Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", value),
                    None => std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED"),
                }
                match &self.prev_enabled {
                    Some(value) => std::env::set_var("CODELENS_TELEMETRY_ENABLED", value),
                    None => std::env::remove_var("CODELENS_TELEMETRY_ENABLED"),
                }
                match &self.prev_sym_path {
                    Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", value),
                    None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
                }
                match &self.prev_path {
                    Some(value) => std::env::set_var("CODELENS_TELEMETRY_PATH", value),
                    None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
                }
            }
        }
    }

    let _env_lock = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let telemetry_path = std::env::temp_dir().join(format!(
        "codelens-delegate-telemetry-{}-{:?}.jsonl",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id()
    ));
    let _env_guard = TelemetryEnvGuard::install(&telemetry_path);

    let project = project_root();
    fs::write(
        project.as_path().join("rename_delegate_telemetry.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "planner-session",
    );
    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "builder-session",
    );

    let planner_payload = call_tool_with_session(
        &state,
        "safe_rename_report",
        json!({
            "file_path": "rename_delegate_telemetry.py",
            "symbol": "old_name",
            "new_name": "new_name"
        }),
        "planner-session",
    );
    assert_eq!(planner_payload["success"], json!(true));

    let delegate_call = planner_payload["suggested_next_calls"]
        .as_array()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.get("tool").and_then(|value| value.as_str())
                    == Some("delegate_to_codex_builder")
            })
        })
        .cloned()
        .expect("delegate_to_codex_builder should include a scaffold payload");

    let handoff_id = delegate_call["arguments"]["handoff_id"]
        .as_str()
        .expect("delegate scaffold should include handoff_id")
        .to_owned();
    let builder_arguments = delegate_call["arguments"]["delegate_arguments"].clone();

    let builder_payload = call_tool_with_session(
        &state,
        "rename_symbol",
        builder_arguments,
        "builder-session",
    );
    assert!(
        builder_payload["data"].is_object()
            || builder_payload.get("suggested_next_tools").is_some(),
        "builder response should remain structured for telemetry correlation: {builder_payload}"
    );

    let contents = std::fs::read_to_string(&telemetry_path).expect("read telemetry jsonl");
    let events: Vec<serde_json::Value> = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse telemetry jsonl line"))
        .collect();

    let planner_event = events
        .iter()
        .find(|event| {
            event["session_id"] == json!("planner-session")
                && event["tool"] == json!("safe_rename_report")
                && event["delegate_handoff_id"].as_str() == Some(handoff_id.as_str())
        })
        .cloned()
        .expect("planner event should persist delegate handoff metadata");
    assert_eq!(
        planner_event["delegate_hint_trigger"],
        json!("preferred_executor_boundary")
    );

    let builder_event = events
        .iter()
        .find(|event| {
            event["session_id"] == json!("builder-session")
                && event["tool"] == json!("rename_symbol")
                && event["handoff_id"].as_str() == Some(handoff_id.as_str())
        })
        .cloned()
        .expect("builder event should persist the replayed handoff_id");
    assert_ne!(planner_event["session_id"], builder_event["session_id"]);

    let _ = std::fs::remove_dir_all(telemetry_path.parent().unwrap());
}

#[test]
fn repeated_builder_tool_emits_codex_builder_delegate_scaffold() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_loop.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "safe_rename_report",
        json!({
            "file_path": "rename_loop.py",
            "symbol": "old_name",
            "new_name": "new_name"
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    assert_ne!(
        preflight["data"]["readiness"]["mutation_ready"],
        json!("blocked")
    );

    let args = json!({
        "file_path": "rename_loop.py",
        "symbol_name": "old_name",
        "new_name": "new_name",
        "dry_run": true
    });
    let _ = call_tool(&state, "rename_symbol", args.clone());
    let _ = call_tool(&state, "rename_symbol", args.clone());
    let payload = call_tool(&state, "rename_symbol", args);
    assert_eq!(payload["success"], json!(false));

    let suggested = payload["suggested_next_tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        suggested
            .iter()
            .any(|value| value == "delegate_to_codex_builder"),
        "expected delegate_to_codex_builder suggestion on repeated builder loop, got {payload}"
    );

    let delegate_call = payload["suggested_next_calls"]
        .as_array()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.get("tool").and_then(|value| value.as_str())
                    == Some("delegate_to_codex_builder")
            })
        })
        .cloned()
        .expect("delegate_to_codex_builder should include a scaffold payload");

    assert_eq!(
        delegate_call["arguments"]["trigger"],
        json!("builder_doom_loop")
    );
    assert!(
        delegate_call["arguments"]["briefing"]["why_delegate"]
            .as_str()
            .map(|value| value.contains("repeated"))
            .unwrap_or(false),
        "doom-loop delegate scaffold should explain the repeated builder retry: {delegate_call}"
    );
    assert_eq!(
        delegate_call["arguments"]["delegate_tool"],
        json!("rename_symbol")
    );
    let handoff_id = delegate_call["arguments"]["handoff_id"]
        .as_str()
        .expect("delegate scaffold should include handoff_id");
    assert_eq!(
        delegate_call["arguments"]["delegate_arguments"]["dry_run"],
        json!(true)
    );
    assert_eq!(
        delegate_call["arguments"]["delegate_arguments"]["handoff_id"],
        json!(handoff_id)
    );
    assert_eq!(
        delegate_call["arguments"]["carry_forward"]["handoff_id"],
        json!(handoff_id)
    );
}

#[test]
fn analysis_artifacts_evict_oldest_disk_payloads() {
    let project = project_root();
    fs::write(
        project.as_path().join("evict.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let mut first_analysis_id = None;

    for idx in 0..70 {
        let payload = call_tool(
            &state,
            "analyze_change_request",
            json!({"task": format!("update alpha flow {idx}")}),
        );
        let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
        if first_analysis_id.is_none() {
            first_analysis_id = Some(analysis_id);
        }
    }

    let first_analysis_id = first_analysis_id.expect("first analysis id");
    assert!(state.get_analysis(&first_analysis_id).is_none());
    assert!(!state.analysis_dir().join(&first_analysis_id).exists());
}

#[test]
fn foreign_project_scoped_analysis_is_ignored_for_reuse() {
    let project = project_root();
    fs::write(
        project.as_path().join("foreign.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let analysis_id = "analysis-foreign";
    let artifact_dir = state.analysis_dir().join(analysis_id);
    fs::create_dir_all(&artifact_dir).unwrap();
    let cache_key = json!({
        "tool": "analyze_change_request",
        "fields": {
            "task": "update alpha safely"
        }
    })
    .to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let artifact = json!({
        "id": analysis_id,
        "tool_name": "analyze_change_request",
        "surface": "preset:full",
        "project_scope": "/tmp/other-project",
        "cache_key": cache_key,
        "summary": "foreign",
        "top_findings": ["foreign"],
        "confidence": 0.5,
        "next_actions": ["ignore"],
        "available_sections": ["summary"],
        "created_at_ms": now_ms,
    });
    fs::write(
        artifact_dir.join("summary.json"),
        serde_json::to_vec_pretty(&artifact).unwrap(),
    )
    .unwrap();

    assert!(state.get_analysis(analysis_id).is_none());
    assert!(state
        .find_reusable_analysis_for_current_scope("analyze_change_request", &cache_key)
        .is_none());
}

#[test]
fn foreign_project_scoped_job_file_is_ignored() {
    let project = project_root();
    let state = make_state(&project);
    let jobs_dir = state.analysis_dir().join("jobs");
    fs::create_dir_all(&jobs_dir).unwrap();
    let job_path = jobs_dir.join("job-foreign.json");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let job = json!({
        "id": "job-foreign",
        "kind": "impact_report",
        "project_scope": "/tmp/other-project",
        "status": "queued",
        "progress": 0,
        "current_step": "queued",
        "profile_hint": "reviewer-graph",
        "estimated_sections": ["impact"],
        "analysis_id": null,
        "error": null,
        "created_at_ms": now_ms,
        "updated_at_ms": now_ms,
    });
    fs::write(&job_path, serde_json::to_vec_pretty(&job).unwrap()).unwrap();

    assert!(state.get_analysis_job("job-foreign").is_none());
    assert!(!job_path.exists());
}

#[test]
fn mutation_tools_write_audit_log() {
    // Phase 2 close part 4: jsonl intent log retired. The sqlite
    // audit_sink absorbs the same per-call metadata via the new
    // session_metadata column.
    let project = project_root();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "audit.txt", "content": "hello"}),
    );
    assert_eq!(payload["success"], json!(true));

    let sink = state.audit_sink().expect("audit sink available");
    let rows = sink.query(None, None, 100).expect("query rows");
    let row = rows
        .iter()
        .find(|r| r.tool == "create_text_file")
        .expect("create_text_file row");
    let metadata = row
        .session_metadata
        .as_ref()
        .expect("session_metadata captured");
    assert_eq!(
        metadata["project_scope"],
        json!(state.current_project_scope()),
        "project_scope must round-trip through session_metadata, got {metadata}"
    );
}

#[test]
fn analysis_artifacts_expire_by_ttl() {
    let project = project_root();
    fs::write(
        project.as_path().join("ttl.py"),
        "def gamma():\n    return 3\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update gamma flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state
        .set_analysis_created_at_for_test(&analysis_id, 0)
        .unwrap();

    assert!(state.get_analysis(&analysis_id).is_none());
    assert!(!state.analysis_dir().join(&analysis_id).exists());
    assert!(state
        .list_analysis_summaries()
        .into_iter()
        .all(|summary| summary.id != analysis_id));
}

#[test]
fn startup_cleanup_removes_expired_analysis_artifacts() {
    let project = project_root();
    fs::write(
        project.as_path().join("startup_ttl.py"),
        "def delta():\n    return 4\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update delta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state
        .set_analysis_created_at_for_test(&analysis_id, 0)
        .unwrap();

    // Must use full constructor — this test verifies startup cleanup behavior.
    let restarted = crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full);
    assert!(!restarted.analysis_dir().join(&analysis_id).exists());
}

#[test]
fn startup_cleanup_preserves_analysis_jobs_dir() {
    let project = project_root();
    fs::write(
        project.as_path().join("jobs_keep.py"),
        "def epsilon():\n    return 5\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "jobs_keep.py"}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap().to_owned();
    let job_path = project
        .as_path()
        .join(".codelens")
        .join("analysis-cache")
        .join("jobs")
        .join(format!("{job_id}.json"));
    assert!(job_path.exists());

    let restarted = make_state(&project);
    assert!(restarted.analysis_dir().join("jobs").exists());
    assert!(job_path.exists());
}

#[test]
fn analysis_reads_update_session_metrics() {
    let project = project_root();
    fs::write(
        project.as_path().join("metrics.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update beta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let _ = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );
    let _ = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_section_reads"],
        json!(1)
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_summary_reads"],
        json!(1)
    );
}

#[test]
fn truncation_followups_are_recorded_in_metrics() {
    let project = project_root();
    fs::write(
        project.as_path().join("truncation.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::PlannerReadonly,
    ));
    state.set_token_budget(1);

    let first = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow"}),
    );
    assert_eq!(first["truncated"], json!(true));

    let second = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow"}),
    );
    assert_eq!(second["truncated"], json!(true));

    state.set_token_budget(3200);
    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["truncated_response_count"],
        json!(2)
    );
    assert_eq!(
        metrics["data"]["session"]["truncation_followup_count"],
        json!(1)
    );
    assert_eq!(
        metrics["data"]["session"]["truncation_same_tool_retry_count"],
        json!(1)
    );
}

#[test]
fn repeated_composite_request_reuses_existing_analysis_handle() {
    let project = project_root();
    fs::write(
        project.as_path().join("reuse.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let first = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow", "profile_hint": "planner-readonly"}),
    );
    let second = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow", "profile_hint": "planner-readonly"}),
    );

    assert_eq!(first["data"]["reused"], json!(false));
    assert_eq!(second["data"]["reused"], json!(true));
    assert_eq!(first["data"]["analysis_id"], second["data"]["analysis_id"]);

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_cache_hit_count"],
        json!(1)
    );
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
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("requires a fresh preflight"));

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
    assert!(fs::read_to_string(project.as_path().join("gated.py"))
        .unwrap()
        .contains("new"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["mutation_with_caution_count"]
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
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("blocked by verifier readiness"));
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
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("symbol-aware preflight"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["rename_without_symbol_preflight_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn stale_preflight_is_rejected() {
    let project = project_root();
    fs::write(project.as_path().join("stale_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update stale gate file",
            "changed_files": ["stale_gate.py"]
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    state.set_recent_preflight_timestamp_for_test(&default_session_id(&state), 0);

    let payload = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "stale_gate.py",
            "old_text": "old",
            "new_text": "new"
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"].as_str().unwrap_or("").contains("stale"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["stale_preflight_reject_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn session_scoped_preflight_does_not_cross_sessions() {
    let project = project_root();
    fs::write(project.as_path().join("session_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update session-gated file",
            "changed_files": ["session_gate.py"]
        }),
        "session-a",
    );
    assert_eq!(preflight["success"], json!(true));

    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "session_gate.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "session-b",
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("requires a fresh preflight"));
}

#[test]
fn get_tool_metrics_filters_by_session_id() {
    let project = project_root();
    fs::write(
        project.as_path().join("session_a.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(project.as_path().join("session_b.py"), "beta\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "session_a.py"}),
        "session-a",
    );
    let _ = call_tool_with_session(
        &state,
        "read_file",
        json!({"relative_path": "session_b.py"}),
        "session-b",
    );

    let metrics_a = call_tool(
        &state,
        "get_tool_metrics",
        json!({"session_id": "session-a"}),
    );
    let metrics_b = call_tool(
        &state,
        "get_tool_metrics",
        json!({"session_id": "session-b"}),
    );

    let per_tool_a = metrics_a["data"]["per_tool"]
        .as_array()
        .expect("session-a per_tool array");
    let per_tool_b = metrics_b["data"]["per_tool"]
        .as_array()
        .expect("session-b per_tool array");

    assert_eq!(metrics_a["data"]["scope"], json!("session"));
    assert_eq!(metrics_a["data"]["session_id"], json!("session-a"));
    assert_eq!(metrics_b["data"]["session_id"], json!("session-b"));
    assert!(per_tool_a
        .iter()
        .any(|entry| entry["tool"] == json!("get_symbols_overview")));
    assert!(!per_tool_a
        .iter()
        .any(|entry| entry["tool"] == json!("read_file")));
    assert!(per_tool_b
        .iter()
        .any(|entry| entry["tool"] == json!("read_file")));
    assert!(!per_tool_b
        .iter()
        .any(|entry| entry["tool"] == json!("get_symbols_overview")));
}

#[test]
fn audit_builder_session_is_not_applicable_for_non_builder_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
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
        "replace",
        json!({
            "relative_path": "bootstrap_warn.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "builder-warn",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "builder-warn"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(audit["data"]["findings"]
        .as_array()
        .map(|findings| findings
            .iter()
            .any(|finding| finding["code"] == json!("bootstrap_order")))
        .unwrap_or(false));
}

#[test]
fn audit_builder_session_fails_when_gate_failure_was_recorded() {
    let project = project_root();
    fs::write(project.as_path().join("audit_fail.py"), "print('old')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "builder-fail",
    );
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
        "replace_content",
        json!({
            "relative_path": "audit_fail.py",
            "old_text": "old",
            "new_text": "new"
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
    assert!(audit["data"]["findings"]
        .as_array()
        .map(|findings| findings
            .iter()
            .any(|finding| finding["code"] == json!("mutation_gate")))
        .unwrap_or(false));
}

#[cfg(feature = "http")]
fn make_http_state(project: &codelens_engine::ProjectRoot) -> crate::AppState {
    crate::AppState::new_minimal(project.clone(), crate::tool_defs::ToolPreset::Full)
        .with_session_store()
}

#[cfg(feature = "http")]
fn create_http_profile_session(
    state: &crate::AppState,
    project: &codelens_engine::ProjectRoot,
    profile: crate::tool_defs::ToolProfile,
) -> String {
    let session = state
        .session_store
        .as_ref()
        .expect("session store")
        .create();
    let session_id = session.id.clone();
    session.set_client_metadata(crate::server::session::SessionClientMetadata {
        client_name: Some("BuilderHarness".to_owned()),
        requested_profile: Some(profile.as_str().to_owned()),
        project_path: Some(project.as_path().to_string_lossy().to_string()),
        ..Default::default()
    });
    state.set_session_surface_and_budget(
        &session_id,
        crate::tool_defs::ToolSurface::Profile(profile),
        crate::tool_defs::default_budget_for_profile(profile),
    );
    session_id
}

#[cfg(feature = "http")]
#[test]
fn audit_builder_session_passes_for_happy_path_http_builder() {
    let project = project_root();
    fs::write(project.as_path().join("http_builder.py"), "print('old')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "refactor-full", "detail": "compact"}),
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
        "replace_content",
        json!({
            "relative_path": "http_builder.py",
            "old_text": "old",
            "new_text": "new"
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

#[cfg(feature = "http")]
#[test]
fn audit_builder_session_warns_when_http_coordination_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("http_warn.py"), "print('old')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "refactor-full", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "http_warn.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "update http warn file", "changed_files": ["http_warn.py"]}),
        &session_id,
    );
    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "http_warn.py",
            "old_text": "old",
            "new_text": "new"
        }),
        &session_id,
    );
    assert_eq!(payload["success"], json!(true));

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(audit["data"]["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .any(|finding| finding["code"] == json!("coordination_registration"))
                && findings
                    .iter()
                    .any(|finding| finding["code"] == json!("coordination_claim"))
        })
        .unwrap_or(false));
}

#[test]
fn audit_planner_session_is_not_applicable_for_non_planner_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "builder-minimal"}),
        "builder-session",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "builder-session"}),
    );
    assert_eq!(audit["data"]["status"], json!("not_applicable"));
}

#[cfg(feature = "http")]
#[test]
fn audit_planner_session_passes_for_happy_path_reviewer() {
    let project = project_root();
    fs::write(project.as_path().join("planner_pass.py"), "print('ok')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_pass.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_pass.py"], "task": "review planner path"}),
        &session_id,
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("pass"));
}

#[test]
fn audit_planner_session_warns_when_bootstrap_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("planner_warn.py"), "print('ok')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_warn.py"}),
        "planner-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_warn.py"], "task": "review planner path"}),
        "planner-warn",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-warn"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(audit["data"]["findings"]
        .as_array()
        .map(|findings| findings
            .iter()
            .any(|finding| finding["code"] == json!("bootstrap_order")))
        .unwrap_or(false));
}

#[test]
fn audit_planner_session_warns_when_change_evidence_is_missing() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-change-evidence",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "planner-change-evidence",
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "review change evidence"}),
        "planner-change-evidence",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-change-evidence"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(audit["data"]["findings"]
        .as_array()
        .map(|findings| findings
            .iter()
            .any(|finding| finding["code"] == json!("change_evidence")))
        .unwrap_or(false));
}

#[test]
fn audit_planner_session_warns_when_workflow_first_guidance_is_missed() {
    let project = project_root();
    fs::write(project.as_path().join("planner_chain.py"), "print('ok')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "planner-readonly", "detail": "compact"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_chain.py"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "find_symbol",
        json!({"name": "missing_symbol", "include_body": true}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "planner_chain.py"}),
        "planner-chain",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-chain"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(audit["data"]["findings"]
        .as_array()
        .map(|findings| findings
            .iter()
            .any(|finding| finding["code"] == json!("workflow_first")))
        .unwrap_or(false));
}

#[test]
fn audit_planner_session_fails_on_mutation_attempt() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-fail",
    );
    let payload = call_tool_with_session(
        &state,
        "create_text_file",
        json!({"relative_path": "planner_fail.py", "content": "print('fail')\n"}),
        "planner-fail",
    );
    assert_eq!(payload["success"], json!(false));

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-fail"}),
    );
    assert_eq!(audit["data"]["status"], json!("fail"));
}

#[cfg(feature = "http")]
#[test]
fn audit_planner_session_isolated_by_session_id() {
    let project = project_root();
    fs::write(project.as_path().join("planner_a.py"), "print('a')\n").unwrap();
    fs::write(project.as_path().join("planner_b.py"), "print('b')\n").unwrap();
    let state = make_http_state(&project);
    let session_a = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );
    let session_b = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_a.py"], "task": "review a"}),
        &session_a,
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &session_b,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_b.py"}),
        &session_b,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_b.py"], "task": "review b"}),
        &session_b,
    );

    let metrics_a = call_tool(
        &state,
        "get_tool_metrics",
        json!({"session_id": session_a.as_str()}),
    );
    assert_eq!(metrics_a["data"]["session_id"], json!(session_a.as_str()));
    let audit_a = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": session_a.as_str()}),
    );
    assert_eq!(audit_a["data"]["status"], json!("warn"));
    assert!(audit_a["data"]["findings"]
        .as_array()
        .map(|findings| findings
            .iter()
            .any(|finding| finding["code"] == json!("bootstrap_order")))
        .unwrap_or(false));
}

#[test]
fn export_session_markdown_appends_planner_audit_summary() {
    let project = project_root();
    fs::write(project.as_path().join("planner_md.py"), "print('md')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_md.py"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_md.py"], "task": "review markdown"}),
        "planner-md",
    );

    let markdown = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "planner-md", "name": "planner-md"}),
    );
    let body = markdown["data"]["markdown"].as_str().unwrap_or("");
    assert!(body.contains("## Planner Audit"));
}

#[test]
fn export_session_markdown_is_safe_for_unknown_session_id() {
    let project = project_root();
    fs::write(
        project.as_path().join("planner_md_unknown.py"),
        "print('md')\n",
    )
    .unwrap();
    let state = make_state(&project);

    let markdown = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "missing-session", "name": "missing-session"}),
    );
    assert_eq!(markdown["success"], json!(false));
    assert_eq!(
        markdown["error"],
        json!("Not found: unknown session_id `missing-session`")
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

#[cfg(feature = "semantic")]
#[test]
fn replace_content_reindexes_existing_embedding_index_when_engine_is_not_loaded() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    fs::write(
        project.as_path().join("semantic_mutation.py"),
        "def winter_orbit_launch():\n    return 1\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);
    let engine = codelens_engine::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);
    assert!(state.embedding_ref().is_none());

    let payload = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "semantic_mutation.py",
            "old_text": "winter_orbit_launch",
            "new_text": "ember_archive_delta"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(state.embedding_ref().is_some());

    let search = call_tool(
        &state,
        "semantic_search",
        json!({"query": "ember archive delta", "max_results": 5}),
    );
    assert_eq!(search["success"], json!(true));
    assert_eq!(
        search["data"]["retrieval"]["semantic_query"],
        json!("ember archive delta")
    );
    assert!(search["data"]["results"]
        .as_array()
        .map(|results| {
            results
                .iter()
                .all(|result| result["provenance"]["source"] == json!("semantic"))
                && results
                    .iter()
                    .all(|result| result["provenance"]["adjusted_score"].is_number())
                && results
                    .iter()
                    .any(|result| result.get("symbol_name") == Some(&json!("ember_archive_delta")))
        })
        .unwrap_or(false));
}

// ── Workflow alias success-contract tests ────────────────────────────────────

/// Global mutex to serialise tests that temporarily mutate PATH so they don't
/// stomp each other when the test runner uses multiple threads.
static PATH_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn prepend_path(dir: &std::path::Path, original_path: &str) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(original_path));
    std::env::join_paths(paths).expect("join PATH entries")
}

#[test]
fn diagnose_issues_returns_structured_content() {
    // diagnose_issues with a path delegates to get_file_diagnostics, which
    // needs an LSP server.  We create a minimal python3-based mock named
    // `pyright-langserver` (the default binary for .py files) in a temp bin
    // directory and prepend it to PATH for the duration of the test.
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
        "    elif m == 'textDocument/diagnostic':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );

    let bin_dir = std::env::temp_dir().join(format!(
        "codelens-test-bin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(windows)]
    let mock_script = bin_dir.join("pyright-langserver.py");
    #[cfg(not(windows))]
    let mock_script = bin_dir.join("pyright-langserver");
    fs::write(&mock_script, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    {
        let wrapper = bin_dir.join("pyright-langserver.cmd");
        fs::write(
            &wrapper,
            format!("@echo off\r\npython \"{}\" %*\r\n", mock_script.display()),
        )
        .unwrap();
    }

    let project = project_root();
    fs::write(
        project.as_path().join("diag_test.py"),
        "def hello():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _guard = PATH_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
    let original_path = std::env::var("PATH").unwrap_or_default();
    let patched_path = prepend_path(&bin_dir, &original_path);
    // SAFETY: protected by PATH_MUTEX; no other thread modifies PATH concurrently.
    unsafe {
        std::env::set_var("PATH", &patched_path);
    }

    let payload = call_tool(&state, "diagnose_issues", json!({"path": "diag_test.py"}));

    // SAFETY: restoring PATH; still under PATH_MUTEX.
    unsafe {
        std::env::set_var("PATH", original_path);
    }

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("diagnose_issues"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("get_file_diagnostics")
    );
    assert!(payload["suggested_next_tools"]
        .as_array()
        .map(|items| items.iter().any(|value| value == "review_changes"))
        .unwrap_or(false));
    assert!(payload["suggested_next_calls"]
        .as_array()
        .map(|items| {
            items.iter().any(|entry| {
                entry["tool"] == json!("review_changes")
                    && entry["arguments"]["path"] == json!("diag_test.py")
            })
        })
        .unwrap_or(false));
}

#[test]
fn assess_change_readiness_returns_structured_content() {
    // assess_change_readiness delegates to verify_change_readiness which
    // requires a `task` string (used as the ranked-context query).
    let project = project_root();
    fs::write(
        project.as_path().join("ready_test.py"),
        "class Foo:\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "assess_change_readiness",
        json!({
            "task": "check readiness for ready_test.py",
            "changed_files": ["ready_test.py"]
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["workflow"],
        json!("assess_change_readiness")
    );
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("verify_change_readiness")
    );
    assert_eq!(payload["data"]["deprecated"], json!(true));
    assert_eq!(
        payload["data"]["replacement_tool"],
        json!("verify_change_readiness")
    );
    assert_eq!(payload["data"]["removal_target"], json!("v2.0"));
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
fn cleanup_duplicate_logic_returns_structured_content() {
    // cleanup_duplicate_logic without the semantic feature delegates to
    // dead_code_report (no required args).
    let project = project_root();
    fs::write(
        project.as_path().join("dup_test.py"),
        "def foo():\n    return 1\ndef bar():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "cleanup_duplicate_logic", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["workflow"],
        json!("cleanup_duplicate_logic")
    );
}

#[test]
fn surface_overlay_resource_returns_compiled_plan() {
    let project = project_root();
    let state = make_state(&project);

    // 1. List includes the overlay URI
    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1001)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let list_body = serde_json::to_string(&list_response).unwrap();
    assert!(list_body.contains("codelens://surface/overlay"));
    assert!(list_body.contains("symbiote://surface/overlay"));

    // 2. Read with host + task renders a compiled plan
    let read_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1002)),
            method: "resources/read".to_owned(),
            params: Some(json!({
                "uri": "codelens://surface/overlay",
                "host": "codex",
                "task": "editing",
            })),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&read_response).unwrap();
    assert!(body.contains("\\\"applied\\\": true") || body.contains("applied"));
    assert!(body.contains("codex"));
    assert!(body.contains("editing"));
    assert!(body.contains("codex-builder"));
    assert!(body.contains("rename_symbol"));
    assert!(body.contains("routing_notes"));

    // 3. Unknown host is reported back without failing
    let unknown_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1003)),
            method: "resources/read".to_owned(),
            params: Some(json!({
                "uri": "codelens://surface/overlay",
                "host": "nonexistent-host",
            })),
        },
    )
    .unwrap();
    let unknown_body = serde_json::to_string(&unknown_response).unwrap();
    assert!(unknown_body.contains("unknown_host"));
    assert!(unknown_body.contains("nonexistent-host"));

    // 4. No params → non-applied plan (regression guard)
    let empty_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1004)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://surface/overlay"})),
        },
    )
    .unwrap();
    let empty_body = serde_json::to_string(&empty_response).unwrap();
    assert!(empty_body.contains("applied"));
    assert!(empty_body.contains("preferred_entrypoints"));
}

#[test]
fn backend_capabilities_resource_reports_all_known_backends() {
    let project = project_root();
    let state = make_state(&project);

    // 1. List includes the backend URI
    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2001)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let list_body = serde_json::to_string(&list_response).unwrap();
    assert!(list_body.contains("codelens://backend/capabilities"));
    assert!(list_body.contains("symbiote://backend/capabilities"));

    // 2. Read returns the declared backends + capability coverage
    let read_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2002)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://backend/capabilities"})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&read_response).unwrap();
    assert!(body.contains("rust-engine"));
    assert!(body.contains("lsp-bridge"));
    assert!(body.contains("scip-bridge"));
    assert!(body.contains("semantic-edit-backend"));
    assert!(body.contains("declared"));
    assert!(body.contains("active"));
    assert!(body.contains("active_reason"));
    assert!(body.contains("capability_coverage"));
    assert!(body.contains("symbol_lookup"));
    assert!(body.contains("diagnostics"));
    assert!(body.contains("semantic_search"));
    assert!(body.contains("semantic_edit_backend"));
    // Passive scaffold disclosure present
    assert!(body.contains("Passive scaffold"));
}

#[test]
fn operator_dashboard_aggregates_across_existing_telemetry() {
    let project = project_root();
    let state = make_state(&project);

    // 1. Listed
    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(4001)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let list_body = serde_json::to_string(&list_response).unwrap();
    assert!(list_body.contains("codelens://operator/dashboard"));
    assert!(list_body.contains("symbiote://operator/dashboard"));

    // 2. Read returns all aggregated sections
    let read_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(4002)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://operator/dashboard"})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&read_response).unwrap();
    assert!(body.contains("project_root"));
    assert!(body.contains("active_surface"));
    assert!(body.contains("daemon_mode"));
    assert!(body.contains("daemon_started_at"));
    assert!(body.contains("\\\"health\\\""));
    assert!(body.contains("indexed_files"));
    assert!(body.contains("\\\"jobs\\\""));
    assert!(body.contains("status_counts"));
    assert!(body.contains("\\\"analyses\\\""));
    assert!(body.contains("tool_counts"));
    assert!(body.contains("\\\"backends\\\""));
    assert!(body.contains("rust-engine"));
    assert!(body.contains("memory_scopes"));
    assert!(body.contains("Operator plane aggregates"));
}
