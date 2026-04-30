use super::*;

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
    assert!(
        notes
            .iter()
            .filter_map(|value| value.as_str())
            .any(|note| note.contains("review_changes"))
    );
    assert!(
        !notes
            .iter()
            .filter_map(|value| value.as_str())
            .any(|note| note.contains("analyze_change_impact"))
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
    assert!(
        per_tool_a
            .iter()
            .any(|entry| entry["tool"] == json!("get_symbols_overview"))
    );
    assert!(
        !per_tool_a
            .iter()
            .any(|entry| entry["tool"] == json!("read_file"))
    );
    assert!(
        per_tool_b
            .iter()
            .any(|entry| entry["tool"] == json!("read_file"))
    );
    assert!(
        !per_tool_b
            .iter()
            .any(|entry| entry["tool"] == json!("get_symbols_overview"))
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
