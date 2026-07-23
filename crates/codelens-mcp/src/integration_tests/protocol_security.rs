use super::*;

// ── Security, daemon mode, and surface gating ─────────────────────────────────────

#[test]
fn tool_call_result_meta_exposes_host_neutral_execution_policy() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "replace_symbol_body",
                "arguments": {
                    "_session_id": default_session_id(&state),
                    "relative_path": "new_file.py",
                    "symbol_name": "alpha",
                    "new_body": "    return 2"
                }
            })),
        },
    )
    .expect("tools/call should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    assert!(
        value["result"]["_meta"]
            .get("codelens/preferredExecutor")
            .is_none(),
        "model-specific executor routing must not leak into tool responses"
    );
    assert_eq!(
        value["result"]["_meta"]["codelens/executionPolicy"],
        json!({
            "execution_class": "mutate",
            "risk": "high",
            "cost_hint": "medium",
            "concurrency_safe": false,
        })
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
        "replace_symbol_body",
        json!({"relative_path": "blocked.py", "symbol_name": "alpha", "new_body": "    return 2"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("blocked by daemon mode")
    );
}

/// ADR-0016 hidden-alias contract: callability is registration-scoped, so a
/// mutation tool that is *not listed* on a read-only surface is no longer
/// bounced by the listing gate — but the read-only / mutation gate must still
/// block it. The security outcome (blocked) is preserved; only the gate that
/// fires changes (surface-listing → read-only-surface).
#[test]
fn unlisted_mutation_stays_blocked_by_read_only_surface_gate() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );

    let payload = call_tool(
        &state,
        "replace_symbol_body",
        json!({"relative_path": "blocked.py", "symbol_name": "alpha", "new_body": "    return 2"}),
    );
    assert_eq!(payload["success"], json!(false));
    let error = payload["error"].as_str().unwrap_or("");
    assert!(
        error.contains("read-only surface"),
        "mutation on a read-only surface must be blocked by the read-only gate, \
         not the listing gate: {error}"
    );
    assert!(
        !error.contains("not available in active surface"),
        "the surface-listing gate must no longer be the thing that blocks a \
         registered mutation tool (ADR-0016 hidden aliases): {error}"
    );
}

/// ADR-0016 (b): a registered read tool that is not listed on the active
/// surface stays callable as a hidden alias and the success payload carries
/// `surface_note = "hidden_alias"`. get_callers is the canonical case — it left
/// the reviewer-graph listed surface in the diet but remains dispatchable.
#[test]
fn unlisted_registered_read_tool_callable_with_hidden_alias_note() {
    let project = project_root();
    fs::write(
        project.as_path().join("hidden_alias.py"),
        "def leaf():\n    pass\n\ndef root():\n    leaf()\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    // get_callers is registered in tools.toml but no longer listed on the
    // reviewer-graph surface (ADR-0016). It must still dispatch.
    assert!(
        !crate::tool_defs::is_tool_in_surface(
            "get_callers",
            crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::ReviewerGraph),
        ),
        "test premise: get_callers must be unlisted on reviewer-graph"
    );

    let payload = call_tool(&state, "get_callers", json!({ "function_name": "leaf" }));
    assert_eq!(
        payload["success"],
        json!(true),
        "unlisted-but-registered get_callers must stay callable on reviewer-graph: {payload}"
    );
    // Injected top-level on the tool payload, which the response pipeline nests
    // under `data` (same placement as the #347 `project_binding` hint).
    assert_eq!(
        payload["data"]["surface_note"],
        json!("hidden_alias"),
        "a hidden-alias call must be flagged with surface_note: {payload}"
    );
}

/// ADR-0016 (c): the "not available in active surface" rejection is now
/// reserved for names that are neither listed nor registered. On a narrowed
/// profile surface a genuinely unregistered name still hits that gate (under
/// the Full preset such names instead flow to the dispatch layer's unknown-tool
/// path, unchanged from before this change).
#[test]
fn unregistered_tool_name_still_rejected_on_profile_surface() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    let payload = call_tool(&state, "totally_unregistered_tool_xyz", json!({}));
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("not available in active surface"),
        "an unregistered name must still be rejected by the surface gate: {payload}"
    );
}

#[test]
fn read_only_surface_marks_content_mutations_for_blocking() {
    assert!(crate::tool_defs::is_read_only_surface(
        crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::PlannerReadonly),
    ));
    assert!(crate::tool_defs::is_content_mutation_tool(
        "replace_symbol_body"
    ));
    assert!(!crate::tool_defs::is_content_mutation_tool("set_profile"));
}
