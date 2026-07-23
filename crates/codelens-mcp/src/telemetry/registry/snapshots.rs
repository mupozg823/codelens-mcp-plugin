use super::{ToolMetricsRegistry, trim_rate_limit_window};
use crate::telemetry::{SessionMetrics, SurfaceMetrics, ToolMetrics};
use std::time::{SystemTime, UNIX_EPOCH};

impl ToolMetricsRegistry {
    /// Return a snapshot of all per-tool metrics, sorted by call_count descending.
    pub fn snapshot(&self) -> Vec<(String, ToolMetrics)> {
        let map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut entries: Vec<(String, ToolMetrics)> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        entries.sort_by_key(|b| std::cmp::Reverse(b.1.call_count));
        entries
    }

    /// Return a snapshot of session-level metrics.
    pub fn session_snapshot(&self) -> SessionMetrics {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn session_snapshot_for(&self, logical_session_id: &str) -> SessionMetrics {
        self.session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(logical_session_id)
            .map(|bucket| bucket.session.clone())
            .unwrap_or_default()
    }

    /// Enumerate logical session ids currently tracked in the telemetry
    /// bucket map. Used by aggregation surfaces such as `eval_session_audit`
    /// that need to iterate every known session in one pass.
    pub fn tracked_session_ids(&self) -> Vec<String> {
        self.session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Return the number of calls recorded for the logical session within
    /// the recent sliding window used by dispatch rate limiting.
    pub fn session_call_count(&self, logical_session_id: &str) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut windows = self
            .session_windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(window) = windows.get_mut(logical_session_id) else {
            return 0;
        };
        trim_rate_limit_window(window, now);
        let count = window.len() as u64;
        if count == 0 {
            windows.remove(logical_session_id);
        }
        count
    }

    pub fn surface_snapshot(&self) -> Vec<(String, SurfaceMetrics)> {
        let map = self
            .surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries: Vec<(String, SurfaceMetrics)> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.1.call_count));
        entries
    }

    pub fn snapshot_for_session(&self, logical_session_id: &str) -> Vec<(String, ToolMetrics)> {
        let buckets = self
            .session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(bucket) = buckets.get(logical_session_id) else {
            return Vec::new();
        };
        let mut entries: Vec<(String, ToolMetrics)> = bucket
            .tools
            .iter()
            .map(|(name, metrics)| (name.clone(), metrics.clone()))
            .collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.1.call_count));
        entries
    }

    pub fn surface_snapshot_for_session(
        &self,
        logical_session_id: &str,
    ) -> Vec<(String, SurfaceMetrics)> {
        let buckets = self
            .session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(bucket) = buckets.get(logical_session_id) else {
            return Vec::new();
        };
        let mut entries: Vec<(String, SurfaceMetrics)> = bucket
            .surfaces
            .iter()
            .map(|(surface, metrics)| (surface.clone(), metrics.clone()))
            .collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.1.call_count));
        entries
    }

    pub fn has_session_snapshot(&self, logical_session_id: &str) -> bool {
        self.session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(logical_session_id)
    }

    /// Clear all recorded metrics.
    #[allow(dead_code)] // used in tests
    pub fn reset(&self) {
        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.clear();

        let mut surfaces = self
            .surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        surfaces.clear();

        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *session = SessionMetrics::default();

        let mut session_windows = self
            .session_windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session_windows.clear();

        let mut session_buckets = self
            .session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session_buckets.clear();
    }

    /// Return tool names that have zero calls in the telemetry JSONL log
    /// within the given `window_days`. Requires telemetry persistence to be
    /// enabled. Falls back to an empty vec if the log is missing or unreadable.
    pub fn underutilized_tools(&self, all_tool_names: &[String], window_days: u64) -> Vec<String> {
        let writer = match &self.writer {
            Some(w) => w,
            None => return Vec::new(),
        };
        let path = writer.path();
        if !path.exists() {
            return Vec::new();
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let cutoff_ms = now_ms.saturating_sub(window_days * 24 * 60 * 60 * 1000);

        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let reader = std::io::BufReader::new(file);
        let mut called = std::collections::HashSet::new();

        for line in std::io::BufRead::lines(reader) {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let value: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let ts = value
                .get("timestamp_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if ts >= cutoff_ms {
                if let Some(tool) = value.get("tool").and_then(|v| v.as_str()) {
                    called.insert(tool.to_owned());
                }
            }
        }

        all_tool_names
            .iter()
            .filter(|name| !called.contains(name.as_str()))
            .cloned()
            .collect()
    }
}
