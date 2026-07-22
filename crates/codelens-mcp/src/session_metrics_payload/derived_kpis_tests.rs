use super::{DERIVED_KPI_SCHEMA_VERSION, build_derived_kpis};
use crate::runtime_types::WatcherFailureHealth;
use crate::telemetry::{SessionMetrics, ToolInvocation};
use codelens_engine::WatcherStats;
use serde_json::json;

#[test]
fn computes_rates_and_infers_refactoring_session() {
    let session = SessionMetrics {
        core: crate::telemetry::CoreMetrics {
            total_calls: 4,
            success_count: 2,
            total_tokens: 1000,
            ..Default::default()
        },
        call_type: crate::telemetry::CallTypeMetrics {
            composite_calls: 2,
            low_level_calls: 2,
        },
        truncation: crate::telemetry::TruncationMetrics {
            handle_reuse_count: 1,
            ..Default::default()
        },
        jobs: crate::telemetry::AnalysisJobMetrics {
            analysis_jobs_started: 4,
            analysis_jobs_completed: 3,
            ..Default::default()
        },
        timeline: vec![
            invocation("plan_safe_refactor"),
            invocation("safe_rename_report"),
            invocation("rename_symbol"),
            invocation("replace_symbol_body"),
        ],
        ..Default::default()
    };
    let watcher_stats = WatcherStats {
        running: true,
        events_processed: 10,
        files_reindexed: 7,
        lock_contention_batches: 2,
        index_failures: None,
    };
    let watcher_failure_health = WatcherFailureHealth {
        recent_failures: 1,
        total_failures: 4,
        ..Default::default()
    };

    let kpis = build_derived_kpis(&session, 2, Some(&watcher_stats), &watcher_failure_health);

    assert_eq!(kpis["schema_version"], json!(DERIVED_KPI_SCHEMA_VERSION));
    assert_eq!(kpis["composite_ratio"], json!(0.5));
    assert_eq!(kpis["surface_token_efficiency"], json!(500.0));
    assert_eq!(kpis["handle_reuse_rate"], json!(0.5));
    assert_eq!(kpis["analysis_job_success_rate"], json!(0.75));
    assert_eq!(kpis["watcher_lock_contention_rate"], json!(0.2));
    assert_eq!(kpis["watcher_recent_failure_share"], json!(0.25));
    assert_eq!(kpis["inferred_session_type"], json!("refactoring"));
}

fn invocation(tool: &str) -> ToolInvocation {
    ToolInvocation {
        tool: tool.to_owned(),
        resolved_target: Some(tool.to_owned()),
        mode: None,
        work_class: crate::operation::operation_work_class(tool),
        downstream_call_count: 1,
        surface: "builder-minimal".to_owned(),
        elapsed_ms: 1,
        tokens: 1,
        success: true,
        truncated: false,
        phase: None,
        target_paths: Vec::new(),
    }
}
