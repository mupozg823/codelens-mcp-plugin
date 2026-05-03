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
fn trace_request_path_delegates_to_explain_flow() {
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
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("explain_code_flow")
    );
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
fn review_architecture_directory_diagram_reports_scope_evidence() {
    let project = project_root();
    let src = project.as_path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("a.ts"),
        "import { b } from './b'\nexport const a = b\n",
    )
    .unwrap();
    fs::write(src.join("b.ts"), "export const b = 1\n").unwrap();
    fs::write(
        project.as_path().join("outside.ts"),
        "import { a } from './src/a'\nconsole.log(a)\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "review_architecture",
        json!({
            "path": src.to_string_lossy(),
            "include_diagram": true,
            "max_nodes": 10,
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("review_architecture"));
    assert_eq!(
        payload["data"]["delegated_tool"],
        json!("mermaid_module_graph")
    );
    let finding = payload["data"]["top_findings"][0]
        .as_str()
        .expect("top finding");
    assert!(
        finding.contains("2 files"),
        "directory report should summarize scoped files: {finding}"
    );
    assert!(
        finding.contains("external importers"),
        "directory report should include boundary evidence: {finding}"
    );
}
