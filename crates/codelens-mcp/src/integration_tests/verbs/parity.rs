use super::*;

fn handler_data(payload: &serde_json::Value) -> serde_json::Value {
    let mut data = payload["data"].clone();
    if let Some(object) = data.as_object_mut() {
        for response_only in [
            "index_freshness",
            "truncated",
            "auto_summarized",
            "truncation_warning",
            "_omitted_keys",
            "delegated_tool",
        ] {
            object.remove(response_only);
        }
    }
    data
}

#[test]
fn facade_and_target_payloads_are_equivalent_for_primitive_and_composite_modes() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_parity.py"),
        "def facade_parity_target():\n    pass\n\ndef facade_parity_caller():\n    facade_parity_target()\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let pairs = [
        (
            "search",
            json!({ "mode": "symbol", "name": "facade_parity_target" }),
            "find_symbol",
            json!({ "name": "facade_parity_target" }),
        ),
        (
            "overview",
            json!({ "mode": "file", "path": "verb_parity.py" }),
            "get_symbols_overview",
            json!({ "path": "verb_parity.py" }),
        ),
        (
            "graph",
            json!({ "mode": "callers", "function_name": "facade_parity_target" }),
            "get_callers",
            json!({ "function_name": "facade_parity_target" }),
        ),
        (
            "overview",
            json!({ "mode": "explore" }),
            "explore_codebase",
            json!({}),
        ),
        (
            "graph",
            json!({ "mode": "trace", "symbol": "facade_parity_target" }),
            "trace_request_path",
            json!({ "symbol": "facade_parity_target" }),
        ),
    ];

    for (facade, facade_args, target, target_args) in pairs {
        let facade_payload = call_tool(&state, facade, facade_args);
        let target_payload = call_tool(&state, target, target_args);
        assert_eq!(facade_payload["success"], json!(true), "{facade} failed");
        assert_eq!(target_payload["success"], json!(true), "{target} failed");
        assert_eq!(
            handler_data(&facade_payload),
            handler_data(&target_payload),
            "{facade} must preserve {target}'s handler data"
        );
    }
}

#[test]
fn composite_facade_mode_suppresses_low_level_chain_guidance() {
    let (_project, state) = super::metrics::seed_metrics_project();
    let session_id = "facade-composite-chain";
    call_tool_with_session(
        &state,
        "search",
        json!({ "mode": "symbol", "name": "facade_metric_target" }),
        session_id,
    );
    call_tool_with_session(
        &state,
        "overview",
        json!({ "mode": "file", "path": "verb_metrics.py" }),
        session_id,
    );
    let composite =
        call_tool_with_session(&state, "overview", json!({ "mode": "explore" }), session_id);

    assert!(
        !composite
            .to_string()
            .contains("Repeated low-level chain detected"),
        "a resolved composite must break the primitive chain: {composite}"
    );
    let metrics = state.metrics().session_snapshot_for(session_id);
    assert_eq!(metrics.call_type.low_level_calls, 2);
    assert_eq!(metrics.call_type.composite_calls, 1);
    assert_eq!(metrics.guidance.repeated_low_level_chain_count, 0);
    assert_eq!(metrics.guidance.composite_guidance_emitted_count, 0);
}
