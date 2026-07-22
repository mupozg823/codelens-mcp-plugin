use super::*;

fn event<'a>(tool: &'a str, surface: &'a str) -> ToolCallEvent<'a> {
    ToolCallEvent {
        tool,
        operation: crate::operation::ResolvedOperation::direct(tool).dispatched(),
        elapsed_ms: 0,
        tokens: 0,
        success: true,
        surface,
        truncated: false,
        phase: None,
        logical_session_id: None,
        client_name: None,
        target_paths: &[],
        hints: CallTelemetryHints::default(),
    }
}

#[test]
fn record_and_snapshot() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call("find_symbol", 42, true);
    reg.record_call("find_symbol", 58, true);

    let snap = reg.snapshot();
    assert_eq!(snap.len(), 1);

    let (name, m) = &snap[0];
    assert_eq!(name, "find_symbol");
    assert_eq!(m.call_count, 2);
    assert_eq!(m.total_ms, 100);
    assert_eq!(m.max_ms, 58);
    assert_eq!(m.error_count, 0);
    assert!(m.last_called_at > 0);
}

#[test]
fn multiple_tools_independent() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call("find_symbol", 10, true);
    reg.record_call("get_ranked_context", 20, true);

    let snap = reg.snapshot();
    assert_eq!(snap.len(), 2);

    let names: Vec<&str> = snap.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"find_symbol"));
    assert!(names.contains(&"get_ranked_context"));
}

#[test]
fn error_count_tracked() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call("bad_tool", 5, false);
    reg.record_call("bad_tool", 3, true);
    reg.record_call("bad_tool", 7, false);

    let snap = reg.snapshot();
    let (_, m) = &snap[0];
    assert_eq!(m.call_count, 3);
    assert_eq!(m.error_count, 2);
}

#[test]
fn reset_clears_all() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call("a", 1, true);
    reg.record_call("b", 2, true);
    assert_eq!(reg.snapshot().len(), 2);

    reg.reset();
    assert!(reg.snapshot().is_empty());
}

#[test]
fn session_metrics_accumulate() {
    let reg = ToolMetricsRegistry::new();
    reg.record_event(ToolCallEvent {
        elapsed_ms: 15,
        tokens: 500,
        ..event("find_symbol", "planner-readonly")
    });
    reg.record_event(ToolCallEvent {
        elapsed_ms: 42,
        tokens: 2000,
        ..event("get_ranked_context", "planner-readonly")
    });
    reg.record_event(ToolCallEvent {
        elapsed_ms: 8,
        success: false,
        ..event("rename_symbol", "refactor-full")
    });

    let session = reg.session_snapshot();
    assert_eq!(session.core.total_calls, 3);
    assert_eq!(session.core.total_ms, 65);
    assert_eq!(session.core.total_tokens, 2500);
    assert_eq!(session.core.error_count, 1);
    assert_eq!(session.timeline.len(), 3);
    assert_eq!(session.timeline[0].tool, "find_symbol");
    assert_eq!(session.timeline[0].surface, "planner-readonly");
    assert_eq!(session.timeline[1].tokens, 2000);
    assert!(!session.timeline[2].success);

    let surfaces = reg.surface_snapshot();
    assert_eq!(surfaces.len(), 2);
    assert_eq!(session.call_type.low_level_calls, 3);
}

#[test]
fn transport_counts_accumulate() {
    let reg = ToolMetricsRegistry::new();
    reg.record_transport_session("stdio");
    reg.record_transport_session("http");
    reg.record_transport_session("http");

    let session = reg.session_snapshot();
    assert_eq!(session.transport.stdio_session_count, 1);
    assert_eq!(session.transport.http_session_count, 2);
}

#[test]
fn analysis_queue_metrics_accumulate() {
    let reg = ToolMetricsRegistry::new();
    reg.record_analysis_worker_pool(2, 3, "http");
    reg.record_analysis_job_enqueued(2, 4, true);
    reg.record_analysis_job_started(1, 3);
    reg.record_analysis_job_finished(crate::runtime_types::JobLifecycle::Completed, 0, 0);
    reg.record_analysis_job_cancelled(0, 0);

    let session = reg.session_snapshot();
    assert_eq!(session.jobs.analysis_jobs_enqueued, 1);
    assert_eq!(session.jobs.analysis_jobs_started, 1);
    assert_eq!(session.jobs.analysis_jobs_completed, 1);
    assert_eq!(session.jobs.analysis_jobs_cancelled, 1);
    assert_eq!(session.jobs.analysis_queue_max_depth, 2);
    assert_eq!(session.jobs.analysis_queue_max_weighted_depth, 4);
    assert_eq!(session.jobs.analysis_queue_priority_promotions, 1);
    assert_eq!(session.jobs.analysis_queue_depth, 0);
    assert_eq!(session.jobs.active_analysis_workers, 0);
    assert_eq!(session.jobs.peak_active_analysis_workers, 1);
    assert_eq!(session.jobs.analysis_cost_budget, 3);
}

