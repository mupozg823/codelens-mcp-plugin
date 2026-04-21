use crate::AppState;
use serde_json::{Value, json};

const DEFAULT_AUTO_REFRESH_STALE_THRESHOLD: usize = 32;

fn index_stats_payload(stats: &codelens_engine::IndexStats) -> Value {
    json!({
        "indexed_files": stats.indexed_files,
        "supported_files": stats.supported_files,
        "stale_files": stats.stale_files,
    })
}

pub(super) fn prepare_harness_index_recovery(state: &AppState, arguments: &Value) -> Value {
    let enabled = arguments
        .get("auto_refresh_stale")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let threshold = arguments
        .get("auto_refresh_stale_threshold")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_AUTO_REFRESH_STALE_THRESHOLD);

    let before = match state.symbol_index().stats() {
        Ok(stats) => stats,
        Err(error) => {
            return json!({
                "enabled": enabled,
                "threshold": threshold,
                "status": "unavailable",
                "reason": "stats_unavailable",
                "error": error.to_string(),
            });
        }
    };

    if !enabled {
        return json!({
            "enabled": false,
            "threshold": threshold,
            "status": "disabled",
            "before": index_stats_payload(&before),
        });
    }

    if before.stale_files == 0 {
        return json!({
            "enabled": true,
            "threshold": threshold,
            "status": "not_needed",
            "before": index_stats_payload(&before),
            "after": index_stats_payload(&before),
        });
    }

    if before.stale_files > threshold {
        return json!({
            "enabled": true,
            "threshold": threshold,
            "status": "skipped",
            "reason": "stale_threshold_exceeded",
            "before": index_stats_payload(&before),
        });
    }

    match state.symbol_index().refresh_all() {
        Ok(after) => {
            state.graph_cache().invalidate();
            json!({
                "enabled": true,
                "threshold": threshold,
                "status": "refreshed",
                "reason": "stale_detected",
                "before": index_stats_payload(&before),
                "after": index_stats_payload(&after),
            })
        }
        Err(error) => json!({
            "enabled": true,
            "threshold": threshold,
            "status": "failed",
            "reason": "refresh_failed",
            "error": error.to_string(),
            "before": index_stats_payload(&before),
        }),
    }
}
