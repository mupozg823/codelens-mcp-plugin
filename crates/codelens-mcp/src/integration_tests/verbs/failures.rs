use super::*;

#[test]
fn unresolved_facade_modes_do_not_count_as_primitive_or_composite_work() {
    let (_project, state) = super::metrics::seed_metrics_project();
    let session_id = "facade-unresolved-modes";

    let unknown = call_tool_with_session(
        &state,
        "search",
        json!({ "mode": "unknown", "name": "facade_metric_target" }),
        session_id,
    );
    let missing = call_tool_with_session(
        &state,
        "search",
        json!({ "name": "facade_metric_target" }),
        session_id,
    );

    for response in [&unknown, &missing] {
        assert!(
            response.get("success").is_none() || response["success"] == json!(false),
            "resolution failure must not report success: {response}"
        );
    }
    let metrics = state.metrics().session_snapshot_for(session_id);
    assert_eq!(metrics.core.total_calls, 2);
    assert_eq!(metrics.call_type.low_level_calls, 0);
    assert_eq!(metrics.call_type.composite_calls, 0);
    assert!(
        metrics
            .timeline
            .iter()
            .all(|invocation| invocation.resolved_target.is_none()
                && invocation.work_class == crate::operation::OperationWorkClass::Unresolved
                && invocation.downstream_call_count == 0)
    );
    assert_eq!(metrics.timeline[0].mode.as_deref(), Some("unknown"));
    assert_eq!(metrics.timeline[1].mode, None);
}
