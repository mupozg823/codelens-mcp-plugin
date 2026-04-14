use super::*;

// ── Protocol-level tests ─────────────────────────────────────────────

fn result_payload(response: &crate::protocol::JsonRpcResponse) -> serde_json::Value {
    serde_json::to_value(response)
        .expect("serialize")
        .get("result")
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn resource_payload(response: &crate::protocol::JsonRpcResponse) -> serde_json::Value {
    let value = serde_json::to_value(response).expect("serialize");
    let text = value["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    serde_json::from_str(text).expect("valid resource JSON")
}

fn assert_fields_match(
    left: &serde_json::Value,
    right: &serde_json::Value,
    fields: &[&str],
) {
    for field in fields {
        assert_eq!(left[*field], right[*field], "field mismatch for `{field}`");
    }
}

fn tool_names_from_field(payload: &serde_json::Value, field: &str) -> Vec<String> {
    payload[field]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            item.get("name")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect()
}

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
    let value = serde_json::to_value(&response).expect("serialize");
    let prepare = value["result"]["tools"]
        .as_array()
        .and_then(|tools| {
            tools.iter().find(|tool| {
                tool.get("name").and_then(|name| name.as_str()) == Some("prepare_harness_session")
            })
        })
        .expect("prepare_harness_session should be listed");
    assert_eq!(
        prepare["orchestrationContract"]["orchestration_owner"],
        json!("host")
    );
    assert_eq!(
        prepare["orchestrationContract"]["server_role"],
        json!("supporting_mcp")
    );
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
    assert!(
        encoded
            .contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]")
    );
    assert!(encoded.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(encoded.contains("\"loaded_tiers\":[]"));
    assert!(encoded.contains("\"review_architecture\""));
    assert!(encoded.contains("\"analyze_change_impact\""));
    assert!(encoded.contains("\"audit_security_context\""));
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
    assert!(encoded.contains("\"tool_count\":6"));
    assert!(encoded.contains("\"activate_project\""));
    assert!(encoded.contains("\"prepare_harness_session\""));
    assert!(encoded.contains("\"set_profile\""));
    assert!(encoded.contains("\"verify_change_readiness\""));
    assert!(encoded.contains("\"safe_rename_report\""));
    assert!(encoded.contains("\"refactor_safety_report\""));
    assert!(!encoded.contains("\"name\":\"rename_symbol\""));
    assert!(!encoded.contains("\"name\":\"replace_symbol_body\""));
    assert!(!encoded.contains("\"name\":\"refactor_extract_function\""));
    assert!(!encoded.contains("\"name\":\"unresolved_reference_check\""));
    assert!(!encoded.contains("\"name\":\"plan_safe_refactor\""));
    assert!(!encoded.contains("\"name\":\"analyze_change_impact\""));
    assert!(!encoded.contains("\"name\":\"trace_request_path\""));
    assert!(!encoded.contains("\"name\":\"get_capabilities\""));
    assert!(!encoded.contains("\"name\":\"get_current_config\""));
    assert!(!encoded.contains("\"name\":\"set_preset\""));
}

#[test]
fn workflow_first_deferred_tools_list_starts_bootstrap_first() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "workflow-first"}));

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1013)),
            method: "tools/list".to_owned(),
            params: Some(json!({"_session_deferred_tool_loading": true})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"deferred_loading_active\":true"));
    assert!(encoded.contains("\"preferred_namespaces\":[\"reports\",\"session\"]"));
    assert!(encoded.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(encoded.contains("\"tool_count\":8"));
    assert!(encoded.contains("\"activate_project\""));
    assert!(encoded.contains("\"prepare_harness_session\""));
    assert!(encoded.contains("\"set_profile\""));
    assert!(encoded.contains("\"explore_codebase\""));
    assert!(encoded.contains("\"trace_request_path\""));
    assert!(encoded.contains("\"review_architecture\""));
    assert!(encoded.contains("\"analyze_change_impact\""));
    assert!(encoded.contains("\"plan_safe_refactor\""));
    assert!(!encoded.contains("\"name\":\"verify_change_readiness\""));
    assert!(!encoded.contains("\"name\":\"get_capabilities\""));
    assert!(!encoded.contains("\"name\":\"get_current_config\""));
    assert!(!encoded.contains("\"name\":\"set_preset\""));
}

