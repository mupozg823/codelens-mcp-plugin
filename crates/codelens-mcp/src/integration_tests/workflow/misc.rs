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
fn trace_request_path_honors_path_scope() {
    let project = project_root();
    fs::write(
        project.as_path().join("selected.py"),
        "def target():\n    pass\n\ndef selected_caller():\n    target()\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("other.py"),
        "def target():\n    pass\n\ndef other_caller():\n    target()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "target",
            "path": "selected.py",
            "max_depth": 1,
            "max_results": 20
        }),
    );

    assert_eq!(payload["success"], json!(true));
    let callers = payload["data"]["callers"]
        .as_array()
        .expect("trace callers should be an array");
    assert!(
        callers
            .iter()
            .any(|caller| caller["name"] == "selected_caller"),
        "expected selected.py caller, got {payload}"
    );
    assert!(
        callers
            .iter()
            .all(|caller| caller["name"] != "other_caller"),
        "path scope leaked callers from another file: {payload}"
    );
}

#[test]
fn trace_request_path_honors_max_depth() {
    let project = project_root();
    fs::write(
        project.as_path().join("flow.py"),
        "def leaf():\n    return 1\n\ndef middle():\n    return leaf()\n\ndef entry():\n    return middle()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let depth_one = call_tool(
        &state,
        "trace_request_path",
        json!({"function_name": "entry", "path": "flow.py", "max_depth": 1}),
    );
    let depth_two = call_tool(
        &state,
        "trace_request_path",
        json!({"function_name": "entry", "path": "flow.py", "max_depth": 2}),
    );

    let depth_one_callees = depth_one["data"]["callees"]
        .as_array()
        .expect("depth-one callees should be an array");
    assert!(
        depth_one_callees
            .iter()
            .any(|callee| callee["name"] == "middle"),
        "expected direct callee at depth one: {depth_one}"
    );
    assert!(
        depth_one_callees
            .iter()
            .all(|callee| callee["name"] != "leaf"),
        "depth one should not include the transitive callee: {depth_one}"
    );

    let depth_two_callees = depth_two["data"]["callees"]
        .as_array()
        .expect("depth-two callees should be an array");
    let leaf = depth_two_callees
        .iter()
        .find(|callee| callee["name"] == "leaf")
        .unwrap_or_else(|| panic!("expected transitive callee at depth two: {depth_two}"));
    assert_eq!(leaf["depth"], json!(2));
}

#[test]
fn trace_request_path_cycle_terminates_without_returning_the_root() {
    let project = project_root();
    fs::write(
        project.as_path().join("cycle.py"),
        "def alpha():\n    return beta()\n\ndef beta():\n    return alpha()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "alpha",
            "path": "cycle.py",
            "max_depth": 8,
            "max_results": 0
        }),
    );

    assert_eq!(payload["success"], json!(true));
    for side in ["callers", "callees"] {
        let entries = payload["data"][side]
            .as_array()
            .unwrap_or_else(|| panic!("{side} must be an array: {payload}"));
        assert!(
            entries.iter().all(|entry| entry["name"] != "alpha"),
            "cycle must not return the trace root on {side}: {payload}"
        );
        assert_eq!(entries.len(), 1, "cycle should expose beta once: {payload}");
        assert_eq!(entries[0]["name"], json!("beta"));
        assert_eq!(entries[0]["depth"], json!(1));
    }
}

#[test]
fn trace_request_path_defines_zero_depth_and_per_side_result_caps() {
    let project = project_root();
    fs::write(
        project.as_path().join("caps.py"),
        "def left():\n    return 1\n\ndef right():\n    return 2\n\ndef root():\n    return left() + right()\n\ndef caller_one():\n    return root()\n\ndef caller_two():\n    return root()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let zero_depth = call_tool(
        &state,
        "trace_request_path",
        json!({"function_name": "root", "path": "caps.py", "max_depth": 0}),
    );
    assert_eq!(zero_depth["data"]["caller_count"], json!(0));
    assert_eq!(zero_depth["data"]["callee_count"], json!(0));

    let capped = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "root",
            "path": "caps.py",
            "max_depth": 1,
            "max_results": 1
        }),
    );
    assert_eq!(capped["data"]["caller_count"], json!(1));
    assert_eq!(capped["data"]["callee_count"], json!(1));

    let unlimited = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "root",
            "path": "caps.py",
            "max_depth": 1,
            "max_results": 0
        }),
    );
    assert_eq!(unlimited["data"]["caller_count"], json!(2));
    assert_eq!(unlimited["data"]["callee_count"], json!(2));
}

