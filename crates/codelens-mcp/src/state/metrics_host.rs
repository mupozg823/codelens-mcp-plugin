//! Tool metrics, doom-loop counters, recent buffers, and token budget
//! accessors for `AppState`.
//!
//! Pure move from `state.rs` — no logic changes.

use crate::telemetry::ToolMetricsRegistry;

use super::AppState;

impl AppState {
    /// Access the tool metrics registry.
    pub(crate) fn metrics(&self) -> &ToolMetricsRegistry {
        self.metrics.as_ref()
    }

    /// Record a tool call in the recent tools ring buffer.
    pub(crate) fn push_recent_tool(&self, name: &str) {
        self.recent_tools.push(name.to_owned());
    }

    /// Doom-loop detection: returns (repeat_count, is_rapid_burst).
    /// Threshold of 3 triggers a warning. `is_rapid_burst` is true when
    /// 3+ identical calls occur within 10 seconds (agent retry loop).
    ///
    /// Keyed by `session_id` so concurrent HTTP sessions maintain independent
    /// counters and do not corrupt each other's state.
    pub(crate) fn doom_loop_count(
        &self,
        session_id: &str,
        name: &str,
        args_hash: u64,
    ) -> (usize, bool) {
        let mut counters = self
            .doom_loop_counter
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let now = Self::now_ms();
        let entry = counters
            .entry(session_id.to_owned())
            .or_insert_with(|| (String::new(), 0, 0, now));
        if entry.0 == name && entry.1 == args_hash {
            entry.2 += 1;
        } else {
            *entry = (name.to_owned(), args_hash, 1, now);
        }
        let is_rapid = entry.2 >= 3 && (now.saturating_sub(entry.3) < 10_000);
        (entry.2, is_rapid)
    }

    /// Get the recent tool call names (up to 5).
    pub(crate) fn recent_tools(&self) -> Vec<String> {
        self.recent_tools.snapshot()
    }

    /// Record a file path as recently accessed (for ranking boost).
    pub(crate) fn record_file_access(&self, path: &str) {
        self.recent_files.push_dedup(path);
    }

    /// Get recently accessed file paths (most recent last).
    pub(crate) fn recent_file_paths(&self) -> Vec<String> {
        self.recent_files.snapshot()
    }

    /// Record an analysis_id for cross-phase context.
    pub(crate) fn push_recent_analysis_id(&self, id: String) {
        self.recent_analysis_ids.push(id);
    }

    /// Get recent analysis IDs (most recent last).
    pub(crate) fn recent_analysis_ids(&self) -> Vec<String> {
        self.recent_analysis_ids.snapshot()
    }

    /// Current global token budget.
    pub(crate) fn token_budget(&self) -> usize {
        self.token_budget.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Set global token budget.
    pub(crate) fn set_token_budget(&self, budget: usize) {
        self.token_budget
            .store(budget, std::sync::atomic::Ordering::Relaxed);
    }
}
