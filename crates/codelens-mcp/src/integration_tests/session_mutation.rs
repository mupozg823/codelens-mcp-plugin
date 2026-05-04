use super::*;

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
            params: Some(json!({"full": true, "include_deprecated": true})),
        },
    )
    .unwrap();
    let full_json = serde_json::to_string(&full_resp).unwrap();
    assert!(
        full_json.contains("dead_code_report"),
        "Full preset should include dead_code_report"
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
        !min_json.contains("dead_code_report"),
        "Minimal preset should NOT include dead_code_report"
    );
    assert!(
        min_json.contains("get_ranked_context"),
        "Default minimal listing should include the MVP retrieval entrypoint"
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
    assert!(encoded.contains("get_ranked_context"));
    assert!(!encoded.contains("analyze_change_request"));
    assert!(!encoded.contains("\"analyze_change_impact\""));
    assert!(!encoded.contains("\"assess_change_readiness\""));
    assert!(!encoded.contains("\"rename_symbol\""));

    let expanded_planner_list = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(91)),
            method: "tools/list".to_owned(),
            params: Some(json!({"full": true})),
        },
    )
    .unwrap();
    let expanded_planner_encoded = serde_json::to_string(&expanded_planner_list).unwrap();
    assert!(expanded_planner_encoded.contains("analyze_change_request"));

    let builder_resp = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));
    assert_eq!(builder_resp["data"]["current_profile"], "builder-minimal");
    let builder_list = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(10)),
            method: "tools/list".to_owned(),
            params: Some(json!({"full": true})),
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
fn surface_mutation_responses_emit_host_action_hint() {
    // #199-B-4: Some MCP hosts cache the visible tool surface and need an
    // explicit reload signal after set_preset / set_profile, otherwise the
    // newly-available tools never appear in the host's deferred pool. The
    // nested `host_action` shape keeps both the programmatic
    // `required` field and the human-readable `hint` under a single key
    // so the response stays under the text-channel field cap.
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let preset_resp = call_tool(&state, "set_preset", json!({"preset": "balanced"}));
    assert_eq!(
        preset_resp["data"]["host_action"]["required"],
        json!("reload_tools_list"),
        "set_preset must emit host_action.required so cached-surface hosts know to refresh"
    );
    let preset_hint = preset_resp["data"]["host_action"]["hint"]
        .as_str()
        .unwrap_or("");
    assert!(
        preset_hint.contains("tools/list"),
        "set_preset host_action.hint should mention tools/list refresh: {preset_hint}"
    );

    let profile_resp = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));
    assert_eq!(
        profile_resp["data"]["host_action"]["required"],
        json!("reload_tools_list"),
        "set_profile must emit host_action.required so cached-surface hosts know to refresh"
    );
    let profile_hint = profile_resp["data"]["host_action"]["hint"]
        .as_str()
        .unwrap_or("");
    assert!(
        profile_hint.contains("tools/list"),
        "set_profile host_action.hint should mention tools/list refresh: {profile_hint}"
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
    assert!(encoded.contains("\"verify_change_readiness\""));
    assert!(!encoded.contains("\"name\":\"rename_symbol\""));
    assert!(!encoded.contains("\"name\":\"refactor_safety_report\""));

    let expanded_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(12)),
            method: "tools/list".to_owned(),
            params: Some(json!({"full": true})),
        },
    )
    .unwrap();
    let expanded_encoded = serde_json::to_string(&expanded_resp).unwrap();
    assert!(expanded_encoded.contains("\"rename_symbol\""));
    assert!(expanded_encoded.contains("\"refactor_safety_report\""));
    assert!(!encoded.contains("\"write_memory\""));
    assert!(!encoded.contains("\"add_queryable_project\""));
    assert!(!expanded_encoded.contains("\"write_memory\""));
    assert!(!expanded_encoded.contains("\"add_queryable_project\""));
}
