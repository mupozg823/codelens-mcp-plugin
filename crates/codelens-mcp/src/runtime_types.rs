use serde::{Deserialize, Serialize};

fn default_risk_level() -> String {
    "medium".to_owned()
}

fn default_verifier_status() -> String {
    "caution".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeTransportMode {
    Stdio,
    Http,
}

impl RuntimeTransportMode {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "http" => Self::Http,
            _ => Self::Stdio,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeDaemonMode {
    Standard,
    ReadOnly,
    MutationEnabled,
}

impl RuntimeDaemonMode {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "read-only" | "readonly" | "read_only" => Self::ReadOnly,
            "mutation-enabled" | "mutation_enabled" | "mutating" => Self::MutationEnabled,
            _ => Self::Standard,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::ReadOnly => "read-only",
            Self::MutationEnabled => "mutation-enabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCoordinationMode {
    Advisory,
    Strict,
}

impl RuntimeCoordinationMode {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "strict" => Self::Strict,
            _ => Self::Advisory,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::Strict => "strict",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct WatcherFailureHealth {
    pub recent_failures: usize,
    pub total_failures: usize,
    pub stale_failures: usize,
    pub persistent_failures: usize,
    pub pruned_missing_failures: usize,
    pub recent_window_seconds: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AnalysisArtifact {
    pub id: String,
    pub tool_name: String,
    pub surface: String,
    #[serde(default)]
    pub project_scope: Option<String>,
    #[serde(default)]
    pub cache_key: Option<String>,
    pub summary: String,
    pub top_findings: Vec<String>,
    #[serde(default = "default_risk_level")]
    pub risk_level: String,
    pub confidence: f64,
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub readiness: AnalysisReadiness,
    #[serde(default)]
    pub verifier_checks: Vec<AnalysisVerifierCheck>,
    pub available_sections: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AnalysisReadiness {
    #[serde(default = "default_verifier_status")]
    pub diagnostics_ready: String,
    #[serde(default = "default_verifier_status")]
    pub reference_safety: String,
    #[serde(default = "default_verifier_status")]
    pub test_readiness: String,
    #[serde(default = "default_verifier_status")]
    pub mutation_ready: String,
    /// Phase P4-c: per-axis human-readable explanation for
    /// caution/blocked verdicts. Keyed by axis name
    /// (`"reference_safety"`, `"mutation_ready"`, etc). Always
    /// serialized (even when empty) so schema readers get a stable
    /// shape.
    #[serde(default)]
    pub rationale: std::collections::BTreeMap<String, String>,
}

impl Default for AnalysisReadiness {
    fn default() -> Self {
        Self {
            diagnostics_ready: default_verifier_status(),
            reference_safety: default_verifier_status(),
            test_readiness: default_verifier_status(),
            mutation_ready: default_verifier_status(),
            rationale: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct AnalysisVerifierCheck {
    #[serde(default)]
    pub check: String,
    #[serde(default = "default_verifier_status")]
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub evidence_section: Option<String>,
    /// Phase P4-c: machine-readable closing condition for the check.
    /// Populated via `default_pass_condition(check, status)` when the
    /// verifier doesn't have a check-specific template. Always
    /// non-empty on the write path so a downstream filter can assert
    /// `every_check_has_pass_condition`.
    #[serde(default)]
    pub pass_condition: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AnalysisSummary {
    pub id: String,
    pub tool_name: String,
    pub summary: String,
    pub surface: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobLifecycle {
    Queued,
    Running,
    Completed,
    Cancelled,
    Error,
}

impl JobLifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }

    #[allow(dead_code)]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Error)
    }
}

impl std::fmt::Display for JobLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AnalysisJob {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub project_scope: Option<String>,
    pub status: JobLifecycle,
    pub progress: u8,
    pub current_step: Option<String>,
    pub profile_hint: Option<String>,
    pub estimated_sections: Vec<String>,
    pub analysis_id: Option<String>,
    pub error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct RecentPreflight {
    pub tool_name: String,
    pub analysis_id: Option<String>,
    pub surface: String,
    pub timestamp_ms: u64,
    pub readiness: AnalysisReadiness,
    pub blocker_count: usize,
    pub target_paths: Vec<String>,
    pub symbol: Option<String>,
    pub overlapping_claim_count: usize,
    pub overlapping_claim_session_ids: Vec<String>,
    pub overlapping_claim_paths: Vec<String>,
}
