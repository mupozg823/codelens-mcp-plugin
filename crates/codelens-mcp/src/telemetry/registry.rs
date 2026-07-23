//! Thread-safe telemetry registry and session recording logic.
#![allow(clippy::collapsible_if)]

use super::writer::{PersistedEvent, TelemetryWriter};
use super::{SessionMetrics, SurfaceMetrics, ToolCallEvent, ToolMetrics};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Clone)]
struct SessionTelemetryBucket {
    tools: HashMap<String, ToolMetrics>,
    surfaces: HashMap<String, SurfaceMetrics>,
    session: SessionMetrics,
}

const MAX_TIMELINE: usize = 200;
const MAX_LATENCY_SAMPLES: usize = 256;
const SESSION_RATE_LIMIT_WINDOW_MS: u64 = 60_000;
mod classification;
mod events;
mod recording;
mod snapshots;
use classification::{has_low_level_chain, is_workflow_tool};
use recording::{record_session_call, record_surface_call, record_tool_call};

#[cfg(test)]
fn default_telemetry_writer() -> Option<TelemetryWriter> {
    // Tests must opt in explicitly so a developer's runtime telemetry env
    // cannot make unrelated cargo-test cases append fixture calls to JSONL.
    if matches!(
        std::env::var("CODELENS_TEST_TELEMETRY_ENABLED").as_deref(),
        Ok("1")
    ) {
        TelemetryWriter::from_env()
    } else {
        None
    }
}

#[cfg(not(test))]
fn default_telemetry_writer() -> Option<TelemetryWriter> {
    TelemetryWriter::from_env()
}

fn push_latency_sample(samples: &mut VecDeque<u64>, elapsed_ms: u64) {
    if samples.len() >= MAX_LATENCY_SAMPLES {
        samples.pop_front();
    }
    samples.push_back(elapsed_ms);
}

fn trim_rate_limit_window(samples: &mut VecDeque<u64>, now_ms: u64) {
    while let Some(oldest) = samples.front().copied() {
        if now_ms.saturating_sub(oldest) <= SESSION_RATE_LIMIT_WINDOW_MS {
            break;
        }
        samples.pop_front();
    }
}

pub(crate) fn percentile_95(samples: &VecDeque<u64>) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut values = samples.iter().copied().collect::<Vec<_>>();
    values.sort_unstable();
    let index = ((values.len() - 1) * 95) / 100;
    values[index]
}

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
            writer: default_telemetry_writer(),
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
        self.record_event(ToolCallEvent {
            tool: name,
            elapsed_ms,
            tokens: 0,
            success,
            surface: "unknown",
            truncated: false,
            phase: None,
            logical_session_id: None,
            client_name: None,
            target_paths: &[],
            hints: Default::default(),
        });
    }

    /// Record a completed call once and fan its facts out to all telemetry
    /// sinks (global/session aggregates and optional JSONL persistence).
    pub(crate) fn record_event(&self, event: ToolCallEvent<'_>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if let Some(session_id) = event.logical_session_id {
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

            record_tool_call(&mut map, &event, now);
        }

        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            record_session_call(&mut session, &event);
        }

        if let Some(session_id) = event.logical_session_id {
            let mut buckets = self
                .session_buckets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let bucket = buckets.entry(session_id.to_owned()).or_default();
            record_tool_call(&mut bucket.tools, &event, now);
            record_surface_call(&mut bucket.surfaces, &event, now);
            record_session_call(&mut bucket.session, &event);
        }

        {
            let mut surfaces = self
                .surfaces
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            record_surface_call(&mut surfaces, &event, now);
        }

        // Persist the event to the append-only telemetry log if enabled.
        // Failures are swallowed so telemetry can never break dispatch.
        if let Some(writer) = &self.writer {
            writer.append_event(&PersistedEvent::from_tool_call(now, &event));
        }
    }
}

impl ToolMetricsRegistry {
    fn mutate_session_metrics<F>(&self, logical_session_id: Option<&str>, mut f: F)
    where
        F: FnMut(&mut SessionMetrics),
    {
        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&mut session);
        }
        if let Some(session_id) = logical_session_id {
            let mut buckets = self
                .session_buckets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let bucket = buckets.entry(session_id.to_owned()).or_default();
            f(&mut bucket.session);
        }
    }
}
