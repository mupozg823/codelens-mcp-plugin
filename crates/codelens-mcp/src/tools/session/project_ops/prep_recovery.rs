use crate::AppState;
use crate::tool_defs::{ToolSurface, is_tool_in_surface};
use serde_json::{Value, json};

fn auto_refresh_stale_threshold(arguments: &Value) -> Option<usize> {
    arguments
        .get("auto_refresh_stale_threshold")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RefreshSymbolIndexRemediation {
    Force,
    StaleOnly,
}

impl RefreshSymbolIndexRemediation {
    pub(super) fn command(self) -> &'static str {
        match self {
            Self::Force => "codelens reindex --force",
            Self::StaleOnly => "codelens reindex --stale-only",
        }
    }

    pub(super) fn args(self) -> Option<Value> {
        match self {
            Self::Force => None,
            Self::StaleOnly => Some(json!({ "scope": "stale_only" })),
        }
    }
}

pub(super) fn refresh_symbol_index_remediation_for_surface(
    surface: ToolSurface,
    mode: RefreshSymbolIndexRemediation,
) -> Value {
    let command = mode.command();
    let args = mode.args();
    let tool_callable = is_tool_in_surface("refresh_symbol_index", surface);

    if tool_callable {
        let mut remediation = serde_json::Map::new();
        remediation.insert("method".to_owned(), json!("tool_call"));
        remediation.insert("tool".to_owned(), json!("refresh_symbol_index"));
        if let Some(args) = args {
            remediation.insert("args".to_owned(), args);
        }
        remediation.insert("callable".to_owned(), json!(true));
        remediation.insert("alternative_command".to_owned(), json!(command));
        return Value::Object(remediation);
    }

    let mut tool_call = serde_json::Map::new();
    tool_call.insert("tool".to_owned(), json!("refresh_symbol_index"));
    if let Some(args) = args {
        tool_call.insert("args".to_owned(), args);
    }
    tool_call.insert("callable".to_owned(), json!(false));
    tool_call.insert("reason".to_owned(), json!("not_in_active_surface"));
    tool_call.insert("surface".to_owned(), json!(surface.as_label()));

    json!({
        "method": "shell",
        "command": command,
        "alternative_command": command,
        "tool_call": Value::Object(tool_call),
    })
}

pub(super) fn refresh_symbol_index_recommended_action_for_surface(
    surface: ToolSurface,
) -> &'static str {
    if is_tool_in_surface("refresh_symbol_index", surface) {
        "refresh_symbol_index"
    } else {
        "run_reindex_command"
    }
}

pub(super) fn index_stats_payload(stats: &codelens_engine::IndexStats) -> Value {
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
    let threshold = auto_refresh_stale_threshold(arguments);

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

    if threshold.is_some_and(|threshold| before.stale_files > threshold) {
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
