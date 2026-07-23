use super::*;
use crate::operation::OperationWorkClass;

pub(super) fn seed_metrics_project() -> (codelens_engine::ProjectRoot, crate::AppState) {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_metrics.py"),
        "def facade_metric_target():\n    pass\n\ndef facade_metric_caller():\n    facade_metric_target()\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));
    (project, state)
}

fn derived_kpis(state: &crate::AppState, session_id: &str) -> serde_json::Value {
    crate::session_metrics_payload::build_session_metrics_payload(state, Some(session_id), None)
        .derived_kpis
}

#[test]
fn facade_call_type_matches_the_resolved_target_instead_of_the_wrapper_name() {
    let (_project, state) = seed_metrics_project();

    call_tool_with_session(
        &state,
        "search",
        json!({ "mode": "symbol", "name": "facade_metric_target" }),
        "facade-primitive",
    );
    call_tool_with_session(
        &state,
        "find_symbol",
        json!({ "name": "facade_metric_target" }),
        "target-primitive",
    );
    call_tool_with_session(
        &state,
        "overview",
        json!({ "mode": "explore" }),
        "facade-composite",
    );
    call_tool_with_session(&state, "explore_codebase", json!({}), "target-composite");
    call_tool_with_session(
        &state,
        "graph",
        json!({ "mode": "trace", "symbol": "facade_metric_target" }),
        "facade-graph-composite",
    );
    call_tool_with_session(
        &state,
        "trace_request_path",
        json!({ "symbol": "facade_metric_target" }),
        "target-graph-composite",
    );

    let facade_primitive = state.metrics().session_snapshot_for("facade-primitive");
    let target_primitive = state.metrics().session_snapshot_for("target-primitive");
    assert_eq!(facade_primitive.call_type.low_level_calls, 1);
    assert_eq!(
        facade_primitive.call_type.low_level_calls, target_primitive.call_type.low_level_calls,
        "search(symbol) must keep find_symbol's primitive classification"
    );
    let facade_invocation = &facade_primitive.timeline[0];
    let target_invocation = &target_primitive.timeline[0];
    assert_eq!(facade_invocation.tool, "search");
    assert_eq!(target_invocation.tool, "find_symbol");
    assert_eq!(
        facade_invocation.resolved_target.as_deref(),
        Some("find_symbol")
    );
    assert_eq!(facade_invocation.mode.as_deref(), Some("symbol"));
    assert_eq!(facade_invocation.work_class, OperationWorkClass::Primitive);
    assert_eq!(facade_invocation.downstream_call_count, 1);
    assert_eq!(
        target_invocation.resolved_target,
        facade_invocation.resolved_target
    );
    assert_eq!(target_invocation.work_class, facade_invocation.work_class);
    assert_eq!(target_invocation.downstream_call_count, 1);

    let facade_composite = state.metrics().session_snapshot_for("facade-composite");
    let target_composite = state.metrics().session_snapshot_for("target-composite");
    assert_eq!(facade_composite.call_type.composite_calls, 1);
    assert_eq!(
        facade_composite.call_type.composite_calls, target_composite.call_type.composite_calls,
        "overview(explore) must keep explore_codebase's composite classification"
    );
    assert_eq!(
        facade_composite.timeline[0].resolved_target.as_deref(),
        Some("explore_codebase")
    );
    assert_eq!(
        facade_composite.timeline[0].mode.as_deref(),
        Some("explore")
    );
    assert_eq!(
        facade_composite.timeline[0].work_class,
        OperationWorkClass::Composite
    );
    assert_eq!(facade_composite.timeline[0].downstream_call_count, 1);

    let graph_composite = state
        .metrics()
        .session_snapshot_for("facade-graph-composite");
    assert_eq!(graph_composite.call_type.composite_calls, 1);
    assert_eq!(
        graph_composite.timeline[0].resolved_target.as_deref(),
        Some("trace_request_path")
    );
    assert_eq!(graph_composite.timeline[0].mode.as_deref(), Some("trace"));
    assert_eq!(
        graph_composite.timeline[0].work_class,
        OperationWorkClass::Composite
    );

    for pair in [
        ("facade-primitive", "target-primitive"),
        ("facade-composite", "target-composite"),
        ("facade-graph-composite", "target-graph-composite"),
    ] {
        let facade_kpis = derived_kpis(&state, pair.0);
        let target_kpis = derived_kpis(&state, pair.1);
        assert_eq!(
            facade_kpis["schema_version"],
            "codelens-session-evidence-kpis"
        );
        assert_eq!(
            facade_kpis["composite_ratio"],
            target_kpis["composite_ratio"]
        );
        assert_eq!(
            facade_kpis["low_level_chain_reduction"],
            target_kpis["low_level_chain_reduction"]
        );
    }
}