#[test]
fn deferred_tools_list_resource_summary_matches_router_contract_fields() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let session_params = json!({
        "_session_id": "parity-summary",
        "_session_client_name": "CodexHarness",
        "_session_deferred_tool_loading": true,
    });

    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1401)),
            method: "tools/list".to_owned(),
            params: Some(session_params.clone()),
        },
    )
    .expect("tools/list response");
    let list_payload = result_payload(&list_response);

    let resource_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1402)),
            method: "resources/read".to_owned(),
            params: Some(json!({
                "uri": "codelens://tools/list",
                "_session_id": "parity-summary",
                "_session_client_name": "CodexHarness",
                "_session_deferred_tool_loading": true,
            })),
        },
    )
    .expect("resource response");
    let resource_payload = resource_payload(&resource_response);

    assert_fields_match(
        &list_payload,
        &resource_payload,
        &[
            "client_profile",
            "active_surface",
            "default_contract_mode",
            "tool_count",
            "tool_count_total",
            "preferred_namespaces",
            "preferred_tiers",
            "loaded_namespaces",
            "loaded_tiers",
            "effective_namespaces",
            "effective_tiers",
            "deferred_loading_active",
            "bootstrap_entrypoint",
            "list_role",
        ],
    );
    assert_eq!(resource_payload["bootstrap_entrypoint"], json!("prepare_harness_session"));
    assert_eq!(resource_payload["list_role"], json!("surface_expansion"));

    let list_names = tool_names_from_field(&list_payload, "tools");
    let recommended_names = tool_names_from_field(&resource_payload, "recommended_tools");
    assert_eq!(
        recommended_names,
        list_names.into_iter().take(recommended_names.len()).collect::<Vec<_>>()
    );
}

#[test]
fn full_tools_list_resource_details_match_router_contract_fields() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1411)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "_session_id": "parity-full",
                "_session_client_name": "CodexHarness",
                "_session_deferred_tool_loading": true,
                "_session_loaded_namespaces": ["lsp"],
                "_session_loaded_tiers": ["primitive"],
                "_session_full_tool_exposure": true,
                "full": true,
            })),
        },
    )
    .expect("tools/list response");
    let list_payload = result_payload(&list_response);

    let resource_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1412)),
            method: "resources/read".to_owned(),
            params: Some(json!({
                "uri": "codelens://tools/list/full",
                "_session_id": "parity-full",
                "_session_client_name": "CodexHarness",
                "_session_deferred_tool_loading": true,
                "_session_loaded_namespaces": ["lsp"],
                "_session_loaded_tiers": ["primitive"],
                "_session_full_tool_exposure": true,
            })),
        },
    )
    .expect("resource response");
    let resource_payload = resource_payload(&resource_response);

    assert_fields_match(
        &list_payload,
        &resource_payload,
        &[
            "client_profile",
            "active_surface",
            "default_contract_mode",
            "tool_count",
            "tool_count_total",
            "all_namespaces",
            "all_tiers",
            "preferred_namespaces",
            "preferred_tiers",
            "loaded_namespaces",
            "loaded_tiers",
            "effective_namespaces",
            "effective_tiers",
            "deferred_loading_active",
            "full_tool_exposure",
            "bootstrap_entrypoint",
            "list_role",
        ],
    );

    let list_names = tool_names_from_field(&list_payload, "tools");
    let resource_names = tool_names_from_field(&resource_payload, "tools");
    assert_eq!(list_names, resource_names);
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
    assert!(encoded.contains("\"orchestrationContract\""));
    assert!(!encoded.contains("\"outputSchema\""));
    assert!(!encoded.contains("\"annotations\""));
    assert!(!encoded.contains("\"visible_namespaces\""));

    let value = serde_json::to_value(&response).expect("serialize");
    let prepare = value["result"]["tools"]
        .as_array()
        .and_then(|tools| {
            tools.iter().find(|tool| {
                tool.get("name").and_then(|name| name.as_str()) == Some("prepare_harness_session")
            })
        })
        .expect("prepare_harness_session should be listed");
    assert_eq!(
        prepare["orchestrationContract"]["server_role"],
        json!("supporting_mcp")
    );
    assert_eq!(
        prepare["orchestrationContract"]["orchestration_owner"],
        json!("host")
    );
    assert_eq!(
        prepare["orchestrationContract"]["stage_hint"],
        json!("session_bootstrap")
    );
    assert!(
        prepare["orchestrationContract"]
            .get("preferred_client_behavior")
            .is_none()
    );
    assert!(
        prepare["orchestrationContract"]
            .get("retry_policy_owner")
            .is_none()
    );
    assert!(
        prepare["inputSchema"]["properties"]["profile"]
            .get("enum")
            .is_none()
    );
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
fn missing_param_error_exposes_protocol_diagnostics_in_error_data() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(9)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "set_profile",
                "arguments": {
                    "_session_client_name": "Claude Code",
                }
            })),
        },
    )
    .expect("tools/call should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    assert_eq!(value["error"]["code"], json!(-32602));
    assert_eq!(value["error"]["data"]["error_class"], json!("validation"));
    assert_eq!(value["error"]["data"]["tool_name"], json!("set_profile"));
    assert_eq!(
        value["error"]["data"]["request_stage"],
        json!("tool_arguments")
    );
    assert!(value["error"]["data"].get("orchestration_contract").is_none());
    assert!(value["error"]["data"].get("recommended_next_steps").is_none());
    assert!(value["error"]["data"].get("recovery_actions").is_none());
}

