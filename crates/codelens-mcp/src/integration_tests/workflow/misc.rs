use super::*;

#[test]
fn low_level_chain_emits_composite_guidance_and_tracks_followthrough() {
    let project = project_root();
    fs::write(
        project.as_path().join("guided.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

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
    let response = call_tool(
        &state,
        "read_file",
        json!({"relative_path": "guided.py", "_harness_phase": "review"}),
    );
    let suggested = response["suggested_next_tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let suggested_names = suggested
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(!suggested_names.is_empty(), "expected composite guidance");
    assert!(
        suggested_names
            .iter()
            .all(|tool| crate::tools::REVIEW_PHASE_TOOLS.contains(tool)),
        "review phase leaked rejected suggestions: {suggested_names:?}"
    );
    assert!(
        !suggested_names.contains(&"delegate_to_codex_builder"),
        "successful read-only chains must not synthesize model-specific delegation: {response}"
    );
    assert!(
        !response.to_string().to_ascii_lowercase().contains("codex"),
        "successful read-only guidance must stay host-neutral: {response}"
    );
    let budget_hint = response["budget_hint"].as_str().unwrap_or_default();
    assert!(budget_hint.contains("Repeated low-level chain detected"));

    let _ = call_tool(
        &state,
        "review_changes",
        json!({"path": "guided.py", "_harness_phase": "review"}),
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
fn safe_rename_report_emits_host_neutral_mutation_intent() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_delegate.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    // No profile restriction — safe_rename_report is deprecated and not
    // in any lean profile; test it with the full surface.
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
    assert!(suggested.iter().any(|value| value == "rename_symbol"));
    assert!(
        suggested
            .iter()
            .all(|value| value != "delegate_to_codex_builder"),
        "successful read-only response must expose mutation intent directly: {payload}"
    );

    let mutation_call = payload["suggested_next_calls"]
        .as_array()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.get("tool").and_then(|value| value.as_str()) == Some("rename_symbol")
            })
        })
        .cloned()
        .expect("rename_symbol mutation intent should include concrete arguments");

    assert_eq!(
        mutation_call["arguments"]["file_path"],
        json!("rename_delegate.py")
    );
    assert_eq!(mutation_call["arguments"]["symbol_name"], json!("old_name"));
    assert_eq!(mutation_call["arguments"]["new_name"], json!("new_name"));
    assert_eq!(mutation_call["arguments"]["dry_run"], json!(true));
    assert!(
        !payload.to_string().to_ascii_lowercase().contains("codex"),
        "mutation intent must not name a model or vendor: {payload}"
    );
}

#[test]
fn explicit_harness_phases_never_emit_rejected_suggestions() {
    let project = project_root();
    fs::write(
        project.as_path().join("phase_guidance.py"),
        "def phase_target():\n    return 1\n",
    )
    .unwrap();

    for (phase, allowed) in [
        ("plan", crate::tools::PLAN_PHASE_TOOLS),
        ("build", crate::tools::BUILD_PHASE_TOOLS),
        ("review", crate::tools::REVIEW_PHASE_TOOLS),
        ("eval", crate::tools::EVAL_PHASE_TOOLS),
    ] {
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "find_symbol",
            json!({
                "name": "phase_target",
                "file_path": "phase_guidance.py",
                "include_body": false,
                "_harness_phase": phase,
            }),
        );
        assert_eq!(payload["success"], json!(true));
        let suggestions = payload["suggested_next_tools"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            suggestions.iter().all(|tool| {
                tool.as_str()
                    .map(|name| allowed.contains(&name))
                    .unwrap_or(false)
            }),
            "{phase} phase leaked rejected output suggestions: {payload}"
        );
    }
}

#[test]
fn metrics_report_does_not_recommend_another_report() {
    let project = project_root();
    let state = make_state(&project);
    let _ = call_tool(&state, "get_current_config", json!({}));

    let payload = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(payload["success"], json!(true));
    let suggestions = payload["suggested_next_tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let report_tools = [
        "get_tool_metrics",
        "audit_builder_session",
        "audit_planner_session",
        "export_session_markdown",
    ];
    assert!(
        suggestions.iter().all(|tool| {
            tool.as_str()
                .map(|name| !report_tools.contains(&name))
                .unwrap_or(false)
        }),
        "a completed metrics report must not immediately recommend another report: {payload}"
    );
}