#[test]
fn trace_request_path_callers_keep_minimum_depth_and_file_identity() {
    let project = project_root();
    let pkg = project.as_path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("a.py"),
        "def target():\n    return 1\n\ndef repeated():\n    target()\n    return target()\n",
    )
    .unwrap();
    fs::write(
        pkg.join("b.py"),
        "def repeated():\n    return target()\n\ndef outer():\n    return repeated()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "target",
            "path": "pkg",
            "max_depth": 2,
            "max_results": 0
        }),
    );
    let callers = payload["data"]["callers"]
        .as_array()
        .expect("callers array");
    let repeated = callers
        .iter()
        .filter(|entry| entry["name"] == "repeated")
        .collect::<Vec<_>>();
    assert_eq!(
        repeated.len(),
        2,
        "same symbol name in different files must remain distinct: {payload}"
    );
    assert!(
        repeated.iter().all(|entry| entry["depth"] == 1),
        "direct callers must retain minimum depth: {payload}"
    );
    let outer = callers
        .iter()
        .find(|entry| entry["name"] == "outer")
        .unwrap_or_else(|| panic!("transitive caller missing: {payload}"));
    assert_eq!(outer["depth"], json!(2));
}

#[test]
fn trace_request_path_keeps_duplicate_name_transitive_nodes_in_their_definition_file() {
    let project = project_root();
    let pkg = project.as_path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("selected.py"),
        "def entry():\n    return duplicate()\n\ndef duplicate():\n    return selected_leaf()\n\ndef selected_leaf():\n    return 1\n\ndef selected_upstream():\n    return duplicate()\n",
    )
    .unwrap();
    fs::write(
        pkg.join("other.py"),
        "def duplicate():\n    return unrelated_leaf()\n\ndef unrelated_leaf():\n    return 2\n\ndef other_upstream():\n    return duplicate()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let caller_payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "selected_leaf",
            "path": "pkg",
            "max_depth": 2,
            "max_results": 0
        }),
    );
    assert_eq!(caller_payload["success"], json!(true));
    let callers = caller_payload["data"]["callers"]
        .as_array()
        .expect("callers array");
    assert!(
        callers
            .iter()
            .any(|entry| entry["name"] == "duplicate" && entry["file"] == "pkg/selected.py"),
        "selected duplicate should be a direct caller: {caller_payload}"
    );
    assert!(
        callers.iter().any(|entry| entry["name"] == "entry"),
        "entry should remain reachable through selected.py duplicate: {caller_payload}"
    );
    assert!(
        callers
            .iter()
            .all(|entry| entry["name"] != "other_upstream"),
        "unrelated duplicate definition leaked into caller traversal: {caller_payload}"
    );

    let callee_payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "entry",
            "path": "pkg",
            "max_depth": 2,
            "max_results": 0
        }),
    );
    assert_eq!(callee_payload["success"], json!(true));
    let callees = callee_payload["data"]["callees"]
        .as_array()
        .expect("callees array");
    assert!(
        callees
            .iter()
            .any(|entry| entry["name"] == "duplicate" && entry["file"] == "pkg/selected.py"),
        "selected duplicate should be a direct callee: {callee_payload}"
    );
    assert!(
        callees.iter().any(|entry| entry["name"] == "selected_leaf"),
        "selected leaf should remain reachable through selected.py duplicate: {callee_payload}"
    );
    assert!(
        callees
            .iter()
            .all(|entry| entry["name"] != "unrelated_leaf"),
        "unrelated duplicate definition leaked into callee traversal: {callee_payload}"
    );
}

