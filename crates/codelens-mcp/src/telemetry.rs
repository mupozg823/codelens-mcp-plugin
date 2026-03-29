//! Per-tool usage telemetry: call counts, latency, and error tracking.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metrics for a single tool.
#[derive(Debug, Default, Serialize, Clone)]
pub struct ToolMetrics {
    pub call_count: u64,
    pub total_ms: u64,
    pub max_ms: u64,
    pub error_count: u64,
    /// Last invocation timestamp (unix epoch milliseconds).
    pub last_called_at: u64,
}

/// Thread-safe registry that accumulates per-tool metrics.
pub struct ToolMetricsRegistry {
    inner: Mutex<HashMap<String, ToolMetrics>>,
}

impl ToolMetricsRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Record a single tool invocation.
    pub fn record_call(&self, name: &str, elapsed_ms: u64, success: bool) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let entry = map.entry(name.to_owned()).or_default();
        entry.call_count += 1;
        entry.total_ms += elapsed_ms;
        if elapsed_ms > entry.max_ms {
            entry.max_ms = elapsed_ms;
        }
        if !success {
            entry.error_count += 1;
        }
        entry.last_called_at = now;
    }

    /// Return a snapshot of all metrics, sorted by call_count descending.
    pub fn snapshot(&self) -> Vec<(String, ToolMetrics)> {
        let map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut entries: Vec<(String, ToolMetrics)> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        entries.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
        entries
    }

    /// Clear all recorded metrics.
    pub fn reset(&self) {
        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_snapshot() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("find_symbol", 42, true);
        reg.record_call("find_symbol", 58, true);

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 1);

        let (name, m) = &snap[0];
        assert_eq!(name, "find_symbol");
        assert_eq!(m.call_count, 2);
        assert_eq!(m.total_ms, 100);
        assert_eq!(m.max_ms, 58);
        assert_eq!(m.error_count, 0);
        assert!(m.last_called_at > 0);
    }

    #[test]
    fn multiple_tools_independent() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("find_symbol", 10, true);
        reg.record_call("get_ranked_context", 20, true);

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 2);

        let names: Vec<&str> = snap.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"find_symbol"));
        assert!(names.contains(&"get_ranked_context"));
    }

    #[test]
    fn error_count_tracked() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("bad_tool", 5, false);
        reg.record_call("bad_tool", 3, true);
        reg.record_call("bad_tool", 7, false);

        let snap = reg.snapshot();
        let (_, m) = &snap[0];
        assert_eq!(m.call_count, 3);
        assert_eq!(m.error_count, 2);
    }

    #[test]
    fn reset_clears_all() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("a", 1, true);
        reg.record_call("b", 2, true);
        assert_eq!(reg.snapshot().len(), 2);

        reg.reset();
        assert!(reg.snapshot().is_empty());
    }

    #[test]
    fn snapshot_sorted_by_call_count() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("low", 1, true);
        reg.record_call("high", 1, true);
        reg.record_call("high", 1, true);
        reg.record_call("high", 1, true);
        reg.record_call("mid", 1, true);
        reg.record_call("mid", 1, true);

        let snap = reg.snapshot();
        let counts: Vec<u64> = snap.iter().map(|(_, m)| m.call_count).collect();
        assert_eq!(counts, vec![3, 2, 1]);
        assert_eq!(snap[0].0, "high");
        assert_eq!(snap[1].0, "mid");
        assert_eq!(snap[2].0, "low");
    }
}
