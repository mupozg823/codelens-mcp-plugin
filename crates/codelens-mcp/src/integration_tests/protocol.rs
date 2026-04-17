use super::*;

// ── Protocol-level tests ─────────────────────────────────────────────

#[test]
fn lists_tools() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .expect("tools/list should return a response");
    assert!(tools().len() >= 64);
    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("get_symbols_overview"));
    assert!(encoded.contains("active_surface"));
}

#[test]
fn notifications_return_none() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    for method in &[
        "notifications/initialized",
        "notifications/cancelled",
        "notifications/progress",
    ] {
        let result = handle_request(
            &state,
            crate::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: None,
                method: method.to_string(),
                params: None,
            },
        );
        assert!(result.is_none(), "notification {method} should return None");
    }
}

#[test]
fn set_preset_changes_tools_list() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let full_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let full_json = serde_json::to_string(&full_resp).unwrap();
    assert!(
        full_json.contains("find_dead_code"),
        "Full preset should include find_dead_code"
    );
    assert!(
        full_json.contains("set_preset"),
        "Full preset should include set_preset"
    );

    let set_resp = call_tool(&state, "set_preset", json!({"preset": "minimal"}));
    assert_eq!(set_resp["data"]["current_preset"], "Minimal");

    let min_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let min_json = serde_json::to_string(&min_resp).unwrap();
    assert!(
        !min_json.contains("find_dead_code"),
        "Minimal preset should NOT include find_dead_code"
    );
    assert!(
        min_json.contains("find_symbol"),
        "Minimal preset should include find_symbol"
    );

    let bal_resp = call_tool(&state, "set_preset", json!({"preset": "balanced"}));
    assert_eq!(bal_resp["data"]["current_preset"], "Balanced");
}

#[test]
fn set_profile_changes_tools_list() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let profile_resp = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );
    assert_eq!(profile_resp["data"]["current_profile"], "planner-readonly");

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(9)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("analyze_change_request"));
    assert!(!encoded.contains("\"analyze_change_impact\""));
    assert!(!encoded.contains("\"assess_change_readiness\""));
    assert!(!encoded.contains("\"rename_symbol\""));

    let builder_resp = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));
    assert_eq!(builder_resp["data"]["current_profile"], "builder-minimal");
    let builder_list = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(10)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let builder_encoded = serde_json::to_string(&builder_list).unwrap();
    assert!(!builder_encoded.contains("\"find_dead_code\""));
    assert!(builder_encoded.contains("\"find_symbol\""));
    assert!(builder_encoded.contains("\"create_text_file\""));
    assert!(!builder_encoded.contains("\"start_analysis_job\""));
    assert!(builder_encoded.contains("\"add_import\""));
    assert!(builder_encoded.contains("\"verify_change_readiness\""));
    assert!(!builder_encoded.contains("\"unresolved_reference_check\""));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["profile_switch_count"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
}

#[test]
fn tools_list_can_be_filtered_by_namespace() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(101)),
            method: "tools/list".to_owned(),
            params: Some(json!({"namespace": "reports"})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"selected_namespace\":\"reports\""));
    assert!(encoded.contains("\"impact_report\""));
    assert!(!encoded.contains("\"find_symbol\""));
}

#[test]
fn tools_list_can_be_filtered_by_phase() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    // phase=build surfaces mutation tools but not planning reports.
    let build_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(201)),
            method: "tools/list".to_owned(),
            params: Some(json!({"phase": "build"})),
        },
    )
    .unwrap();
    let build_encoded = serde_json::to_string(&build_resp).unwrap();
    assert!(
        build_encoded.contains("\"rename_symbol\""),
        "build phase should include rename_symbol"
    );
    assert!(
        build_encoded.contains("\"replace_symbol_body\""),
        "build phase should include replace_symbol_body"
    );
    assert!(
        !build_encoded.contains("\"impact_report\""),
        "build phase should not include impact_report (plan tool)"
    );

    // phase=plan surfaces analysis/retrieval, not mutations.
    let plan_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(202)),
            method: "tools/list".to_owned(),
            params: Some(json!({"phase": "plan"})),
        },
    )
    .unwrap();
    let plan_encoded = serde_json::to_string(&plan_resp).unwrap();
    assert!(plan_encoded.contains("\"impact_report\""));
    assert!(plan_encoded.contains("\"find_symbol\""));
    assert!(
        !plan_encoded.contains("\"rename_symbol\""),
        "plan phase should not include mutation tools"
    );

    // Phase-agnostic infrastructure (read_file) should pass through in both.
    assert!(
        build_encoded.contains("\"read_file\""),
        "phase filter should pass through phase-agnostic infrastructure"
    );
    assert!(plan_encoded.contains("\"read_file\""));

    // Unknown phase label falls back to unfiltered — malformed input does
    // not strip the surface.
    let bogus_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(203)),
            method: "tools/list".to_owned(),
            params: Some(json!({"phase": "deploy"})),
        },
    )
    .unwrap();
    let bogus_encoded = serde_json::to_string(&bogus_resp).unwrap();
    assert!(bogus_encoded.contains("\"rename_symbol\""));
    assert!(bogus_encoded.contains("\"impact_report\""));
}

