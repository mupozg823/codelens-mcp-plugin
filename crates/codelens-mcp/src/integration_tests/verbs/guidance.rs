use super::*;

#[test]
fn primitive_verb_chain_emits_host_neutral_composite_guidance() {
    let project = project_root();
    assert!(
        fs::write(
            project.as_path().join("verb_chain.py"),
            "def sink():\n    pass\n\ndef source():\n    sink()\n",
        )
        .is_ok()
    );
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let _ = call_tool(
        &state,
        "search",
        json!({ "mode": "symbol", "name": "sink" }),
    );
    let _ = call_tool(
        &state,
        "overview",
        json!({ "mode": "file", "path": "verb_chain.py" }),
    );
    let payload = call_tool(
        &state,
        "graph",
        json!({ "mode": "callers", "function_name": "sink" }),
    );

    assert_eq!(payload["success"], json!(true));
    let suggested = payload["suggested_next_tools"].as_array();
    assert!(
        suggested
            .is_some_and(|tools| { tools.iter().any(|tool| tool == "cleanup_duplicate_logic") }),
        "the third resolved primitive must emit composite recovery guidance: {payload}"
    );
    assert!(
        !suggested
            .is_some_and(|tools| { tools.iter().any(|tool| tool == "delegate_to_codex_builder") })
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["composite_guidance_emitted_count"],
        json!(1)
    );
}