#[test]
fn trace_request_path_callers_keep_external_callers_for_the_resolved_definition() {
    let project = project_root();
    let pkg = project.as_path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("selected.ts"),
        "export function leaf() { return 1; }\nexport function middle() { return leaf(); }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("other-target.ts"),
        "export function middle() { return 2; }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("a-local.ts"),
        "import { middle as localMiddle } from './other-target';\nexport function local_upstream() { return localMiddle(); }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("index.ts"),
        "export { middle } from './selected';\n",
    )
    .unwrap();
    fs::write(
        pkg.join("z-page.ts"),
        "import { middle as onMiddle } from './index';\nexport function external_upstream() { return onMiddle(); }\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "leaf",
            "path": "pkg",
            "max_depth": 2,
            "max_results": 0
        }),
    );

    assert_eq!(payload["success"], json!(true));
    let callers = payload["data"]["callers"]
        .as_array()
        .expect("callers array");
    assert!(
        callers
            .iter()
            .any(|entry| entry["name"] == "external_upstream"),
        "aliased external caller of selected.ts::middle should remain reachable: {payload}"
    );
    assert!(
        callers
            .iter()
            .all(|entry| entry["name"] != "local_upstream"),
        "same-named middle definition leaked its local caller: {payload}"
    );

    let capped = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "leaf",
            "path": "pkg",
            "max_depth": 2,
            "max_results": 2
        }),
    );
    let capped_callers = capped["data"]["callers"]
        .as_array()
        .expect("capped callers array");
    assert_eq!(
        capped_callers.len(),
        2,
        "per-side caller cap should hold: {capped}"
    );
    assert!(
        capped_callers
            .iter()
            .any(|entry| entry["name"] == "external_upstream"),
        "identity filtering must happen before the caller cap: {capped}"
    );
}

#[test]
fn trace_request_path_applies_caps_after_root_and_seen_filters() {
    let project = project_root();
    fs::write(
        project.as_path().join("cycle_cap.py"),
        "def other():\n    return 1\n\ndef root():\n    root()\n    root()\n    return other()\n\ndef caller():\n    root()\n    return root()\n\ndef later():\n    return root()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let cap_one = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "root",
            "path": "cycle_cap.py",
            "max_depth": 1,
            "max_results": 1
        }),
    );
    assert_eq!(cap_one["data"]["caller_count"], json!(1));
    assert_eq!(cap_one["data"]["callers"][0]["name"], json!("caller"));
    assert_eq!(cap_one["data"]["callee_count"], json!(1));
    assert_eq!(cap_one["data"]["callees"][0]["name"], json!("other"));

    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "root",
            "path": "cycle_cap.py",
            "max_depth": 1,
            "max_results": 2
        }),
    );

    assert_eq!(payload["data"]["caller_count"], json!(2));
    let caller_names = payload["data"]["callers"]
        .as_array()
        .expect("callers array")
        .iter()
        .map(|caller| caller["name"].as_str().expect("caller name"))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        caller_names,
        std::collections::HashSet::from(["caller", "later"])
    );
    assert_eq!(payload["data"]["callee_count"], json!(1));
    assert_eq!(payload["data"]["callees"][0]["name"], json!("other"));
}

#[test]
fn trace_request_path_uses_canonical_callee_identity_after_an_alias_hop() {
    let project = project_root();
    let pkg = project.as_path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("selected.ts"),
        "export function leaf() { return 1; }\nexport function handleSubmit() { return leaf(); }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("index.ts"),
        "export { handleSubmit } from './selected';\n",
    )
    .unwrap();
    fs::write(
        pkg.join("page.ts"),
        "import { handleSubmit as onSubmit } from './index';\nexport function entry() { return onSubmit(); }\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "entry",
            "path": "pkg",
            "max_depth": 2,
            "max_results": 0
        }),
    );
    let callees = payload["data"]["callees"]
        .as_array()
        .expect("callees array");
    assert!(
        callees
            .iter()
            .any(|callee| callee["name"] == "onSubmit" && callee["depth"] == 1),
        "public output must retain the raw alias: {payload}"
    );
    assert!(
        callees
            .iter()
            .any(|callee| callee["name"] == "leaf" && callee["depth"] == 2),
        "canonical target identity must drive the second hop: {payload}"
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
fn review_architecture_with_diagram_keeps_boundary_report() {
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
        json!("module_boundary_report")
    );
    let sections = payload["data"]["available_sections"]
        .as_array()
        .expect("architecture report should expose section handles");
    assert!(sections.iter().any(|section| section == "diagram"));
}

