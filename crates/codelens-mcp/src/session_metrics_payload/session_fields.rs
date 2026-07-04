use crate::agent_coordination::{CoordinationCounts, CoordinationLockStats};
use crate::runtime_types::WatcherFailureHealth;
use crate::telemetry::SessionMetrics;
use codelens_engine::WatcherStats;
use serde_json::{Map, Value};

mod core_fields;
mod guidance_fields;
mod ops_fields;

pub(super) struct SessionFieldInputs<'a> {
    pub(super) session: &'a SessionMetrics,
    pub(super) active_http_sessions: usize,
    pub(super) session_resume_supported: bool,
    pub(super) session_timeout_seconds: u64,
    pub(super) coordination: &'a CoordinationCounts,
    pub(super) coordination_lock: &'a CoordinationLockStats,
    pub(super) daemon_mode: &'a str,
    pub(super) watcher_stats: Option<&'a WatcherStats>,
    pub(super) watcher_failure_health: &'a WatcherFailureHealth,
}

pub(super) fn build_session_fields(inputs: SessionFieldInputs<'_>) -> Map<String, Value> {
    let mut session_json = Map::new();
    core_fields::put_core_fields(&mut session_json, &inputs);
    guidance_fields::put_guidance_fields(&mut session_json, inputs.session);
    ops_fields::put_ops_fields(&mut session_json, &inputs);
    session_json
}

fn put(m: &mut Map<String, Value>, k: &str, v: Value) {
    m.insert(k.to_owned(), v);
}

#[cfg(test)]
mod tests {
    use super::{SessionFieldInputs, build_session_fields};
    use crate::agent_coordination::{CoordinationCounts, CoordinationLockStats};
    use crate::runtime_types::WatcherFailureHealth;
    use crate::telemetry::SessionMetrics;
    use codelens_engine::WatcherStats;
    use serde_json::json;

    #[test]
    fn builds_session_fields_from_typed_snapshots() {
        let session = SessionMetrics {
            core: crate::telemetry::CoreMetrics {
                total_calls: 4,
                success_count: 3,
                total_ms: 40,
                total_tokens: 1200,
                ..Default::default()
            },
            jobs: crate::telemetry::AnalysisJobMetrics {
                analysis_jobs_started: 2,
                analysis_jobs_completed: 1,
                analysis_transport_mode: "http".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        let coordination = CoordinationCounts {
            active_agents: 2,
            active_claims: 3,
        };
        let coordination_lock = CoordinationLockStats {
            acquire_count: 4,
            wait_total_micros: 20,
            wait_max_micros: 9,
        };
        let watcher_stats = WatcherStats {
            running: true,
            events_processed: 8,
            files_reindexed: 5,
            lock_contention_batches: 1,
            index_failures: None,
        };
        let watcher_failure_health = WatcherFailureHealth {
            recent_failures: 1,
            total_failures: 3,
            stale_failures: 1,
            ..Default::default()
        };

        let fields = build_session_fields(SessionFieldInputs {
            session: &session,
            active_http_sessions: 7,
            session_resume_supported: true,
            session_timeout_seconds: 1800,
            coordination: &coordination,
            coordination_lock: &coordination_lock,
            daemon_mode: "mutation-enabled",
            watcher_stats: Some(&watcher_stats),
            watcher_failure_health: &watcher_failure_health,
        });

        assert_eq!(fields["total_calls"], json!(4));
        assert_eq!(fields["active_http_sessions"], json!(7));
        assert_eq!(fields["active_coordination_agents"], json!(2));
        assert_eq!(fields["coordination_lock_avg_wait_micros"], json!(5));
        assert_eq!(fields["daemon_mode"], json!("mutation-enabled"));
        assert_eq!(fields["watcher_running"], json!(true));
        assert_eq!(fields["watcher_stale_index_failures"], json!(1));
        assert_eq!(fields["avg_ms_per_call"], json!(10));
        assert_eq!(fields["avg_tool_output_tokens"], json!(300));
        assert_eq!(fields["analysis_transport_mode"], json!("http"));
    }
}