#[test]
fn session_reset_clears() {
    let reg = ToolMetricsRegistry::new();
    reg.record_event(ToolCallEvent {
        elapsed_ms: 10,
        tokens: 100,
        ..event("a", "planner-readonly")
    });
    assert_eq!(reg.session_snapshot().core.total_calls, 1);

    reg.reset();
    let session = reg.session_snapshot();
    assert_eq!(session.core.total_calls, 0);
    assert_eq!(session.core.total_tokens, 0);
    assert!(session.timeline.is_empty());
}

#[test]
fn session_call_count_tracks_logical_sessions_independently() {
    let reg = ToolMetricsRegistry::new();
    reg.record_event(ToolCallEvent {
        elapsed_ms: 15,
        tokens: 100,
        logical_session_id: Some("session-a"),
        ..event("find_symbol", "planner-readonly")
    });
    reg.record_event(ToolCallEvent {
        elapsed_ms: 15,
        tokens: 100,
        logical_session_id: Some("session-a"),
        ..event("find_symbol", "planner-readonly")
    });
    reg.record_event(ToolCallEvent {
        elapsed_ms: 20,
        tokens: 100,
        logical_session_id: Some("session-b"),
        ..event("impact_report", "reviewer-graph")
    });

    assert_eq!(reg.session_call_count("session-a"), 2);
    assert_eq!(reg.session_call_count("session-b"), 1);
    assert_eq!(reg.session_call_count("missing"), 0);
}

#[test]
fn reset_clears_session_rate_limit_windows() {
    let reg = ToolMetricsRegistry::new();
    reg.record_event(ToolCallEvent {
        elapsed_ms: 15,
        tokens: 100,
        logical_session_id: Some("session-a"),
        ..event("find_symbol", "planner-readonly")
    });
    assert_eq!(reg.session_call_count("session-a"), 1);
    reg.reset();
    assert_eq!(reg.session_call_count("session-a"), 0);
}

#[test]
fn truncation_metrics_capture_followup() {
    let reg = ToolMetricsRegistry::new();
    reg.record_event(ToolCallEvent {
        elapsed_ms: 20,
        tokens: 1200,
        truncated: true,
        phase: Some("review"),
        ..event("analyze_change_request", "planner-readonly")
    });
    reg.record_event(ToolCallEvent {
        elapsed_ms: 18,
        tokens: 800,
        phase: Some("review"),
        ..event("analyze_change_request", "planner-readonly")
    });
    reg.record_event(ToolCallEvent {
        elapsed_ms: 10,
        tokens: 500,
        truncated: true,
        ..event("impact_report", "reviewer-graph")
    });
    reg.record_analysis_read_for_session(true, None);

    let session = reg.session_snapshot();
    assert_eq!(session.truncation.truncated_response_count, 2);
    assert_eq!(session.truncation.truncation_followup_count, 2);
    assert_eq!(session.truncation.truncation_same_tool_retry_count, 1);
    assert_eq!(session.truncation.truncation_handle_followup_count, 1);
}

#[test]
fn profile_and_preset_switch_counts_accumulate() {
    let reg = ToolMetricsRegistry::new();
    reg.record_preset_switch_for_session(None);
    reg.record_preset_switch_for_session(None);
    reg.record_profile_switch_for_session(None);

    let session = reg.session_snapshot();
    assert_eq!(session.namespace.preset_switch_count, 2);
    assert_eq!(session.namespace.profile_switch_count, 1);
}

#[test]
fn snapshot_sorted_by_call_count() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call("low", 1, true);
    reg.record_call("high", 1, true);
    reg.record_call("high", 1, true);
    reg.record_call("high", 1, true);
    reg.record_call("mid", 1, true);
    reg.record_call("mid", 1, true);

    let snap = reg.snapshot();
    let counts: Vec<u64> = snap.iter().map(|(_, m)| m.call_count).collect();
    assert_eq!(counts, vec![3, 2, 1]);
    assert_eq!(snap[0].0, "high");
    assert_eq!(snap[1].0, "mid");
    assert_eq!(snap[2].0, "low");
}