#[test]
fn profile_declares_preferred_phases_as_adoption_signal() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    // planner-readonly is curated to emit plan + review tools as its
    // preferred phase surface; the manifest field proves CodeLens
    // itself consumes the phase alias, not just its hosts.
    let _ = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(301)),
            method: "tools/list".to_owned(),
            params: Some(json!({})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(
        encoded.contains("\"preferred_phases\":[\"plan\",\"review\"]"),
        "planner-readonly must advertise plan+review as preferred phases"
    );

    // builder-minimal advertises build+review.
    let _ = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));
    let list_builder = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(302)),
            method: "tools/list".to_owned(),
            params: Some(json!({})),
        },
    )
    .unwrap();
    let encoded_builder = serde_json::to_string(&list_builder).unwrap();
    assert!(
        encoded_builder.contains("\"preferred_phases\":[\"build\",\"review\"]"),
        "builder-minimal must advertise build+review as preferred phases"
    );

    // workflow-first is intentionally phase-agnostic.
    let _ = call_tool(&state, "set_profile", json!({"profile": "workflow-first"}));
    let list_workflow = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(303)),
            method: "tools/list".to_owned(),
            params: Some(json!({})),
        },
    )
    .unwrap();
    let encoded_workflow = serde_json::to_string(&list_workflow).unwrap();
    assert!(
        encoded_workflow.contains("\"preferred_phases\":[]"),
        "workflow-first must remain phase-agnostic"
    );
}

#[test]
fn deferred_tools_list_defaults_to_preferred_namespaces_only() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1011)),
            method: "tools/list".to_owned(),
            params: Some(json!({"_session_deferred_tool_loading": true})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"deferred_loading_active\":true"));
    assert!(encoded
        .contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]"));
    assert!(encoded.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(encoded.contains("\"loaded_tiers\":[]"));
    assert!(encoded.contains("\"review_architecture\""));
    assert!(encoded.contains("\"review_changes\""));
    assert!(encoded.contains("\"cleanup_duplicate_logic\""));
    assert!(!encoded.contains("\"analyze_change_impact\""));
    assert!(!encoded.contains("\"audit_security_context\""));
    assert!(!encoded.contains("\"find_symbol\""));
    assert!(!encoded.contains("\"read_file\""));
    assert!(encoded.contains("\"tool_count_total\""));
}

#[test]
fn refactor_deferred_tools_list_starts_preview_first() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1012)),
            method: "tools/list".to_owned(),
            params: Some(json!({"_session_deferred_tool_loading": true})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"deferred_loading_active\":true"));
    assert!(encoded.contains("\"preferred_namespaces\":[\"reports\",\"session\"]"));
    assert!(encoded.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(encoded.contains("\"tool_count\":"));
    assert!(encoded.contains("\"plan_safe_refactor\""));
    assert!(encoded.contains("\"review_changes\""));
    assert!(encoded.contains("\"trace_request_path\""));
    assert!(!encoded.contains("\"analyze_change_impact\""));
    assert!(encoded.contains("\"activate_project\""));
    assert!(encoded.contains("\"set_profile\""));
    assert!(!encoded.contains("\"name\":\"rename_symbol\""));
    assert!(!encoded.contains("\"name\":\"replace_symbol_body\""));
    assert!(!encoded.contains("\"name\":\"refactor_extract_function\""));
    assert!(!encoded.contains("\"name\":\"verify_change_readiness\""));
    assert!(!encoded.contains("\"name\":\"refactor_safety_report\""));
    assert!(!encoded.contains("\"name\":\"safe_rename_report\""));
    assert!(!encoded.contains("\"name\":\"unresolved_reference_check\""));
}

