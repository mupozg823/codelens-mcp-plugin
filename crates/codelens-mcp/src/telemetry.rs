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

/// A single tool invocation in the session timeline.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInvocation {
    pub tool: String,
    pub elapsed_ms: u64,
    pub tokens: usize,
    pub success: bool,
}

/// Session-level aggregate metrics across all tool calls.
#[derive(Debug, Default, Serialize, Clone)]
pub struct SessionMetrics {
    pub total_calls: u64,
    pub total_ms: u64,
    pub total_tokens: usize,
    pub error_count: u64,
    /// Ordered tool invocation timeline (capped at 200 entries).
    pub timeline: Vec<ToolInvocation>,
}

const MAX_TIMELINE: usize = 200;

/// Thread-safe registry that accumulates per-tool and session-level metrics.
pub struct ToolMetricsRegistry {
    inner: Mutex<HashMap<String, ToolMetrics>>,
    session: Mutex<SessionMetrics>,
}

impl ToolMetricsRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            session: Mutex::new(SessionMetrics::default()),
        }
    }

    /// Record a single tool invocation (per-tool + session).
    #[allow(dead_code)] // used in tests and as convenience wrapper
    pub fn record_call(&self, name: &str, elapsed_ms: u64, success: bool) {
        self.record_call_with_tokens(name, elapsed_ms, success, 0);
    }

    /// Record a tool invocation with token estimate.
    pub fn record_call_with_tokens(
        &self,
        name: &str,
        elapsed_ms: u64,
        success: bool,
        tokens: usize,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Per-tool metrics
        {
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

        // Session-level metrics
        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            session.total_calls += 1;
            session.total_ms += elapsed_ms;
            session.total_tokens += tokens;
            if !success {
                session.error_count += 1;
            }
            if session.timeline.len() < MAX_TIMELINE {
                session.timeline.push(ToolInvocation {
                    tool: name.to_owned(),
                    elapsed_ms,
                    tokens,
                    success,
                });
            }
        }
    }

    /// Return a snapshot of all per-tool metrics, sorted by call_count descending.
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

    /// Return a snapshot of session-level metrics.
    pub fn session_snapshot(&self) -> SessionMetrics {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Clear all recorded metrics.
    #[allow(dead_code)] // used in tests
    pub fn reset(&self) {
        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.clear();

        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *session = SessionMetrics::default();
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
    fn session_metrics_accumulate() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call_with_tokens("find_symbol", 15, true, 500);
        reg.record_call_with_tokens("get_ranked_context", 42, true, 2000);
        reg.record_call_with_tokens("rename_symbol", 8, false, 0);

        let session = reg.session_snapshot();
        assert_eq!(session.total_calls, 3);
        assert_eq!(session.total_ms, 65);
        assert_eq!(session.total_tokens, 2500);
        assert_eq!(session.error_count, 1);
        assert_eq!(session.timeline.len(), 3);
        assert_eq!(session.timeline[0].tool, "find_symbol");
        assert_eq!(session.timeline[1].tokens, 2000);
        assert!(!session.timeline[2].success);
    }

    #[test]
    fn session_reset_clears() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call_with_tokens("a", 10, true, 100);
        assert_eq!(reg.session_snapshot().total_calls, 1);

        reg.reset();
        let session = reg.session_snapshot();
        assert_eq!(session.total_calls, 0);
        assert_eq!(session.total_tokens, 0);
        assert!(session.timeline.is_empty());
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