#[test]
fn invalid_jsonrpc_version_exposes_router_protocol_diagnostics() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "1.0".to_owned(),
            id: Some(json!(10)),
            method: "initialize".to_owned(),
            params: Some(json!({
                "clientInfo": {
                    "name": "Claude Code",
                    "version": "1.0.0"
                },
                "profile": "reviewer-graph"
            })),
        },
    )
    .expect("invalid jsonrpc version should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    assert_eq!(value["error"]["code"], json!(-32600));
    assert_eq!(value["error"]["data"]["error_scope"], json!("router"));
    assert_eq!(
        value["error"]["data"]["request_stage"],
        json!("jsonrpc_envelope")
    );
    assert_eq!(
        value["error"]["data"]["orchestration_contract"]["host_id"],
        json!("claude-code")
    );
    assert_eq!(
        value["error"]["data"]["orchestration_contract"]["active_surface"],
        json!("reviewer-graph")
    );
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
fn profile_input_schemas_include_workflow_first() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "includeOutputSchema": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    for tool_name in ["set_profile", "prepare_harness_session"] {
        let tool = value["result"]["tools"]
            .as_array()
            .and_then(|tools| {
                tools
                    .iter()
                    .find(|tool| tool.get("name").and_then(|name| name.as_str()) == Some(tool_name))
            })
            .unwrap_or_else(|| panic!("{tool_name} should be listed"));
        let variants = tool["inputSchema"]["properties"]["profile"]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{tool_name} profile enum should be present"));
        assert!(
            variants.iter().any(|item| item == "workflow-first"),
            "{tool_name} should advertise workflow-first"
        );
    }
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
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("blocked by daemon mode")
    );
    assert!(
        payload["recovery_actions"]
            .as_array()
            .map(|items| {
                items.iter().any(|item| {
                    item["kind"] == json!("tool_call")
                        && item["target"] == json!("prepare_harness_session")
                })
            })
            .unwrap_or(false)
    );
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
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("not available in active surface")
    );
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
    assert!(
        payload["data"]
            .get("recent_failure_window_seconds")
            .is_some()
    );
}

#[test]
fn routine_structured_responses_omit_low_signal_budget_hint() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert!(payload.get("budget_hint").is_none());
    assert!(payload.get("suggested_next_tools").is_none());
    assert!(payload.get("suggestion_reasons").is_none());
    assert!(payload.get("orchestration_contract").is_none());
    assert!(payload.get("recommended_next_steps").is_none());
}

#[test]
fn generic_validation_errors_omit_orchestration_contract() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let payload = call_tool(
        &state,
        "set_profile",
        json!({"profile": "not-a-real-profile"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert_eq!(payload["routing_hint"], json!("sync"));
    assert!(payload.get("budget_hint").is_none());
    assert!(payload.get("suggested_next_tools").is_none());
    assert!(payload.get("suggestion_reasons").is_none());
    assert!(payload.get("orchestration_contract").is_none());
    assert!(payload.get("recommended_next_steps").is_none());
    assert!(payload.get("recovery_actions").is_none());
}

#[test]
fn deferred_hidden_tool_errors_keep_recovery_contract() {
    let project = project_root();
    let state = crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let file_path = project.as_path().join("deferred_hidden.py");
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "beta",
            "file_path": file_path.display().to_string(),
            "include_body": false,
            "_session_id": "deferred-hidden",
            "_session_deferred_tool_loading": true,
            "_session_client_name": "Claude Code",
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("hidden by deferred loading")
    );
    assert_eq!(
        payload["orchestration_contract"]["host_id"],
        json!("claude-code")
    );
    assert_eq!(
        payload["orchestration_contract"]["active_surface"],
        json!("reviewer-graph")
    );
    assert!(
        payload["recovery_actions"]
            .as_array()
            .map(|items| {
                items.iter().any(|item| {
                    item["kind"] == json!("rpc_call")
                        && item["target"] == json!("tools/list")
                        && item["arguments"]["tier"] == json!("primitive")
                })
            })
            .unwrap_or(false)
    );
    assert!(
        payload["recommended_next_steps"]
            .as_array()
            .map(|items| items
                .iter()
                .any(|item| item["target"] == json!("host_orchestrator")))
            .unwrap_or(false)
    );
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
