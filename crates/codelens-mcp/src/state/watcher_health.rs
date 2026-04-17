use super::AppState;
use crate::error::CodeLensError;
use crate::runtime_types::WatcherFailureHealth;

/// Sliding window (seconds) over which watcher index-failure counts are
/// aggregated for health reporting. Owned here because this submodule is
/// the sole consumer.
const WATCHER_RECENT_FAILURE_WINDOW_SECS: i64 = 15 * 60;

pub(super) fn watcher_failure_health(state: &AppState) -> WatcherFailureHealth {
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    let summary = db
        .index_failure_summary(WATCHER_RECENT_FAILURE_WINDOW_SECS)
        .unwrap_or_default();
    let scope = state.current_project_scope();
    let pruned_missing_failures = state
        .watcher_maintenance
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&scope)
        .copied()
        .unwrap_or(0);
    WatcherFailureHealth {
        recent_failures: summary.recent_failures,
        total_failures: summary.total_failures,
        stale_failures: summary.stale_failures,
        persistent_failures: summary.persistent_failures,
        pruned_missing_failures,
        recent_window_seconds: WATCHER_RECENT_FAILURE_WINDOW_SECS,
    }
}

pub(super) fn prune_index_failures(
    state: &AppState,
) -> Result<WatcherFailureHealth, CodeLensError> {
    let project = state.project();
    let scope = state.current_project_scope();
    let symbol_index = state.symbol_index();
    let pruned_missing_failures = {
        let db = symbol_index.db();
        db.prune_missing_index_failures(project.as_path())?
    };
    state
        .watcher_maintenance
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(scope, pruned_missing_failures);
    Ok(watcher_failure_health(state))
}
