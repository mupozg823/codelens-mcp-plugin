use super::*;

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
    assert!(
        payload["data"]["diagnostics_guidance"]
            .get("recommended_action")
            .is_some()
    );
    assert!(
        payload["data"]["diagnostics_guidance"]
            .get("reason_code")
            .is_some()
    );
    assert!(payload["data"].get("embeddings_loaded").is_some());
    let expected_model = if cfg!(feature = "semantic") {
        json!("MiniLM-L12-CodeSearchNet-INT8")
    } else {
        json!("disabled")
    };
    assert_eq!(payload["data"]["embedding_model"], expected_model);
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
    assert!(
        payload["data"]["daemon_binary_drift"]
            .get("recommended_action")
            .is_some()
    );
    assert!(
        payload["data"]["daemon_binary_drift"]
            .get("reason_code")
            .is_some()
    );
}

#[cfg(feature = "semantic")]
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