#[test]
fn review_architecture_diagram_flag_preserves_structural_contract() {
    let project = project_root();
    fs::write(project.as_path().join("mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);

    let without_diagram = call_tool(
        &state,
        "review_architecture",
        json!({"path": "mod.py", "include_diagram": false}),
    );
    let with_diagram = call_tool(
        &state,
        "review_architecture",
        json!({"path": "mod.py", "include_diagram": true}),
    );

    assert_eq!(
        with_diagram["data"]["top_findings"],
        without_diagram["data"]["top_findings"]
    );
    assert_eq!(
        with_diagram["data"]["risk_level"],
        without_diagram["data"]["risk_level"]
    );
    assert_eq!(
        with_diagram["data"]["readiness"],
        without_diagram["data"]["readiness"]
    );
    assert_eq!(
        with_diagram["data"]["analysis_completeness"],
        without_diagram["data"]["analysis_completeness"]
    );
    let without_sections = without_diagram["data"]["available_sections"]
        .as_array()
        .expect("plain architecture report sections");
    let with_sections = with_diagram["data"]["available_sections"]
        .as_array()
        .expect("diagram architecture report sections");
    assert!(!without_sections.iter().any(|section| section == "diagram"));
    assert!(with_sections.iter().any(|section| section == "diagram"));
}

#[test]
fn review_architecture_zero_findings_are_not_high_risk() {
    let project = project_root();
    fs::write(project.as_path().join("mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(&state, "review_architecture", json!({"path": "mod.py"}));

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["risk_level"], json!("low"));
}

#[cfg(feature = "semantic")]
#[test]
fn review_architecture_without_a_semantic_index_stays_lazy() {
    let project = project_root();
    fs::write(project.as_path().join("mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);
    assert!(state.embedding_ref().is_none());

    let payload = call_tool(&state, "review_architecture", json!({"path": "mod.py"}));

    assert_eq!(payload["success"], json!(true));
    assert!(
        state.embedding_ref().is_none(),
        "architecture review must not initialize an embedding engine when no semantic index exists"
    );
}

#[cfg(feature = "semantic")]
#[test]
fn dead_code_report_without_a_semantic_index_stays_lazy() {
    let project = project_root();
    fs::write(project.as_path().join("mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);
    assert!(state.embedding_ref().is_none());

    let payload = call_tool(
        &state,
        "dead_code_report",
        json!({"scope": "mod.py", "max_results": 5}),
    );

    assert_eq!(payload["success"], json!(true));
    assert!(
        state.embedding_ref().is_none(),
        "dead-code review must not initialize an embedding engine when no semantic index exists"
    );
}

#[test]
fn review_architecture_rejects_a_missing_scope_instead_of_claiming_complete() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "review_architecture",
        json!({"path": "missing/module.py"}),
    );

    assert_eq!(payload["success"], json!(false));
    assert!(
        !payload.to_string().contains("\"status\":\"complete\""),
        "missing analysis evidence must not be reported as complete: {payload}"
    );
}

#[test]
fn review_architecture_rejects_an_unsupported_file_scope() {
    let project = project_root();
    fs::write(project.as_path().join("notes.txt"), "not a source module\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(&state, "review_architecture", json!({"path": "notes.txt"}));

    assert_eq!(payload["success"], json!(false));
    assert!(
        payload.to_string().contains("does not support"),
        "unsupported scope should return a capability error: {payload}"
    );
}

#[test]
fn review_architecture_directory_is_not_a_touched_file() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("pkg")).unwrap();
    fs::write(project.as_path().join("pkg/mod.py"), "class A: pass\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(&state, "review_architecture", json!({"path": "pkg"}));

    let diagnostic_check = payload["data"]["verifier_checks"]
        .as_array()
        .and_then(|checks| {
            checks
                .iter()
                .find(|check| check["check"] == "diagnostic_verifier")
        })
        .expect("diagnostic verifier check");
    assert!(
        diagnostic_check["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("No touched files")),
        "directory scope must not masquerade as one touched file: {payload}"
    );
}

#[test]
fn review_architecture_directory_limit_marks_analysis_partial() {
    let project = project_root();
    let pkg = project.as_path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    for index in 0..3 {
        fs::write(
            pkg.join(format!("module_{index}.py")),
            format!("def function_{index}():\n    return {index}\n"),
        )
        .unwrap();
    }
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "review_architecture",
        json!({"path": "pkg", "_test_directory_file_limit": 2}),
    );

    assert_eq!(
        payload["data"]["analysis_completeness"]["status"],
        json!("partial")
    );
    assert_eq!(
        payload["data"]["analysis_completeness"]["in_scope_file_limit_hit"],
        json!(true)
    );
    assert_eq!(
        payload["data"]["analysis_completeness"]["in_scope_file_count"],
        json!(3)
    );
    assert_ne!(
        payload["data"]["readiness"]["mutation_ready"],
        json!("ready")
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
        json!("module_boundary_report")
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
        json!({"analysis_id": analysis_id, "section": "impact"}),
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

#[test]
fn trace_run_analysis_job_depth_two_excludes_unrelated_rust_new_owners() {
    // Given: the direct caller is `AnalysisQueue::new`, while other files call
    // homonymous constructors owned by unrelated Rust types.
    let project = project_root();
    fs::write(
        project.as_path().join("runners.rs"),
        "pub fn run_analysis_job_from_queue() {}\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("analysis_queue.rs"),
        "use crate::runners::run_analysis_job_from_queue;\npub struct AnalysisQueue;\nimpl AnalysisQueue { pub fn new() -> Self { run_analysis_job_from_queue(); Self } }\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("bootstrap.rs"),
        "use crate::analysis_queue::AnalysisQueue;\npub fn start_queue() { let _ = AnalysisQueue::new(); }\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("main.rs"),
        "pub struct EnvFilter;\nimpl EnvFilter { pub fn new() -> Self { Self } }\npub struct Arc;\nimpl Arc { pub fn new() -> Self { Self } }\npub fn configured_log_filter() { let _ = EnvFilter::new(); }\npub fn main() { let _ = Arc::new(); configured_log_filter(); }\n",
    )
    .unwrap();
    let state = make_state(&project);
    state
        .symbol_index()
        .refresh_all()
        .expect("refresh fixture index");

    // When: the public trace workflow expands callers through depth two.
    let payload = call_tool(
        &state,
        "trace_request_path",
        json!({
            "function_name": "run_analysis_job_from_queue",
            "max_depth": 2,
            "max_results": 0
        }),
    );
    let callers = payload["data"]["callers"]
        .as_array()
        .unwrap_or_else(|| panic!("callers must be an array: {payload}"));

    // Then: the true owner-qualified upstream survives, while basename-only
    // EnvFilter::new and Arc::new branches do not enter the traversal.
    assert!(
        callers
            .iter()
            .any(|entry| entry["name"] == "new" && entry["depth"] == 1),
        "direct AnalysisQueue::new caller missing: {payload}"
    );
    assert!(
        callers
            .iter()
            .any(|entry| entry["name"] == "start_queue" && entry["depth"] == 2),
        "owner-qualified upstream missing: {payload}"
    );
    assert!(
        callers
            .iter()
            .all(|entry| { entry["name"] != "configured_log_filter" && entry["name"] != "main" }),
        "unrelated constructor owners leaked into depth two: {payload}"
    );
}