#[test]
fn facade_and_target_sequences_have_identical_derived_kpis() {
    let (_project, state) = seed_metrics_project();
    let facade_session = "facade-sequence";
    let target_session = "target-sequence";

    call_tool_with_session(
        &state,
        "search",
        json!({ "mode": "symbol", "name": "facade_metric_target" }),
        facade_session,
    );
    call_tool_with_session(
        &state,
        "overview",
        json!({ "mode": "file", "path": "verb_metrics.py" }),
        facade_session,
    );
    let facade_third = call_tool_with_session(
        &state,
        "graph",
        json!({ "mode": "callers", "function_name": "facade_metric_target" }),
        facade_session,
    );

    call_tool_with_session(
        &state,
        "find_symbol",
        json!({ "name": "facade_metric_target" }),
        target_session,
    );
    call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({ "path": "verb_metrics.py" }),
        target_session,
    );
    let target_third = call_tool_with_session(
        &state,
        "get_callers",
        json!({ "function_name": "facade_metric_target" }),
        target_session,
    );

    for response in [&facade_third, &target_third] {
        assert!(
            response
                .to_string()
                .contains("Repeated low-level chain detected"),
            "the third primitive call must receive canonical chain guidance: {response}"
        );
    }

    let facade_metrics = state.metrics().session_snapshot_for(facade_session);
    let target_metrics = state.metrics().session_snapshot_for(target_session);
    assert_eq!(facade_metrics.call_type.low_level_calls, 3);
    assert_eq!(
        facade_metrics.call_type.low_level_calls,
        target_metrics.call_type.low_level_calls
    );
    assert_eq!(
        facade_metrics.call_type.composite_calls,
        target_metrics.call_type.composite_calls
    );
    assert_eq!(facade_metrics.guidance.repeated_low_level_chain_count, 1);
    assert_eq!(
        facade_metrics.guidance.repeated_low_level_chain_count,
        target_metrics.guidance.repeated_low_level_chain_count
    );
    assert_eq!(
        facade_metrics.guidance.composite_guidance_emitted_count,
        target_metrics.guidance.composite_guidance_emitted_count
    );

    let facade_kpis = derived_kpis(&state, facade_session);
    let target_kpis = derived_kpis(&state, target_session);
    assert_eq!(
        facade_kpis["schema_version"],
        "codelens-session-evidence-kpis"
    );
    assert_eq!(
        facade_kpis["composite_ratio"],
        target_kpis["composite_ratio"]
    );
    assert_eq!(
        facade_kpis["low_level_chain_reduction"],
        target_kpis["low_level_chain_reduction"]
    );
}

#[test]
fn resolved_validation_failure_records_zero_downstream_calls() {
    let (_project, state) = seed_metrics_project();
    let response = call_tool_with_session(
        &state,
        "overview",
        json!({ "mode": "file" }),
        "facade-validation-failure",
    );
    assert!(
        response.get("success").is_none() || response["success"] == json!(false),
        "target validation must not report success: {response}"
    );

    let metrics = state
        .metrics()
        .session_snapshot_for("facade-validation-failure");
    let invocation = &metrics.timeline[0];
    assert_eq!(invocation.tool, "overview");
    assert_eq!(
        invocation.resolved_target.as_deref(),
        Some("get_symbols_overview")
    );
    assert_eq!(invocation.mode.as_deref(), Some("file"));
    assert_eq!(invocation.work_class, OperationWorkClass::Primitive);
    assert_eq!(invocation.downstream_call_count, 0);
    assert!(!invocation.success);
}
