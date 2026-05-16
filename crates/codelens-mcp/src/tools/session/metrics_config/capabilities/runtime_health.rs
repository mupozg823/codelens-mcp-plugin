use crate::AppState;
use crate::tool_defs::ToolSurface;

use super::semantic::{
    SemanticSearchStatus, build_health_summary, determine_semantic_search_status,
};

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