#[test]
fn repeated_builder_error_keeps_recovery_host_neutral() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_loop.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "rename old_name in rename_loop.py",
            "changed_files": ["rename_loop.py"]
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
            .all(|value| value != "delegate_to_codex_builder"),
        "error recovery must not synthesize a model-specific handoff: {payload}"
    );
    assert!(
        !payload.to_string().to_ascii_lowercase().contains("codex"),
        "error recovery must be expressed as deterministic tool guidance: {payload}"
    );
}

#[test]
fn stale_preflight_is_rejected() {
    let project = project_root();
    fs::write(project.as_path().join("stale_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));

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
        "replace_symbol_body",
        json!({
            "relative_path": "stale_gate.py",
            "symbol_name": "alpha",
            "new_body": "    return 2"
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"].as_str().unwrap_or("").contains("stale")
            || payload["error"]
                .as_str()
                .unwrap_or("")
                .contains("preflight")
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["stale_preflight_reject_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[cfg(feature = "semantic")]
#[test]
fn symbol_mutation_reindexes_existing_embedding_index_when_engine_is_not_loaded() {
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
        "insert_after_symbol",
        json!({
            "relative_path": "semantic_mutation.py",
            "symbol_name": "winter_orbit_launch",
            "content": "\ndef ember_archive_delta():\n    return 2\n"
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
    assert!(
        search["data"]["results"]
            .as_array()
            .map(|results| {
                results
                    .iter()
                    .all(|result| result["provenance"]["source"] == json!("semantic"))
                    && results
                        .iter()
                        .all(|result| result["provenance"]["adjusted_score"].is_number())
                    && results.iter().any(|result| {
                        result.get("symbol_name") == Some(&json!("ember_archive_delta"))
                    })
            })
            .unwrap_or(false)
    );
}

// ── Workflow alias success-contract tests ──────────────────────────────────────────

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

// ── Workflow tool coverage tests ─────────────────────────────────────────

#[test]
fn explore_codebase_with_query_delegates_to_ranked_context() {
    let project = project_root();
    fs::write(project.as_path().join("hello.py"), "def hello(): pass\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "explore_codebase", json!({"query": "hello"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("explore_codebase"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("get_ranked_context")
    );
}

#[test]
fn trace_request_path_delegates_to_call_graph_flow() {
    let project = project_root();
    fs::write(
        project.as_path().join("flow.py"),
        "def alpha(): pass\ndef beta(): alpha()\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({"function_name": "beta"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("trace_request_path"));
    assert_eq!(payload["data"]["delegated_tool"], json!("call_graph_flow"));
}

#[test]
fn review_architecture_with_path_uses_boundary_report() {
    let project = project_root();
    fs::write(project.as_path().join("mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "review_architecture", json!({"path": "mod.py"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("review_architecture"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("module_boundary_report")
    );
}

#[test]
fn review_architecture_with_diagram_uses_mermaid() {
    let project = project_root();
    fs::write(project.as_path().join("mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "review_architecture",
        json!({"path": "mod.py", "include_diagram": true}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("review_architecture"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("mermaid_module_graph")
    );
}

#[test]
fn review_architecture_directory_diagram_aggregates_import_edges() {
    let project = project_root();
    let pkg = project.as_path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("__init__.py"), "").unwrap();
    fs::write(pkg.join("b.py"), "class B:\n    pass\n").unwrap();
    fs::write(
        pkg.join("a.py"),
        "from .b import B\n\nclass A:\n    value = B()\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("consumer.py"),
        "from pkg.a import A\n\nvalue = A()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "review_architecture",
        json!({"path": "pkg", "include_diagram": true, "max_nodes": 8}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("mermaid_module_graph")
    );
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id should be present");

    let stats = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "stats"}),
    );
    assert_eq!(stats["success"], json!(true));
    assert_eq!(stats["data"]["content"]["scope_kind"], json!("directory"));
    assert!(
        stats["data"]["content"]["upstream_total"]
            .as_u64()
            .unwrap_or_default()
            >= 1,
        "directory architecture review should include importers of files inside the directory: {stats:?}"
    );

    let raw = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "raw_impact"}),
    );
    assert_eq!(raw["success"], json!(true));
    assert!(
        raw["data"]["content"]["in_scope_file_count"]
            .as_u64()
            .unwrap_or_default()
            >= 3,
        "directory impact should report scoped files: {raw:?}"
    );
}
