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
