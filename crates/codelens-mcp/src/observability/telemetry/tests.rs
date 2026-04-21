use super::*;

mod persistence;

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
    reg.record_call_with_tokens(
        "find_symbol",
        15,
        true,
        500,
        "planner-readonly",
        false,
        None,
    );
    reg.record_call_with_tokens(
        "get_ranked_context",
        42,
        true,
        2000,
        "planner-readonly",
        false,
        None,
    );
    reg.record_call_with_tokens("rename_symbol", 8, false, 0, "refactor-full", false, None);

    let session = reg.session_snapshot();
    assert_eq!(session.total_calls, 3);
    assert_eq!(session.total_ms, 65);
    assert_eq!(session.total_tokens, 2500);
    assert_eq!(session.error_count, 1);
    assert_eq!(session.timeline.len(), 3);
    assert_eq!(session.timeline[0].tool, "find_symbol");
    assert_eq!(session.timeline[0].surface, "planner-readonly");
    assert_eq!(session.timeline[1].tokens, 2000);
    assert!(!session.timeline[2].success);

    let surfaces = reg.surface_snapshot();
    assert_eq!(surfaces.len(), 2);
    assert_eq!(session.low_level_calls, 3);
}

#[test]
fn transport_counts_accumulate() {
    let reg = ToolMetricsRegistry::new();
    reg.record_transport_session("stdio");
    reg.record_transport_session("http");
    reg.record_transport_session("http");

    let session = reg.session_snapshot();
    assert_eq!(session.stdio_session_count, 1);
    assert_eq!(session.http_session_count, 2);
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
    assert_eq!(session.analysis_jobs_enqueued, 1);
    assert_eq!(session.analysis_jobs_started, 1);
    assert_eq!(session.analysis_jobs_completed, 1);
    assert_eq!(session.analysis_jobs_cancelled, 1);
    assert_eq!(session.analysis_queue_max_depth, 2);
    assert_eq!(session.analysis_queue_max_weighted_depth, 4);
    assert_eq!(session.analysis_queue_priority_promotions, 1);
    assert_eq!(session.analysis_queue_depth, 0);
    assert_eq!(session.active_analysis_workers, 0);
    assert_eq!(session.peak_active_analysis_workers, 1);
    assert_eq!(session.analysis_cost_budget, 3);
}

#[test]
fn session_reset_clears() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call_with_tokens("a", 10, true, 100, "planner-readonly", false, None);
    assert_eq!(reg.session_snapshot().total_calls, 1);

    reg.reset();
    let session = reg.session_snapshot();
    assert_eq!(session.total_calls, 0);
    assert_eq!(session.total_tokens, 0);
    assert!(session.timeline.is_empty());
}

#[test]
fn session_call_count_tracks_logical_sessions_independently() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call_with_tokens_for_session(
        "find_symbol",
        15,
        true,
        100,
        "planner-readonly",
        false,
        None,
        Some("session-a"),
    );
    reg.record_call_with_tokens_for_session(
        "find_symbol",
        15,
        true,
        100,
        "planner-readonly",
        false,
        None,
        Some("session-a"),
    );
    reg.record_call_with_tokens_for_session(
        "impact_report",
        20,
        true,
        100,
        "reviewer-graph",
        false,
        None,
        Some("session-b"),
    );

    assert_eq!(reg.session_call_count("session-a"), 2);
    assert_eq!(reg.session_call_count("session-b"), 1);
    assert_eq!(reg.session_call_count("missing"), 0);
}

#[test]
fn reset_clears_session_rate_limit_windows() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call_with_tokens_for_session(
        "find_symbol",
        15,
        true,
        100,
        "planner-readonly",
        false,
        None,
        Some("session-a"),
    );
    assert_eq!(reg.session_call_count("session-a"), 1);
    reg.reset();
    assert_eq!(reg.session_call_count("session-a"), 0);
}

#[test]
fn truncation_metrics_capture_followup() {
    let reg = ToolMetricsRegistry::new();
    reg.record_call_with_tokens(
        "analyze_change_request",
        20,
        true,
        1200,
        "planner-readonly",
        true,
        Some("review"),
    );
    reg.record_call_with_tokens(
        "analyze_change_request",
        18,
        true,
        800,
        "planner-readonly",
        false,
        Some("review"),
    );
    reg.record_call_with_tokens("impact_report", 10, true, 500, "reviewer-graph", true, None);
    reg.record_analysis_read(true);

    let session = reg.session_snapshot();
    assert_eq!(session.truncated_response_count, 2);
    assert_eq!(session.truncation_followup_count, 2);
    assert_eq!(session.truncation_same_tool_retry_count, 1);
    assert_eq!(session.truncation_handle_followup_count, 1);
}

#[test]
fn profile_and_preset_switch_counts_accumulate() {
    let reg = ToolMetricsRegistry::new();
    reg.record_preset_switch();
    reg.record_preset_switch();
    reg.record_profile_switch();

    let session = reg.session_snapshot();
    assert_eq!(session.preset_switch_count, 2);
    assert_eq!(session.profile_switch_count, 1);
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
