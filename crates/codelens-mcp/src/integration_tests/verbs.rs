use super::*;

// ── Verb facade tests (search / graph / review) ──────────────────────
//
// Phase-1 consolidation: read-only tool families are reachable behind
// mode-routed verbs. The verbs are additive facades — every absorbed
// tool ID stays registered and callable; the verb resolves `mode` to
// the target tool and delegates through the dispatch table.

#[test]
fn search_verb_symbol_mode_delegates_to_find_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_sample.py"),
        "def verb_target():\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "search",
        json!({ "mode": "symbol", "name": "verb_target" }),
    );

    assert_eq!(payload["success"], json!(true));
    let symbols = payload["data"]["symbols"]
        .as_array()
        .expect("symbols array from delegated find_symbol");
    assert!(
        symbols.iter().any(|s| s["name"] == json!("verb_target")),
        "delegated find_symbol must surface the seeded symbol"
    );
}

#[test]
fn graph_verb_callers_mode_delegates_to_get_callers() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_graph.py"),
        "def callee_fn():\n    pass\n\ndef caller_fn():\n    callee_fn()\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "graph",
        json!({ "mode": "callers", "function_name": "callee_fn" }),
    );

    assert_eq!(
        payload["success"],
        json!(true),
        "graph(mode=callers) must delegate to get_callers: {payload}"
    );
}

#[test]
fn search_verb_unknown_mode_lists_valid_modes() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(&state, "search", json!({ "mode": "bogus" }));

    assert_eq!(payload["success"], json!(false));
    let text = payload.to_string();
    assert!(
        text.contains("bogus"),
        "error must echo the bad mode: {text}"
    );
    assert!(
        text.contains("symbol") && text.contains("refs"),
        "error must list valid modes so the caller can self-correct: {text}"
    );
}

/// ADR-0016 (acceptance f): with get_callers/get_callees no longer *listed* on
/// the reviewer-graph surface, the `graph` verb façade and the P4 refs merge
/// must keep working — both route through the callability gate, which now
/// accepts registered-but-unlisted targets as hidden aliases.
#[test]
fn reviewer_graph_facade_and_refs_work_without_listed_call_graph_primitives() {
    let project = project_root();
    fs::write(
        project.as_path().join("reviewer_graph.py"),
        "def sink():\n    pass\n\ndef source():\n    sink()\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    // Precondition: the raw primitive is unlisted on this surface.
    assert!(
        !crate::tool_defs::is_tool_in_surface(
            "get_callers",
            crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::ReviewerGraph),
        ),
        "test premise: get_callers must be unlisted on reviewer-graph"
    );

    // graph(mode=callers) façade → resolves to the hidden get_callers target.
    let graph_payload = call_tool(
        &state,
        "graph",
        json!({ "mode": "callers", "function_name": "sink" }),
    );
    assert_eq!(
        graph_payload["success"],
        json!(true),
        "graph(mode=callers) must still delegate to get_callers on reviewer-graph: {graph_payload}"
    );

    // find_referencing_symbols (listed) with its P4 import_graph merge path.
    let refs_payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "symbol_name": "sink", "relative_path": "reviewer_graph.py" }),
    );
    assert_eq!(
        refs_payload["success"],
        json!(true),
        "find_referencing_symbols must keep working on reviewer-graph: {refs_payload}"
    );
}

// ── Phase-2 verbs (overview / diagnose / analyze) ────────────────────

#[test]
fn overview_verb_file_mode_delegates_to_symbols_overview() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_overview.py"),
        "class Omega:\n    def method(self):\n        pass\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "overview",
        json!({ "mode": "file", "path": "verb_overview.py" }),
    );

    assert_eq!(
        payload["success"],
        json!(true),
        "overview(mode=file) must delegate to get_symbols_overview: {payload}"
    );
}

#[test]
fn diagnose_verb_unresolved_mode_delegates_without_lsp() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_diag.py"),
        "def ok():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    // `unresolved` routes to unresolved_reference_check (index-based) —
    // deterministic in CI, unlike the LSP-dependent `file`/`issues` modes.
    let payload = call_tool(
        &state,
        "diagnose",
        json!({ "mode": "unresolved", "file_path": "verb_diag.py", "symbol": "ok" }),
    );

    assert_eq!(
        payload["success"],
        json!(true),
        "diagnose(mode=unresolved) must delegate to unresolved_reference_check: {payload}"
    );
}

#[test]
fn analyze_verb_list_mode_delegates_to_list_analysis_jobs() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(&state, "analyze", json!({ "mode": "list" }));

    assert_eq!(
        payload["success"],
        json!(true),
        "analyze(mode=list) must delegate to list_analysis_jobs: {payload}"
    );
}

/// ADR-0016 (item 3 / acceptance f): a verb façade delegates to its resolved
/// target through the same callability gate. `cancel_analysis_job` is
/// registered but not listed on planner-readonly, so under the hidden-alias
/// contract the façade now *reaches* it (returning the target handler's own
/// job-not-found result) instead of bouncing off the listing gate. The
/// "not available in active surface" denial is reserved for unregistered names.
#[test]
fn analyze_verb_cancel_mode_reaches_callable_hidden_target() {
    let project = project_root();
    let state = make_state(&project);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::PlannerReadonly,
    ));

    let payload = call_tool(
        &state,
        "analyze",
        json!({ "mode": "cancel", "job_id": "missing-job" }),
    );

    assert_eq!(
        payload["success"],
        json!(false),
        "cancelling a missing job must still fail (job-not-found), just not via the listing gate: {payload}"
    );
    let error = payload["error"].as_str().unwrap_or_default();
    assert!(
        !error.contains("not available in active surface"),
        "the façade must invoke the now-callable hidden target, not deny it by listing: {error}"
    );
}

#[test]
fn review_verb_missing_mode_is_missing_param() {
    let project = project_root();
    let state = make_state(&project);

    // Schema-level `required = ["mode"]` fires before the handler, so the
    // failure surfaces as a JSON-RPC error response — assert on the full
    // serialized response rather than the tool payload.
    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({ "name": "review", "arguments": {} })),
        },
    )
    .expect("tools/call should return a response");

    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(
        encoded.contains("mode"),
        "missing-mode error must name the `mode` parameter: {encoded}"
    );
    assert!(
        encoded.to_ascii_lowercase().contains("missing")
            || encoded.contains("-32602")
            || encoded.contains("required"),
        "response must be a missing-required-parameter error: {encoded}"
    );
}
