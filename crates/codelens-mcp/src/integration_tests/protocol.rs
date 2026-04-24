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
    let value = serde_json::to_value(&response).expect("serialize");
    assert_eq!(value["result"]["active_surface"], json!("preset:full"));
    let names = value["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"get_ranked_context"));
    assert!(names.contains(&"get_callers"));
    assert!(names.contains(&"start_analysis_job"));
    assert!(
        !names.contains(&"get_symbols_overview"),
        "default tools/list should stay MVP-focused; use full=true or namespace filters for primitive expansion"
    );
}

#[test]
fn default_tools_list_is_mvp_focused_but_full_and_namespace_expand() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let default_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(10)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .expect("default tools/list");
    let default_value = serde_json::to_value(&default_resp).expect("serialize");
    let default_tools = default_value["result"]["tools"]
        .as_array()
        .expect("default tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        default_tools,
        vec![
            "activate_project",
            "prepare_harness_session",
            "get_current_config",
            "set_profile",
            "set_preset",
            "explore_codebase",
            "get_ranked_context",
            "get_callers",
            "get_callees",
            "verify_change_readiness",
            "start_analysis_job",
            "get_analysis_job",
            "get_analysis_section",
        ]
    );

    let full_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(11)),
            method: "tools/list".to_owned(),
            params: Some(json!({"full": true})),
        },
    )
    .expect("full tools/list");
    let full_value = serde_json::to_value(&full_resp).expect("serialize");
    let full_tools = full_value["result"]["tools"]
        .as_array()
        .expect("full tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(full_tools.contains(&"get_symbols_overview"));
    assert!(full_tools.len() > default_tools.len());

    let namespace_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(12)),
            method: "tools/list".to_owned(),
            params: Some(json!({"namespace": "symbols"})),
        },
    )
    .expect("namespace tools/list");
    let namespace_value = serde_json::to_value(&namespace_resp).expect("serialize");
    let namespace_tools = namespace_value["result"]["tools"]
        .as_array()
        .expect("namespace tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(namespace_tools.contains(&"get_symbols_overview"));
    assert!(namespace_tools.contains(&"find_symbol"));
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
fn tools_list_resource_summary_matches_default_tools_list_count() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(103)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let list_value = serde_json::to_value(&list_resp).unwrap();
    let list_count = list_value["result"]["tool_count"]
        .as_u64()
        .expect("tools/list tool_count");

    for (offset, uri) in ["codelens://tools/list"].into_iter().enumerate() {
        let resource_resp = handle_request(
            &state,
            crate::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(104 + offset)),
                method: "resources/read".to_owned(),
                params: Some(json!({"uri": uri})),
            },
        )
        .unwrap();
        let resource_value = serde_json::to_value(&resource_resp).unwrap();
        let resource_text = resource_value["result"]["contents"][0]["text"]
            .as_str()
            .expect("resource text");
        let resource_payload: serde_json::Value =
            serde_json::from_str(resource_text).expect("resource payload");
        let resource_count = resource_payload["tool_count"]
            .as_u64()
            .expect("resource tool_count");

        assert_eq!(
            resource_count, list_count,
            "default tools/list and {uri} should expose the same default visible count"
        );
    }

    let full_resource_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(105)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list/full"})),
        },
    )
    .unwrap();
    let full_resource_value = serde_json::to_value(&full_resource_resp).unwrap();
    let full_resource_text = full_resource_value["result"]["contents"][0]["text"]
        .as_str()
        .expect("full resource text");
    let full_resource_payload: serde_json::Value =
        serde_json::from_str(full_resource_text).expect("full resource payload");
    let full_resource_count = full_resource_payload["tool_count"]
        .as_u64()
        .expect("full resource tool_count");
    assert!(
        full_resource_count > list_count,
        "full tool resource should remain an expansion path"
    );
}

#[test]
fn graph_profiles_expose_call_graph_primitives_by_namespace() {
    for profile in ["reviewer-graph", "refactor-full", "ci-audit"] {
        let project = project_root();
        let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
        let _ = call_tool(&state, "set_profile", json!({"profile": profile}));

        let list_resp = handle_request(
            &state,
            crate::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(102)),
                method: "tools/list".to_owned(),
                params: Some(json!({"namespace": "graph"})),
            },
        )
        .unwrap();
        let encoded = serde_json::to_string(&list_resp).unwrap();
        assert!(
            encoded.contains("\"get_callers\""),
            "{profile} graph namespace should include get_callers"
        );
        assert!(
            encoded.contains("\"get_callees\""),
            "{profile} graph namespace should include get_callees"
        );
        assert!(!encoded.contains("\"find_symbol\""));
    }
}

