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
            // Mirror `refresh_symbol_index`: the sparse cache's fingerprint can
            // collide across a same-tick re-scan, so drop this project's sparse
            // entries here too or recovery would keep serving stale symbols.
            state
                .sparse_symbol_cache()
                .invalidate_project(&state.current_project_scope());
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

#[cfg(test)]
mod tests {
    use super::{AppState, prepare_harness_index_recovery};
    use crate::sparse_symbol_cache::{SparseSymbolCacheKey, SparseSymbolIndexFingerprint};
    use crate::symbol_retrieval::SparseSymbolIndex;
    use codelens_engine::ProjectRoot;
    use serde_json::json;
    use std::sync::Arc;

    // Exercise the recovery refresh path end to end: an indexed file is edited
    // on disk so `stats()` reports it stale, a sparse entry is warmed under the
    // project scope, then recovery runs. The refresh must drop that sparse
    // entry — the same gap `refresh_symbol_index` had at the second call site.
    #[test]
    fn recovery_refresh_invalidates_sparse_cache() {
        let dir = tempfile::tempdir().expect("recovery tempdir");
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn original() {}\n").expect("write source");
        let project = ProjectRoot::new_exact(dir.path()).expect("project root");
        let state = AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full);

        // Seed the index, then edit the file so recovery sees a stale file and
        // takes the refresh branch.
        state.symbol_index().refresh_all().expect("initial index");
        std::fs::write(&file, "fn original() {}\nfn added() {}\n").expect("edit source");

        // Warm the sparse cache for this project scope with an arbitrary
        // fingerprint — invalidation must clear it regardless of fingerprint.
        let scope = state.current_project_scope();
        let key = SparseSymbolCacheKey::new(scope.clone(), None);
        let fingerprint = SparseSymbolIndexFingerprint::for_test(1, Some(1_000));
        state.sparse_symbol_cache().store(
            key.clone(),
            fingerprint,
            Arc::new(SparseSymbolIndex::new(Vec::new())),
        );
        assert!(
            state.sparse_symbol_cache().get(&key, fingerprint).is_some(),
            "precondition: sparse entry is warm before recovery"
        );

        let result = prepare_harness_index_recovery(&state, &json!({ "auto_refresh_stale": true }));

        assert_eq!(
            result.get("status").and_then(|v| v.as_str()),
            Some("refreshed"),
            "recovery must take the refresh branch for a stale file: {result}"
        );
        assert!(
            state.sparse_symbol_cache().get(&key, fingerprint).is_none(),
            "recovery refresh must invalidate the project's sparse cache"
        );
    }
}