#[test]
fn codex_client_name_enables_lean_tools_list_contract() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "_session_client_name": "CodexHarness",
            })),
        },
    )
    .expect("tools/list should return a response");

    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("\"client_profile\":\"codex\""));
    assert!(encoded.contains("\"default_contract_mode\":\"lean\""));
    assert!(encoded.contains("\"include_output_schema\":false"));
    assert!(encoded.contains("\"include_annotations\":false"));
    assert!(!encoded.contains("\"outputSchema\""));
    assert!(!encoded.contains("\"annotations\""));
    assert!(!encoded.contains("\"visible_namespaces\""));
}

#[test]
fn claude_client_name_keeps_full_tools_list_contract() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "_session_client_name": "Claude Code",
            })),
        },
    )
    .expect("tools/list should return a response");

    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("\"client_profile\":\"claude\""));
    assert!(encoded.contains("\"default_contract_mode\":\"full\""));
    assert!(encoded.contains("\"include_output_schema\":true"));
    assert!(encoded.contains("\"include_annotations\":true"));
    assert!(encoded.contains("\"outputSchema\""));
    assert!(encoded.contains("\"annotations\""));
    assert!(encoded.contains("\"visible_namespaces\""));
}

#[test]
fn codex_client_can_restore_annotations_explicitly() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "_session_client_name": "CodexHarness",
                "includeAnnotations": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("\"client_profile\":\"codex\""));
    assert!(encoded.contains("\"include_annotations\":true"));
    assert!(encoded.contains("\"annotations\""));
}

#[test]
fn deferred_tools_list_omits_output_schema_by_default() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "_session_deferred_tool_loading": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("\"include_output_schema\":false"));
    assert!(
        !encoded.contains("\"outputSchema\""),
        "deferred bootstrap should omit outputSchema by default"
    );
}

#[test]
fn deferred_tools_list_can_restore_output_schema_explicitly() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "_session_deferred_tool_loading": true,
                "includeOutputSchema": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("\"include_output_schema\":true"));
    assert!(
        encoded.contains("\"outputSchema\""),
        "explicit includeOutputSchema should preserve output schemas"
    );
}

#[test]
fn refactor_profile_limits_surface_to_approved_mutations() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let profile_resp = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));
    assert_eq!(profile_resp["data"]["current_profile"], "refactor-full");

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(11)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"rename_symbol\""));
    assert!(encoded.contains("\"refactor_safety_report\""));
    assert!(!encoded.contains("\"write_memory\""));
    assert!(!encoded.contains("\"add_queryable_project\""));
}

#[test]
fn read_only_daemon_rejects_mutation_even_with_mutating_profile() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::ReadOnly);

    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "blocked.txt", "content": "nope"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("blocked by daemon mode"));
}

#[test]
fn hidden_tools_are_blocked_at_call_time() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );

    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "blocked.txt", "content": "nope"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("not available in active surface"));
}

#[test]
fn read_only_surface_marks_content_mutations_for_blocking() {
    assert!(crate::tool_defs::is_read_only_surface(
        crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::PlannerReadonly),
    ));
    assert!(crate::tool_defs::is_content_mutation_tool(
        "create_text_file"
    ));
    assert!(!crate::tool_defs::is_content_mutation_tool("set_profile"));
}

#[test]
fn watch_status_reports_lock_contention_field() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert!(payload["data"].get("lock_contention_batches").is_some());
    assert!(payload["data"].get("index_failures").is_some());
    assert!(payload["data"].get("index_failures_total").is_some());
    assert!(payload["data"].get("stale_index_failures").is_some());
    assert!(payload["data"].get("persistent_index_failures").is_some());
    assert!(payload["data"].get("pruned_missing_failures").is_some());
    assert!(payload["data"]
        .get("recent_failure_window_seconds")
        .is_some());
}

#[test]
fn watch_status_is_read_only_for_failure_health() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert_eq!(
        payload["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        0
    );
    assert_eq!(
        payload["data"]["index_failures_total"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 1);
}

#[test]
fn prune_index_failures_explicitly_cleans_missing_failure_records() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let payload = call_tool(&state, "prune_index_failures", json!({}));
    assert_eq!(
        payload["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        payload["data"]["index_failures_total"]
            .as_u64()
            .unwrap_or_default(),
        0
    );
    let watch_status = call_tool(&state, "get_watch_status", json!({}));
    assert_eq!(
        watch_status["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 0);
}

#[test]
fn observability_reads_do_not_mutate_index_failures() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let _ = call_tool(&state, "get_tool_metrics", json!({}));
    let _ = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2502)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://stats/token-efficiency"})),
        },
    )
    .unwrap();

    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 1);
}