#[test]
fn get_callers_input_schema_exposes_file_path_hint() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(106)),
            method: "tools/list".to_owned(),
            params: Some(json!({"namespace": "graph", "includeOutputSchema": true})),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&list_resp).unwrap();
    let tools = value["result"]["tools"].as_array().expect("tools");
    let callers = tools
        .iter()
        .find(|tool| tool["name"] == "get_callers")
        .expect("get_callers tool");
    assert!(
        callers["inputSchema"]["properties"]["file_path"].is_object(),
        "get_callers should expose optional file_path hint"
    );
    assert!(
        callers["outputSchema"]["properties"]["confidence_basis"].is_object(),
        "get_callers should expose call-graph output schema"
    );
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
    assert!(
        encoded
            .contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]")
    );
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
    assert!(encoded.contains("\"codelens/preferredExecutor\""));
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
fn tools_list_exposes_preferred_executor_per_tool() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "full": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    let tools = value["result"]["tools"]
        .as_array()
        .expect("tools/list tools array");
    let rename = tools
        .iter()
        .find(|tool| tool["name"] == "rename_symbol")
        .expect("rename_symbol present");
    let review = tools
        .iter()
        .find(|tool| tool["name"] == "review_changes")
        .expect("review_changes present");
    let symbol = tools
        .iter()
        .find(|tool| tool["name"] == "find_symbol")
        .expect("find_symbol present");

    assert_eq!(
        rename["_meta"]["codelens/preferredExecutor"],
        json!("codex-builder")
    );
    assert_eq!(
        review["_meta"]["codelens/preferredExecutor"],
        json!("claude")
    );
    assert_eq!(symbol["_meta"]["codelens/preferredExecutor"], json!("any"));
}

#[test]
fn tools_list_exposes_claude_toolsearch_meta_for_bootstrap_tools() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "full": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    let tools = value["result"]["tools"]
        .as_array()
        .expect("tools/list tools array");
    let bootstrap = tools
        .iter()
        .find(|tool| tool["name"] == "prepare_harness_session")
        .expect("prepare_harness_session present");
    let explore = tools
        .iter()
        .find(|tool| tool["name"] == "explore_codebase")
        .expect("explore_codebase present");
    let review = tools
        .iter()
        .find(|tool| tool["name"] == "review_changes")
        .expect("review_changes present");
    let symbol = tools
        .iter()
        .find(|tool| tool["name"] == "find_symbol")
        .expect("find_symbol present");

    assert_eq!(bootstrap["_meta"]["anthropic/alwaysLoad"], json!(true));
    assert_eq!(
        bootstrap["_meta"]["anthropic/searchHint"],
        json!("bootstrap CodeLens harness session")
    );
    assert_eq!(
        explore["_meta"]["anthropic/searchHint"],
        json!("explore codebase with compressed context")
    );
    assert_eq!(
        review["_meta"]["anthropic/searchHint"],
        json!("review changed files and risk")
    );
    assert!(symbol["_meta"].get("anthropic/alwaysLoad").is_none());
}

#[test]
fn tools_list_exposes_annotation_titles() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: Some(json!({
                "full": true,
            })),
        },
    )
    .expect("tools/list should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    let tools = value["result"]["tools"]
        .as_array()
        .expect("tools/list tools array");
    let bootstrap = tools
        .iter()
        .find(|tool| tool["name"] == "prepare_harness_session")
        .expect("prepare_harness_session present");
    let find_symbol = tools
        .iter()
        .find(|tool| tool["name"] == "find_symbol")
        .expect("find_symbol present");
    let lsp_recipe = tools
        .iter()
        .find(|tool| tool["name"] == "get_lsp_recipe")
        .expect("get_lsp_recipe present");
    let metrics = tools
        .iter()
        .find(|tool| tool["name"] == "get_tool_metrics")
        .expect("get_tool_metrics present");

    assert_eq!(
        bootstrap["annotations"]["title"],
        json!("Prepare Harness Session")
    );
    assert_eq!(find_symbol["annotations"]["title"], json!("Find Symbol"));
    assert_eq!(lsp_recipe["annotations"]["title"], json!("LSP Recipe"));
    assert_eq!(metrics["annotations"]["title"], json!("Tool Metrics"));
}

#[test]
fn tool_call_result_meta_exposes_preferred_executor() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "create_text_file",
                "arguments": {
                    "_session_id": default_session_id(&state),
                    "relative_path": "new_file.txt",
                    "content": "hello\n"
                }
            })),
        },
    )
    .expect("tools/call should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    assert_eq!(
        value["result"]["_meta"]["codelens/preferredExecutor"],
        json!("codex-builder")
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

#[test]
fn symbiote_uri_alias_matches_codelens_response() {
    // ADR-0007 Phase 2: clients can address resources under either
    // `codelens://` (canonical) or `symbiote://` (rebrand alias). Both
    // must resolve to the same payload without any dispatch difference.
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let codelens_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://project/overview"})),
        },
    )
    .expect("codelens:// uri must resolve");

    let symbiote_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "symbiote://project/overview"})),
        },
    )
    .expect("symbiote:// alias must resolve");

    let codelens_body = serde_json::to_value(&codelens_response)
        .unwrap()
        .get("result")
        .cloned()
        .unwrap_or_default();
    let symbiote_body = serde_json::to_value(&symbiote_response)
        .unwrap()
        .get("result")
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        codelens_body, symbiote_body,
        "symbiote:// alias must return the same resource payload as codelens://"
    );
}
