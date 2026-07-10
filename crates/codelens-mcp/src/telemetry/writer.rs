//! Append-only JSONL telemetry persistence.
#![allow(clippy::collapsible_if)]

use crate::env_compat::dual_prefix_env;
use crate::telemetry::ToolCallEvent;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;

/// A single telemetry event appended to the persistence log.
#[derive(Debug, Serialize)]
pub(crate) struct PersistedEvent<'a> {
    pub(crate) timestamp_ms: u64,
    pub(crate) tool: &'a str,
    pub(crate) surface: &'a str,
    pub(crate) elapsed_ms: u64,
    pub(crate) tokens: usize,
    pub(crate) success: bool,
    pub(crate) truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) client_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) phase: Option<&'a str>,
    /// Whether this row came from the live server or a test-only writer.
    pub(crate) recording_origin: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_paths: Option<&'a [String]>,
    #[serde(skip_serializing_if = "<[_]>::is_empty", default)]
    pub(crate) suggested_next_tools: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delegate_hint_trigger: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delegate_target_tool: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delegate_handoff_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) handoff_id: Option<&'a str>,
}

impl<'a> PersistedEvent<'a> {
    pub(crate) fn from_tool_call(timestamp_ms: u64, event: &ToolCallEvent<'a>) -> Self {
        Self {
            timestamp_ms,
            tool: event.tool,
            surface: event.surface,
            elapsed_ms: event.elapsed_ms,
            tokens: event.tokens,
            success: event.success,
            truncated: event.truncated,
            session_id: event.logical_session_id,
            client_name: event.client_name,
            phase: event.phase,
            recording_origin: if cfg!(test) { "test" } else { "runtime" },
            target_paths: (!event.target_paths.is_empty()).then_some(event.target_paths),
            suggested_next_tools: event.hints.suggested_next_tools,
            delegate_hint_trigger: event.hints.delegate_hint_trigger,
            delegate_target_tool: event.hints.delegate_target_tool,
            delegate_handoff_id: event.hints.delegate_handoff_id,
            handoff_id: event.hints.handoff_id,
        }
    }
}

/// Append-only JSONL writer for tool invocation telemetry.
///
/// Enabled via `SYMBIOTE_TELEMETRY_ENABLED=1` / `CODELENS_TELEMETRY_ENABLED=1`
/// (default path `.codelens/telemetry/tool_usage.jsonl`) or
/// `SYMBIOTE_TELEMETRY_PATH=<override>` / `CODELENS_TELEMETRY_PATH=<override>`.
///
/// The writer runs on the hot dispatch path. All I/O failures are logged once
/// and swallowed so telemetry can never break tool execution.
pub(crate) struct TelemetryWriter {
    path: PathBuf,
}

impl TelemetryWriter {
    /// Resolve a writer from environment variables. Returns `None` when
    /// persistence is disabled (the default).
    pub(crate) fn from_env() -> Option<Self> {
        if let Some(custom) = dual_prefix_env("CODELENS_TELEMETRY_PATH") {
            return Some(Self {
                path: PathBuf::from(custom),
            });
        }
        let enabled = dual_prefix_env("CODELENS_TELEMETRY_ENABLED")
            .map(|v| {
                let lowered = v.to_ascii_lowercase();
                matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false);
        if enabled {
            return Some(Self {
                path: PathBuf::from(".codelens/telemetry/tool_usage.jsonl"),
            });
        }
        None
    }

    /// Append a single event. Errors are reported to stderr once and swallowed.
    pub(crate) fn append_event(&self, event: &PersistedEvent<'_>) {
        if let Err(err) = self.try_append(event) {
            eprintln!("codelens: telemetry write failed: {err}");
        }
    }

    fn try_append(&self, event: &PersistedEvent<'_>) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut line = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())
    }

    #[cfg(test)]
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}
