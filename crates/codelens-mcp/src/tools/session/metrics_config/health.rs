use super::semantic::{SemanticSearchStatus, determine_semantic_search_status};
use crate::AppState;
use crate::tool_defs::ToolSurface;
use serde_json::{Value, json};

pub(crate) fn build_health_summary(
    index_stats: Option<&codelens_engine::IndexStats>,
    semantic_status: &SemanticSearchStatus,
    daemon_binary_drift: &serde_json::Value,
) -> serde_json::Value {
    let indexed_files = index_stats.map(|s| s.indexed_files).unwrap_or(0);
    let supported_files = index_stats.map(|s| s.supported_files).unwrap_or(0);
    let stale_files = index_stats.map(|s| s.stale_files).unwrap_or(0);
    let mut warnings = Vec::new();

    let mut push_warning =
        |code: &str,
         message: String,
         recommended_action: Option<&str>,
         action_target: Option<&str>,
         extras: Option<serde_json::Map<String, Value>>| {
            let mut warning = serde_json::Map::new();
            warning.insert("code".to_owned(), json!(code));
            warning.insert("severity".to_owned(), json!("warn"));
            warning.insert("message".to_owned(), json!(message));
            warning.insert("recommended_action".to_owned(), json!(recommended_action));
            warning.insert("action_target".to_owned(), json!(action_target));
            if let Some(extras) = extras {
                for (key, value) in extras {
                    warning.insert(key, value);
                }
            }
            warnings.push(Value::Object(warning));
        };

    if supported_files == 0 {
        push_warning(
            "no_supported_files",
            "no supported source files detected".to_string(),
            None,
            None,
            None,
        );
    }
    if indexed_files == 0 {
        push_warning(
            "empty_index",
            "symbol index is empty".to_string(),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
            None,
        );
    }
    if supported_files > 0 && indexed_files < supported_files {
        let unindexed = supported_files.saturating_sub(indexed_files);
        let extras = json!({
            "indexed_files": indexed_files,
            "supported_files": supported_files,
            "unindexed_files": unindexed,
            "remediation": {
                "method": "tool_call",
                "tool": "refresh_symbol_index",
                "args": {},
            },
        });
        push_warning(
            "partial_index_coverage",
            format!("index coverage incomplete ({indexed_files}/{supported_files})"),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
            extras.as_object().cloned(),
        );
    }
    if stale_files > 0 {
        let extras = json!({
            "stale_files": stale_files,
            "indexed_files": indexed_files,
            "supported_files": supported_files,
            "remediation": {
                "method": "tool_call",
                "tool": "refresh_symbol_index",
                "args": {},
            },
        });
        push_warning(
            "stale_index",
            format!("{stale_files} indexed files are stale"),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
            extras.as_object().cloned(),
        );
    }

    #[cfg(feature = "semantic")]
    match semantic_status {
        SemanticSearchStatus::ModelAssetsUnavailable | SemanticSearchStatus::IndexMissing => {
            push_warning(
                semantic_status
                    .reason_code()
                    .unwrap_or("semantic_unavailable"),
                semantic_status
                    .reason_str()
                    .unwrap_or("semantic search unavailable")
                    .to_string(),
                semantic_status.recommended_action(),
                semantic_status.action_target(),
                None,
            );
        }
        _ => {}
    }

    #[cfg(not(feature = "semantic"))]
    if matches!(semantic_status, SemanticSearchStatus::FeatureDisabled) {
        push_warning(
            semantic_status
                .reason_code()
                .unwrap_or("semantic_feature_disabled"),
            semantic_status
                .reason_str()
                .unwrap_or("semantic feature disabled")
                .to_string(),
            semantic_status.recommended_action(),
            semantic_status.action_target(),
            None,
        );
    }

    if daemon_binary_drift
        .get("stale_daemon")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        push_warning(
            daemon_binary_drift
                .get("reason_code")
                .and_then(|v| v.as_str())
                .unwrap_or("stale_daemon"),
            daemon_binary_drift
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("daemon binary drift detected")
                .to_string(),
            daemon_binary_drift
                .get("recommended_action")
                .and_then(|v| v.as_str()),
            daemon_binary_drift
                .get("action_target")
                .and_then(|v| v.as_str()),
            None,
        );
    }

    json!({
        "status": if warnings.is_empty() { "ok" } else { "warn" },
        "warning_count": warnings.len(),
        "warnings": warnings,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeHealthSnapshot {
    pub(crate) index_stats: Option<codelens_engine::IndexStats>,
    pub(crate) semantic_status: SemanticSearchStatus,
    pub(crate) daemon_binary_drift: serde_json::Value,
    pub(crate) health_summary: serde_json::Value,
}

impl RuntimeHealthSnapshot {
    pub(crate) fn index_fresh(&self) -> bool {
        self.index_stats
            .as_ref()
            .map(|stats| stats.stale_files == 0 && stats.indexed_files > 0)
            .unwrap_or(false)
    }

    pub(crate) fn indexed_files(&self) -> usize {
        self.index_stats
            .as_ref()
            .map(|stats| stats.indexed_files)
            .unwrap_or(0)
    }

    pub(crate) fn supported_files(&self) -> usize {
        self.index_stats
            .as_ref()
            .map(|stats| stats.supported_files)
            .unwrap_or(0)
    }

    pub(crate) fn stale_files(&self) -> usize {
        self.index_stats
            .as_ref()
            .map(|stats| stats.stale_files)
            .unwrap_or(0)
    }
}

pub(crate) fn collect_runtime_health_snapshot(
    state: &AppState,
    surface: ToolSurface,
) -> RuntimeHealthSnapshot {
    let index_stats = state.symbol_index().stats().ok();
    let semantic_status = determine_semantic_search_status(state, surface);
    let daemon_binary_drift = crate::build_info::daemon_binary_drift_payload(
        state.daemon_started_at(),
        Some(state.project().as_path()),
    );
    let health_summary =
        build_health_summary(index_stats.as_ref(), &semantic_status, &daemon_binary_drift);
    RuntimeHealthSnapshot {
        index_stats,
        semantic_status,
        daemon_binary_drift,
        health_summary,
    }
}
