use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;

pub fn get_watch_status(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let failure_health = state.watcher_failure_health();
    match state.watcher_stats() {
        Some(mut stats) => {
            stats.index_failures = Some(failure_health.recent_failures);
            let mut payload = serde_json::to_value(stats).unwrap_or_else(|_| json!({}));
            if let Some(map) = payload.as_object_mut() {
                map.insert(
                    "index_failures_total".to_owned(),
                    json!(failure_health.total_failures),
                );
                map.insert(
                    "stale_index_failures".to_owned(),
                    json!(failure_health.stale_failures),
                );
                map.insert(
                    "persistent_index_failures".to_owned(),
                    json!(failure_health.persistent_failures),
                );
                map.insert(
                    "pruned_missing_failures".to_owned(),
                    json!(failure_health.pruned_missing_failures),
                );
                map.insert(
                    "recent_failure_window_seconds".to_owned(),
                    json!(failure_health.recent_window_seconds),
                );
            }
            Ok((payload, success_meta(BackendKind::Config, 1.0)))
        }
        None => Ok((
            json!({
                "running": false,
                "events_processed": 0,
                "files_reindexed": 0,
                "lock_contention_batches": 0,
                "index_failures": failure_health.recent_failures,
                "index_failures_total": failure_health.total_failures,
                "stale_index_failures": failure_health.stale_failures,
                "persistent_index_failures": failure_health.persistent_failures,
                "pruned_missing_failures": failure_health.pruned_missing_failures,
                "recent_failure_window_seconds": failure_health.recent_window_seconds,
                "note": "File watcher not started"
            }),
            success_meta(BackendKind::Config, 1.0),
        )),
    }
}

pub fn prune_index_failures(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let failure_health = state.prune_index_failures()?;
    Ok((
        json!({
            "pruned_missing_failures": failure_health.pruned_missing_failures,
            "index_failures": failure_health.recent_failures,
            "index_failures_total": failure_health.total_failures,
            "stale_index_failures": failure_health.stale_failures,
            "persistent_index_failures": failure_health.persistent_failures,
            "recent_failure_window_seconds": failure_health.recent_window_seconds,
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}
