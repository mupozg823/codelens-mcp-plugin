//! Per-tool usage telemetry: call counts, latency, and error tracking.

mod helpers;
mod session_mutations;
#[cfg(test)]
mod tests;
mod types;
mod writer;

use self::helpers::{SessionCallRef, record_session_call, record_surface_call, record_tool_call};
pub(crate) use self::types::percentile_95;
pub use self::types::{
    CallTelemetryHints, SessionMetrics, SurfaceMetrics, ToolInvocation, ToolMetrics,
};
use self::types::{SessionTelemetryBucket, trim_rate_limit_window};
use self::writer::{PersistedEvent, TelemetryWriter};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Thread-safe registry that accumulates per-tool and session-level metrics.
pub struct ToolMetricsRegistry {
    inner: Mutex<HashMap<String, ToolMetrics>>,
    surfaces: Mutex<HashMap<String, SurfaceMetrics>>,
    session: Mutex<SessionMetrics>,
    session_buckets: Mutex<HashMap<String, SessionTelemetryBucket>>,
    session_windows: Mutex<HashMap<String, VecDeque<u64>>>,
    writer: Option<TelemetryWriter>,
}

impl ToolMetricsRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            surfaces: Mutex::new(HashMap::new()),
            session: Mutex::new(SessionMetrics::default()),
            session_buckets: Mutex::new(HashMap::new()),
            session_windows: Mutex::new(HashMap::new()),
            writer: TelemetryWriter::from_env(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_writer(writer: Option<TelemetryWriter>) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            surfaces: Mutex::new(HashMap::new()),
            session: Mutex::new(SessionMetrics::default()),
            session_buckets: Mutex::new(HashMap::new()),
            session_windows: Mutex::new(HashMap::new()),
            writer,
        }
    }

    /// Record a single tool invocation (per-tool + session).
    #[allow(dead_code)] // used in tests and as convenience wrapper
    pub fn record_call(&self, name: &str, elapsed_ms: u64, success: bool) {
        self.record_call_with_tokens(name, elapsed_ms, success, 0, "unknown", false, None);
    }

    /// Record a tool invocation with token estimate.
    #[allow(clippy::too_many_arguments)]
    pub fn record_call_with_tokens(
        &self,
        name: &str,
        elapsed_ms: u64,
        success: bool,
        tokens: usize,
        surface: &str,
        truncated: bool,
        phase: Option<&str>,
    ) {
        self.record_call_with_tokens_for_session(
            name, elapsed_ms, success, tokens, surface, truncated, phase, None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_call_with_targets_for_session(
        &self,
        name: &str,
        elapsed_ms: u64,
        success: bool,
        tokens: usize,
        surface: &str,
        truncated: bool,
        phase: Option<&str>,
        logical_session_id: Option<&str>,
        target_paths: &[String],
        telemetry_hints: CallTelemetryHints<'_>,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if let Some(session_id) = logical_session_id {
            let mut windows = self
                .session_windows
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let window = windows.entry(session_id.to_owned()).or_default();
            trim_rate_limit_window(window, now);
            window.push_back(now);
        }

        // Per-tool metrics
        {
            let mut map = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            record_tool_call(&mut map, name, elapsed_ms, success, tokens, now);
        }

        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let session_call = SessionCallRef {
                name,
                elapsed_ms,
                success,
                tokens,
                surface,
                truncated,
                phase,
                target_paths,
            };
            record_session_call(&mut session, session_call);
        }

        if let Some(session_id) = logical_session_id {
            let mut buckets = self
                .session_buckets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let bucket = buckets.entry(session_id.to_owned()).or_default();
            record_tool_call(&mut bucket.tools, name, elapsed_ms, success, tokens, now);
            record_surface_call(
                &mut bucket.surfaces,
                surface,
                elapsed_ms,
                success,
                tokens,
                now,
            );
            record_session_call(
                &mut bucket.session,
                SessionCallRef {
                    name,
                    elapsed_ms,
                    success,
                    tokens,
                    surface,
                    truncated,
                    phase,
                    target_paths,
                },
            );
        }

        {
            let mut surfaces = self
                .surfaces
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            record_surface_call(&mut surfaces, surface, elapsed_ms, success, tokens, now);
        }

        // Persist the event to the append-only telemetry log if enabled.
        // Failures are swallowed so telemetry can never break dispatch.
        if let Some(writer) = &self.writer {
            writer.append_event(&PersistedEvent {
                timestamp_ms: now,
                tool: name,
                surface,
                elapsed_ms,
                tokens,
                success,
                truncated,
                session_id: logical_session_id,
                phase,
                target_paths: (!target_paths.is_empty()).then_some(target_paths),
                suggested_next_tools: telemetry_hints.suggested_next_tools,
                delegate_hint_trigger: telemetry_hints.delegate_hint_trigger,
                delegate_target_tool: telemetry_hints.delegate_target_tool,
                delegate_handoff_id: telemetry_hints.delegate_handoff_id,
                handoff_id: telemetry_hints.handoff_id,
            });
        }
    }

    /// Record a tool invocation with token estimate and logical session context.
    #[allow(clippy::too_many_arguments)]
    pub fn record_call_with_tokens_for_session(
        &self,
        name: &str,
        elapsed_ms: u64,
        success: bool,
        tokens: usize,
        surface: &str,
        truncated: bool,
        phase: Option<&str>,
        logical_session_id: Option<&str>,
    ) {
        self.record_call_with_targets_for_session(
            name,
            elapsed_ms,
            success,
            tokens,
            surface,
            truncated,
            phase,
            logical_session_id,
            &[],
            CallTelemetryHints::default(),
        );
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
        entries.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
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
        entries.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
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
        entries.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
        entries
    }

    pub fn has_session_snapshot(&self, logical_session_id: &str) -> bool {
        self.session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(logical_session_id)
    }
}
