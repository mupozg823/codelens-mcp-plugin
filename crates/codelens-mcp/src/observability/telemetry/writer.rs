use crate::env_compat::dual_prefix_env;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub(super) struct PersistedEvent<'a> {
    pub timestamp_ms: u64,
    pub tool: &'a str,
    pub surface: &'a str,
    pub elapsed_ms: u64,
    pub tokens: usize,
    pub success: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_paths: Option<&'a [String]>,
    #[serde(skip_serializing_if = "<[_]>::is_empty", default)]
    pub suggested_next_tools: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_hint_trigger: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_target_tool: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_handoff_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_id: Option<&'a str>,
}

pub(crate) struct TelemetryWriter {
    path: PathBuf,
}

impl TelemetryWriter {
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

    pub(super) fn append_event(&self, event: &PersistedEvent<'_>) {
        if let Err(err) = self.try_append(event) {
            eprintln!("codelens: telemetry write failed: {err}");
        }
    }

    fn try_append(&self, event: &PersistedEvent<'_>) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
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

    #[cfg(test)]
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}
