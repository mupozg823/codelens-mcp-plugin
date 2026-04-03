#[cfg(feature = "semantic")]
use codelens_core::EmbeddingEngine;
use codelens_core::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::analysis_queue::{
    analysis_job_cost_units, AnalysisJobRequest, AnalysisWorkerQueue, HTTP_ANALYSIS_WORKER_COUNT,
    STDIO_ANALYSIS_WORKER_COUNT,
};
use crate::error::CodeLensError;
use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface};
use serde_json::Value;
use std::collections::VecDeque;

const MAX_ANALYSIS_ARTIFACTS: usize = 64;
const MAX_ANALYSIS_JOBS: usize = 128;
const ANALYSIS_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const WATCHER_RECENT_FAILURE_WINDOW_SECS: i64 = 15 * 60;
pub(crate) const PREFLIGHT_TTL_MS: u64 = 10 * 60 * 1000;

fn default_risk_level() -> String {
    "medium".to_owned()
}

fn default_verifier_status() -> String {
    "caution".to_owned()
}

fn push_unique_string(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

fn normalize_path_for_project(project_root: &Path, path: &str) -> String {
    let normalized = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        project_root.join(path)
    };
    normalized
        .strip_prefix(project_root)
        .map(|relative| relative.to_path_buf())
        .unwrap_or(normalized)
        .to_string_lossy()
        .replace('\\', "/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeTransportMode {
    Stdio,
    Http,
}

impl RuntimeTransportMode {
    fn from_str(value: &str) -> Self {
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

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct WatcherFailureHealth {
    pub recent_failures: usize,
    pub total_failures: usize,
    pub stale_failures: usize,
    pub persistent_failures: usize,
    pub pruned_missing_failures: usize,
    pub recent_window_seconds: i64,
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

// ── Application state ──────────────────────────────────────────────────

/// Holds project-specific resources that can be swapped at runtime.
struct ProjectOverride {
    project: ProjectRoot,
    symbol_index: Arc<SymbolIndex>,
    graph_cache: Arc<GraphCache>,
    memories_dir: PathBuf,
    analysis_dir: PathBuf,
    audit_dir: PathBuf,
    #[allow(dead_code)]
    watcher: Option<FileWatcher>,
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
}

impl Default for AnalysisReadiness {
    fn default() -> Self {
        Self {
            diagnostics_ready: default_verifier_status(),
            reference_safety: default_verifier_status(),
            test_readiness: default_verifier_status(),
            mutation_ready: default_verifier_status(),
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
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AnalysisSummary {
    pub id: String,
    pub tool_name: String,
    pub summary: String,
    pub surface: String,
    pub created_at_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AnalysisJob {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub project_scope: Option<String>,
    pub status: String,
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
}

pub(crate) struct AppState {
    // Default project (set at startup, immutable)
    default_project: ProjectRoot,
    default_symbol_index: Arc<SymbolIndex>,
    default_graph_cache: Arc<GraphCache>,
    default_memories_dir: PathBuf,
    default_analysis_dir: PathBuf,
    default_audit_dir: PathBuf,
    // Runtime project override (set by activate_project)
    project_override: std::sync::RwLock<Option<ProjectOverride>>,
    lsp_pool: LspSessionPool,
    transport_mode: Mutex<RuntimeTransportMode>,
    daemon_mode: Mutex<RuntimeDaemonMode>,
    surface: Mutex<ToolSurface>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    pub(crate) token_budget: std::sync::atomic::AtomicUsize,
    analysis_seq: std::sync::atomic::AtomicU64,
    job_seq: std::sync::atomic::AtomicU64,
    pub(crate) metrics: Arc<ToolMetricsRegistry>,
    /// Recent tool call names for context-aware suggestions (max 5).
    recent_tools: Mutex<VecDeque<String>>,
    recent_preflights: Mutex<HashMap<String, RecentPreflight>>,
    analysis_order: Mutex<VecDeque<String>>,
    analyses: Mutex<HashMap<String, AnalysisArtifact>>,
    job_order: Mutex<VecDeque<String>>,
    jobs: Mutex<HashMap<String, AnalysisJob>>,
    analysis_queue: OnceLock<AnalysisWorkerQueue>,
    pub(crate) watcher: Option<FileWatcher>,
    watcher_maintenance: Mutex<HashMap<String, usize>>,
    #[cfg(feature = "semantic")]
    pub(crate) embedding: std::sync::OnceLock<Option<EmbeddingEngine>>,
    /// Secondary (read-only) project indexes for cross-project queries.
    pub(crate) secondary_projects: Mutex<HashMap<String, SecondaryProject>>,
    #[cfg(feature = "http")]
    pub(crate) session_store: Option<crate::server::session::SessionStore>,
}

/// A read-only project registered for cross-project queries.
pub(crate) struct SecondaryProject {
    pub project: ProjectRoot,
    pub index: Arc<SymbolIndex>,
}

impl AppState {
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn analysis_expired(created_at_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(created_at_ms) > ANALYSIS_TTL_MS
    }

    pub(crate) fn current_project_scope(&self) -> String {
        self.project().as_path().to_string_lossy().to_string()
    }

    pub(crate) fn preflight_ttl_seconds(&self) -> u64 {
        PREFLIGHT_TTL_MS / 1000
    }

    fn matches_current_project_scope(&self, scope: Option<&str>) -> bool {
        match scope {
            Some(scope) => scope == self.current_project_scope(),
            None => true,
        }
    }

    fn preflight_key(&self, logical_session: &str) -> String {
        format!("{}::{logical_session}", self.current_project_scope())
    }

    fn clear_recent_preflights(&self) {
        self.recent_preflights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    pub(crate) fn normalize_target_path(&self, path: &str) -> String {
        normalize_path_for_project(self.project().as_path(), path)
    }

    pub(crate) fn extract_target_paths(&self, arguments: &Value) -> Vec<String> {
        let mut targets = Vec::new();

        for key in ["file_path", "relative_path", "path", "target_file"] {
            if let Some(path) = arguments.get(key).and_then(|value| value.as_str()) {
                push_unique_string(&mut targets, self.normalize_target_path(path));
            }
        }

        if let Some(paths) = arguments
            .get("changed_files")
            .and_then(|value| value.as_array())
        {
            for value in paths {
                if let Some(path) = value.as_str() {
                    push_unique_string(&mut targets, self.normalize_target_path(path));
                } else if let Some(path) = value.get("path").and_then(|item| item.as_str()) {
                    push_unique_string(&mut targets, self.normalize_target_path(path));
                }
            }
        }

        targets
    }

    pub(crate) fn extract_symbol_hint(&self, arguments: &Value) -> Option<String> {
        for key in [
            "name_path",
            "symbol",
            "symbol_name",
            "name",
            "function_name",
        ] {
            if let Some(value) = arguments.get(key).and_then(|entry| entry.as_str()) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_owned());
                }
            }
        }
        None
    }

    pub(crate) fn record_recent_preflight_from_payload(
        &self,
        tool_name: &str,
        surface: &str,
        logical_session: &str,
        arguments: &Value,
        payload: &Value,
    ) {
        let readiness = payload
            .get("readiness")
            .cloned()
            .and_then(|value| serde_json::from_value::<AnalysisReadiness>(value).ok())
            .unwrap_or_default();
        let blocker_count = payload
            .get("blocker_count")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| {
                payload
                    .get("blockers")
                    .and_then(|value| value.as_array())
                    .map(|value| value.len())
                    .unwrap_or_default()
            });
        let preflight = RecentPreflight {
            tool_name: tool_name.to_owned(),
            analysis_id: payload
                .get("analysis_id")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
            surface: surface.to_owned(),
            timestamp_ms: Self::now_ms(),
            readiness,
            blocker_count,
            target_paths: self.extract_target_paths(arguments),
            symbol: self.extract_symbol_hint(arguments),
        };
        self.recent_preflights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(self.preflight_key(logical_session), preflight);
    }

    pub(crate) fn recent_preflight(&self, logical_session: &str) -> Option<RecentPreflight> {
        self.recent_preflights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&self.preflight_key(logical_session))
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn set_recent_preflight_timestamp_for_test(
        &self,
        logical_session: &str,
        timestamp_ms: u64,
    ) {
        if let Some(preflight) = self
            .recent_preflights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(&self.preflight_key(logical_session))
        {
            preflight.timestamp_ms = timestamp_ms;
        }
    }

    // ── Active project accessors (check override, fallback to default) ──

    /// Get the active project root. Clones the ProjectRoot (just a PathBuf).
    pub(crate) fn project(&self) -> ProjectRoot {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => o.project.clone(),
            None => self.default_project.clone(),
        }
    }

    /// Get the active symbol index.
    pub(crate) fn symbol_index(&self) -> Arc<SymbolIndex> {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => Arc::clone(&o.symbol_index),
            None => Arc::clone(&self.default_symbol_index),
        }
    }

    pub(crate) fn watcher_failure_health(&self) -> WatcherFailureHealth {
        let symbol_index = self.symbol_index();
        let db = symbol_index.db();
        let summary = db
            .index_failure_summary(WATCHER_RECENT_FAILURE_WINDOW_SECS)
            .unwrap_or_default();
        let scope = self.current_project_scope();
        let pruned_missing_failures = self
            .watcher_maintenance
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&scope)
            .copied()
            .unwrap_or(0);
        WatcherFailureHealth {
            recent_failures: summary.recent_failures,
            total_failures: summary.total_failures,
            stale_failures: summary.stale_failures,
            persistent_failures: summary.persistent_failures,
            pruned_missing_failures,
            recent_window_seconds: WATCHER_RECENT_FAILURE_WINDOW_SECS,
        }
    }

    pub(crate) fn prune_index_failures(&self) -> Result<WatcherFailureHealth, CodeLensError> {
        let project = self.project();
        let scope = self.current_project_scope();
        let symbol_index = self.symbol_index();
        let pruned_missing_failures = {
            let db = symbol_index.db();
            db.prune_missing_index_failures(project.as_path())?
        };
        self.watcher_maintenance
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(scope, pruned_missing_failures);
        Ok(self.watcher_failure_health())
    }

    /// Get the active graph cache.
    pub(crate) fn graph_cache(&self) -> Arc<GraphCache> {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => Arc::clone(&o.graph_cache),
            None => Arc::clone(&self.default_graph_cache),
        }
    }

    /// Get the active memories directory.
    pub(crate) fn memories_dir(&self) -> PathBuf {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => o.memories_dir.clone(),
            None => self.default_memories_dir.clone(),
        }
    }

    /// Get the active analysis cache directory.
    pub(crate) fn analysis_dir(&self) -> PathBuf {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => o.analysis_dir.clone(),
            None => self.default_analysis_dir.clone(),
        }
    }

    pub(crate) fn audit_dir(&self) -> PathBuf {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => o.audit_dir.clone(),
            None => self.default_audit_dir.clone(),
        }
    }

    fn jobs_dir(&self) -> PathBuf {
        self.analysis_dir().join("jobs")
    }

    /// Switch the active project at runtime. Creates a new index and graph cache.
    pub(crate) fn switch_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        let symbol_index = Arc::new(SymbolIndex::new(project.clone()));
        if symbol_index
            .stats()
            .map(|s| s.indexed_files == 0)
            .unwrap_or(true)
        {
            let _ = symbol_index.refresh_all();
        }
        let graph_cache = Arc::new(GraphCache::new(30));
        let memories_dir = project.as_path().join(".codelens").join("memories");
        let analysis_dir = project.as_path().join(".codelens").join("analysis-cache");
        let audit_dir = project.as_path().join(".codelens").join("audit");
        let _ = fs::create_dir_all(&memories_dir);
        let _ = fs::create_dir_all(&analysis_dir);
        let _ = fs::create_dir_all(analysis_dir.join("jobs"));
        let _ = fs::create_dir_all(&audit_dir);
        let watcher = FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        )
        .ok();
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(ProjectOverride {
            project,
            symbol_index,
            graph_cache,
            memories_dir,
            analysis_dir,
            audit_dir,
            watcher,
        });
        self.clear_analysis_handles();
        self.clear_analysis_jobs();
        self.clear_recent_preflights();
        self.cleanup_stale_analysis_dirs(Self::now_ms());
        self.cleanup_stale_job_files(Self::now_ms());
        Ok(name)
    }

    /// Reset to the default project.
    #[allow(dead_code)]
    pub(crate) fn reset_project(&self) {
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.clear_analysis_handles();
        self.clear_analysis_jobs();
        self.clear_recent_preflights();
    }

    /// Check if running on the default project.
    #[allow(dead_code)]
    pub(crate) fn is_default_project(&self) -> bool {
        self.project_override
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_none()
    }

    /// Access the LSP session pool. Pool uses internal per-session locking.
    pub(crate) fn lsp_pool(&self) -> &LspSessionPool {
        &self.lsp_pool
    }

    /// Acquire active tool surface with poison recovery.
    pub(crate) fn surface(&self) -> std::sync::MutexGuard<'_, ToolSurface> {
        self.surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn set_surface(&self, surface: ToolSurface) {
        *self
            .surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = surface;
    }

    pub(crate) fn configure_daemon_mode(&self, daemon_mode: RuntimeDaemonMode) {
        *self
            .daemon_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = daemon_mode;
    }

    pub(crate) fn configure_transport_mode(&self, transport: &str) {
        let mode = RuntimeTransportMode::from_str(transport);
        *self
            .transport_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode;
        self.metrics.record_analysis_worker_pool(
            self.analysis_worker_limit(),
            self.analysis_cost_budget(),
            mode.as_str(),
        );
    }

    pub(crate) fn transport_mode(&self) -> RuntimeTransportMode {
        *self
            .transport_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn daemon_mode(&self) -> RuntimeDaemonMode {
        *self
            .daemon_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn mutation_allowed_in_runtime(&self) -> bool {
        !matches!(self.daemon_mode(), RuntimeDaemonMode::ReadOnly)
    }

    pub(crate) fn analysis_worker_limit(&self) -> usize {
        match self.transport_mode() {
            RuntimeTransportMode::Http => HTTP_ANALYSIS_WORKER_COUNT,
            RuntimeTransportMode::Stdio => STDIO_ANALYSIS_WORKER_COUNT,
        }
    }

    pub(crate) fn analysis_cost_budget(&self) -> usize {
        match self.transport_mode() {
            RuntimeTransportMode::Http => 3,
            RuntimeTransportMode::Stdio => 2,
        }
    }

    pub(crate) fn analysis_parallelism_for_profile(&self, profile_hint: Option<&str>) -> usize {
        let hinted_profile =
            profile_hint
                .and_then(ToolProfile::from_str)
                .or_else(|| match *self.surface() {
                    ToolSurface::Profile(profile) => Some(profile),
                    ToolSurface::Preset(_) => None,
                });
        let transport_limit = self.analysis_worker_limit();
        match hinted_profile {
            Some(ToolProfile::PlannerReadonly)
            | Some(ToolProfile::ReviewerGraph)
            | Some(ToolProfile::CiAudit) => transport_limit.min(HTTP_ANALYSIS_WORKER_COUNT),
            Some(ToolProfile::BuilderMinimal)
            | Some(ToolProfile::EvaluatorCompact)
            | Some(ToolProfile::RefactorFull)
            | None => 1,
        }
    }

    /// Access the tool metrics registry.
    pub(crate) fn metrics(&self) -> &ToolMetricsRegistry {
        self.metrics.as_ref()
    }

    /// Record a tool call in the recent tools ring buffer.
    pub(crate) fn push_recent_tool(&self, name: &str) {
        let mut q = self.recent_tools.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() >= 5 {
            q.pop_front();
        }
        q.push_back(name.to_owned());
    }

    /// Get the recent tool call names (up to 5).
    pub(crate) fn recent_tools(&self) -> Vec<String> {
        self.recent_tools
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    fn sanitize_section_name(section: &str) -> String {
        section
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn analysis_artifact_dir(&self, analysis_id: &str) -> PathBuf {
        self.analysis_dir().join(analysis_id)
    }

    fn analysis_job_path(&self, job_id: &str) -> PathBuf {
        self.jobs_dir().join(format!("{job_id}.json"))
    }

    fn write_analysis_to_disk(
        &self,
        artifact: &AnalysisArtifact,
        sections: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<(), CodeLensError> {
        let artifact_dir = self.analysis_artifact_dir(&artifact.id);
        fs::create_dir_all(&artifact_dir)?;
        let summary_path = artifact_dir.join("summary.json");
        let summary_bytes = serde_json::to_vec_pretty(artifact)
            .map_err(|error| CodeLensError::Internal(error.into()))?;
        fs::write(summary_path, summary_bytes)?;
        for (section, value) in sections {
            let section_path =
                artifact_dir.join(format!("{}.json", Self::sanitize_section_name(section)));
            let section_bytes = serde_json::to_vec_pretty(value)
                .map_err(|error| CodeLensError::Internal(error.into()))?;
            fs::write(section_path, section_bytes)?;
        }
        Ok(())
    }

    fn read_analysis_from_disk(&self, analysis_id: &str) -> Option<AnalysisArtifact> {
        let summary_path = self.analysis_artifact_dir(analysis_id).join("summary.json");
        fs::read(summary_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AnalysisArtifact>(&bytes).ok())
            .filter(|artifact| {
                self.matches_current_project_scope(artifact.project_scope.as_deref())
            })
    }

    fn remove_analysis_from_disk(&self, analysis_id: &str) {
        let _ = fs::remove_dir_all(self.analysis_artifact_dir(analysis_id));
    }

    fn write_job_to_disk(&self, job: &AnalysisJob) -> Result<(), CodeLensError> {
        let jobs_dir = self.jobs_dir();
        fs::create_dir_all(&jobs_dir)?;
        let bytes = serde_json::to_vec_pretty(job)
            .map_err(|error| CodeLensError::Internal(error.into()))?;
        let path = self.analysis_job_path(&job.id);
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, bytes)?;
        fs::rename(tmp_path, path)?;
        Ok(())
    }

    fn remove_job_from_disk(&self, job_id: &str) {
        let _ = fs::remove_file(self.analysis_job_path(job_id));
    }

    fn cleanup_stale_analysis_dirs(&self, now_ms: u64) {
        let entries = match fs::read_dir(self.analysis_dir()) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let is_system_dir = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| matches!(name, "jobs"))
                .unwrap_or(false);
            if is_system_dir {
                continue;
            }
            let summary_path = path.join("summary.json");
            let created_at_ms = fs::read(&summary_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<AnalysisArtifact>(&bytes).ok())
                .map(|artifact| artifact.created_at_ms);
            match created_at_ms {
                Some(created_at_ms) if Self::analysis_expired(created_at_ms, now_ms) => {
                    let _ = fs::remove_dir_all(&path);
                }
                None => {
                    let _ = fs::remove_dir_all(&path);
                }
                _ => {}
            }
        }
    }

    fn list_analysis_ids_on_disk(&self) -> Vec<String> {
        let entries = match fs::read_dir(self.analysis_dir()) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                path.is_dir().then(|| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
            })
            .filter(|name| !name.is_empty() && name != "jobs")
            .collect()
    }

    fn cleanup_stale_job_files(&self, now_ms: u64) {
        let entries = match fs::read_dir(self.jobs_dir()) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let job = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<AnalysisJob>(&bytes).ok());
            match job {
                Some(job)
                    if Self::analysis_expired(job.updated_at_ms, now_ms)
                        || !self.matches_current_project_scope(job.project_scope.as_deref()) =>
                {
                    let _ = fs::remove_file(&path);
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
                _ => {}
            }
        }
    }

    fn prune_analysis_artifacts(&self, now_ms: u64) {
        self.cleanup_stale_analysis_dirs(now_ms);

        let expired_ids = {
            let analyses = self.analyses.lock().unwrap_or_else(|p| p.into_inner());
            analyses
                .iter()
                .filter_map(|(id, artifact)| {
                    Self::analysis_expired(artifact.created_at_ms, now_ms).then(|| id.clone())
                })
                .collect::<Vec<_>>()
        };

        let mut evicted = expired_ids;
        {
            let mut order = self
                .analysis_order
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if !evicted.is_empty() {
                order.retain(|id| !evicted.contains(id));
            }
            while order.len() > MAX_ANALYSIS_ARTIFACTS {
                if let Some(oldest) = order.pop_front() {
                    evicted.push(oldest);
                }
            }
        }

        if evicted.is_empty() {
            return;
        }

        evicted.sort();
        evicted.dedup();
        let mut analyses = self.analyses.lock().unwrap_or_else(|p| p.into_inner());
        for analysis_id in &evicted {
            analyses.remove(analysis_id);
        }
        drop(analyses);
        for analysis_id in evicted {
            self.remove_analysis_from_disk(&analysis_id);
        }
    }

    fn prune_analysis_jobs(&self, now_ms: u64) {
        self.cleanup_stale_job_files(now_ms);

        let expired_ids = {
            let jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
            jobs.iter()
                .filter_map(|(id, job)| {
                    Self::analysis_expired(job.updated_at_ms, now_ms).then(|| id.clone())
                })
                .collect::<Vec<_>>()
        };

        let mut evicted = expired_ids;
        {
            let mut order = self.job_order.lock().unwrap_or_else(|p| p.into_inner());
            if !evicted.is_empty() {
                order.retain(|id| !evicted.contains(id));
            }
            while order.len() > MAX_ANALYSIS_JOBS {
                if let Some(oldest) = order.pop_front() {
                    evicted.push(oldest);
                }
            }
        }

        if evicted.is_empty() {
            return;
        }

        evicted.sort();
        evicted.dedup();
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        for job_id in &evicted {
            jobs.remove(job_id);
        }
        drop(jobs);
        for job_id in evicted {
            self.remove_job_from_disk(&job_id);
        }
    }

    fn clear_analysis_handles(&self) {
        self.analyses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.analysis_order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    fn clear_analysis_jobs(&self) {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner()).clear();
        self.job_order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    pub(crate) fn enqueue_analysis_job(
        &self,
        job_id: String,
        kind: String,
        arguments: Value,
        profile_hint: Option<String>,
    ) -> Result<(), CodeLensError> {
        let (depth, weighted_depth, priority_promoted) = self
            .analysis_queue
            .get_or_init(|| AnalysisWorkerQueue::new(self))
            .enqueue(AnalysisJobRequest {
                job_id,
                cost_units: analysis_job_cost_units(&kind),
                kind,
                arguments,
                profile_hint,
            })?;
        self.metrics
            .record_analysis_job_enqueued(depth, weighted_depth, priority_promoted);
        Ok(())
    }

    pub(crate) fn store_analysis_job(
        &self,
        kind: &str,
        profile_hint: Option<String>,
        estimated_sections: Vec<String>,
        status: &str,
        progress: u8,
        current_step: Option<String>,
        analysis_id: Option<String>,
        error: Option<String>,
    ) -> Result<AnalysisJob, CodeLensError> {
        let now_ms = Self::now_ms();
        let seq = self
            .job_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("job-{now_ms}-{seq}");
        let job = AnalysisJob {
            id: id.clone(),
            kind: kind.to_owned(),
            project_scope: Some(self.current_project_scope()),
            status: status.to_owned(),
            progress,
            current_step,
            profile_hint,
            estimated_sections,
            analysis_id,
            error,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        self.write_job_to_disk(&job)?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), job.clone());
        self.job_order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(id);
        self.prune_analysis_jobs(now_ms);
        Ok(job)
    }

    pub(crate) fn get_analysis_job(&self, job_id: &str) -> Option<AnalysisJob> {
        self.prune_analysis_jobs(Self::now_ms());
        let path = self.analysis_job_path(job_id);
        let job = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AnalysisJob>(&bytes).ok())
            .filter(|job| self.matches_current_project_scope(job.project_scope.as_deref()))
            .or_else(|| {
                self.jobs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(job_id)
                    .cloned()
                    .filter(|job| self.matches_current_project_scope(job.project_scope.as_deref()))
            })?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.to_owned(), job.clone());
        if let Some(analysis_id) = job.analysis_id.as_deref() {
            let _ = self.get_analysis(analysis_id);
        }
        Some(job)
    }

    pub(crate) fn cancel_analysis_job(&self, job_id: &str) -> Result<AnalysisJob, CodeLensError> {
        self.prune_analysis_jobs(Self::now_ms());
        let mut job = self
            .get_analysis_job(job_id)
            .ok_or_else(|| CodeLensError::NotFound(format!("Unknown job `{job_id}`")))?;
        let previous_status = job.status.clone();
        if job.status != "completed" {
            job.status = "cancelled".to_owned();
            job.progress = 0;
            job.current_step = Some("cancelled".to_owned());
            job.updated_at_ms = Self::now_ms();
            if previous_status == "queued" {
                self.metrics.record_analysis_job_cancelled(0, 0);
            }
        }
        self.write_job_to_disk(&job)?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.to_owned(), job.clone());
        Ok(job)
    }

    pub(crate) fn update_analysis_job(
        &self,
        job_id: &str,
        status: Option<&str>,
        progress: Option<u8>,
        current_step: Option<Option<String>>,
        estimated_sections: Option<Vec<String>>,
        analysis_id: Option<Option<String>>,
        error: Option<Option<String>>,
    ) -> Result<AnalysisJob, CodeLensError> {
        let path = self.analysis_job_path(job_id);
        let mut job = self
            .get_analysis_job(job_id)
            .ok_or_else(|| CodeLensError::NotFound(format!("Unknown job `{job_id}`")))?;
        if let Some(status) = status {
            job.status = status.to_owned();
        }
        if let Some(progress) = progress {
            job.progress = progress;
        }
        if let Some(current_step) = current_step {
            job.current_step = current_step;
        }
        if let Some(estimated_sections) = estimated_sections {
            job.estimated_sections = estimated_sections;
        }
        if let Some(analysis_id) = analysis_id {
            job.analysis_id = analysis_id;
        }
        if let Some(error) = error {
            job.error = error;
        }
        job.updated_at_ms = Self::now_ms();
        self.write_job_to_disk(&job)?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.to_owned(), job.clone());
        if !path.exists() {
            self.job_order
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push_back(job_id.to_owned());
        }
        Ok(job)
    }

    pub(crate) fn record_mutation_audit(
        &self,
        tool: &str,
        surface: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), CodeLensError> {
        let audit_dir = self.audit_dir();
        fs::create_dir_all(&audit_dir)?;
        let path = audit_dir.join("mutation-audit.jsonl");
        let session_id = arguments
            .get("_session_id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let session_trusted_client = arguments
            .get("_session_trusted_client")
            .and_then(|value| value.as_bool());
        let session_requested_profile = arguments
            .get("_session_requested_profile")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let session_client_name = arguments
            .get("_session_client_name")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let session_client_version = arguments
            .get("_session_client_version")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let scrubbed_arguments = match arguments {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .filter(|(key, _)| !key.starts_with("_session_"))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
            other => other.clone(),
        };
        let event = serde_json::json!({
            "timestamp_ms": Self::now_ms(),
            "project_scope": self.current_project_scope(),
            "surface": surface,
            "daemon_mode": self.daemon_mode().as_str(),
            "tool": tool,
            "arguments": scrubbed_arguments,
            "session": {
                "id": session_id,
                "trusted_client": session_trusted_client,
                "requested_profile": session_requested_profile,
                "client_name": session_client_name,
                "client_version": session_client_version,
            },
        });
        let mut line =
            serde_json::to_string(&event).map_err(|error| CodeLensError::Internal(error.into()))?;
        line.push('\n');
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    pub(crate) fn store_analysis(
        &self,
        tool_name: &str,
        cache_key: Option<String>,
        summary: String,
        top_findings: Vec<String>,
        risk_level: String,
        confidence: f64,
        next_actions: Vec<String>,
        blockers: Vec<String>,
        readiness: AnalysisReadiness,
        verifier_checks: Vec<AnalysisVerifierCheck>,
        sections: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<AnalysisArtifact, CodeLensError> {
        let available_sections = sections.keys().cloned().collect::<Vec<_>>();
        let created_at_ms = Self::now_ms();
        let seq = self
            .analysis_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("analysis-{created_at_ms}-{seq}");
        let artifact = AnalysisArtifact {
            id: id.clone(),
            tool_name: tool_name.to_owned(),
            surface: self.surface().as_label().to_owned(),
            project_scope: Some(self.current_project_scope()),
            cache_key,
            summary,
            top_findings,
            risk_level,
            confidence,
            next_actions,
            blockers,
            readiness,
            verifier_checks,
            available_sections,
            created_at_ms,
        };
        self.write_analysis_to_disk(&artifact, &sections)?;
        {
            let mut analyses = self.analyses.lock().unwrap_or_else(|p| p.into_inner());
            analyses.insert(id.clone(), artifact.clone());
        }
        {
            let mut order = self
                .analysis_order
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            order.push_back(id.clone());
        }
        self.prune_analysis_artifacts(created_at_ms);
        Ok(artifact)
    }

    pub(crate) fn find_reusable_analysis(
        &self,
        tool_name: &str,
        cache_key: &str,
    ) -> Option<AnalysisArtifact> {
        self.prune_analysis_artifacts(Self::now_ms());
        for analysis_id in self.list_analysis_ids_on_disk() {
            let _ = self.get_analysis(&analysis_id);
        }
        let active_surface = self.surface().as_label().to_owned();
        let order = self
            .analysis_order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        let analyses = self.analyses.lock().unwrap_or_else(|p| p.into_inner());
        order.into_iter().find_map(|id| {
            let artifact = analyses.get(&id)?;
            if artifact.tool_name == tool_name
                && artifact.surface == active_surface
                && self.matches_current_project_scope(artifact.project_scope.as_deref())
                && artifact.cache_key.as_deref() == Some(cache_key)
            {
                Some(artifact.clone())
            } else {
                None
            }
        })
    }

    pub(crate) fn get_analysis(&self, analysis_id: &str) -> Option<AnalysisArtifact> {
        self.prune_analysis_artifacts(Self::now_ms());
        if let Some(artifact) = self
            .analyses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(analysis_id)
            .cloned()
            .filter(|artifact| {
                self.matches_current_project_scope(artifact.project_scope.as_deref())
            })
        {
            return Some(artifact);
        }
        let artifact = self.read_analysis_from_disk(analysis_id)?;
        self.analyses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(analysis_id.to_owned(), artifact.clone());
        let mut order = self
            .analysis_order
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !order.iter().any(|existing| existing == analysis_id) {
            order.push_back(analysis_id.to_owned());
        }
        Some(artifact)
    }

    pub(crate) fn list_analysis_summaries(&self) -> Vec<AnalysisSummary> {
        self.prune_analysis_artifacts(Self::now_ms());
        for analysis_id in self.list_analysis_ids_on_disk() {
            let _ = self.get_analysis(&analysis_id);
        }
        let order = self
            .analysis_order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let analyses = self.analyses.lock().unwrap_or_else(|p| p.into_inner());
        order
            .iter()
            .rev()
            .filter_map(|id| analyses.get(id))
            .map(|artifact| AnalysisSummary {
                id: artifact.id.clone(),
                tool_name: artifact.tool_name.clone(),
                summary: artifact.summary.clone(),
                surface: artifact.surface.clone(),
                created_at_ms: artifact.created_at_ms,
            })
            .collect()
    }

    pub(crate) fn get_analysis_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.prune_analysis_artifacts(Self::now_ms());
        self.metrics.record_analysis_read(true);
        let section_path = self
            .analysis_artifact_dir(analysis_id)
            .join(format!("{}.json", Self::sanitize_section_name(section)));
        let bytes = fs::read(&section_path)?;
        serde_json::from_slice(&bytes).map_err(|error| CodeLensError::Internal(error.into()))
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

    pub(crate) fn clone_for_worker(&self) -> Self {
        let project = self.project();
        let symbol_index = self.symbol_index();
        let graph_cache = self.graph_cache();
        let memories_dir = self.memories_dir();
        let analysis_dir = self.analysis_dir();
        let audit_dir = self.audit_dir();
        Self {
            default_project: project.clone(),
            default_symbol_index: symbol_index,
            default_graph_cache: graph_cache,
            default_memories_dir: memories_dir,
            default_analysis_dir: analysis_dir,
            default_audit_dir: audit_dir,
            project_override: std::sync::RwLock::new(None),
            lsp_pool: LspSessionPool::new(project),
            transport_mode: Mutex::new(self.transport_mode()),
            daemon_mode: Mutex::new(self.daemon_mode()),
            surface: Mutex::new(*self.surface()),
            token_budget: std::sync::atomic::AtomicUsize::new(self.token_budget()),
            analysis_seq: std::sync::atomic::AtomicU64::new(0),
            job_seq: std::sync::atomic::AtomicU64::new(0),
            metrics: Arc::clone(&self.metrics),
            recent_tools: Mutex::new(VecDeque::with_capacity(5)),
            recent_preflights: Mutex::new(HashMap::new()),
            analysis_order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_ARTIFACTS)),
            analyses: Mutex::new(HashMap::new()),
            job_order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_JOBS)),
            jobs: Mutex::new(HashMap::new()),
            analysis_queue: OnceLock::new(),
            watcher: None,
            watcher_maintenance: Mutex::new(HashMap::new()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
        }
    }

    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let symbol_index = Arc::new(SymbolIndex::new(project.clone()));
        // Auto-index on startup if DB is empty — ensures zero-config first use.
        if symbol_index
            .stats()
            .map(|s| s.indexed_files == 0)
            .unwrap_or(true)
        {
            let _ = symbol_index.refresh_all();
        }
        let lsp_pool = LspSessionPool::new(project.clone());
        let graph_cache = Arc::new(GraphCache::new(30));
        let memories_dir = project.as_path().join(".codelens").join("memories");
        let analysis_dir = project.as_path().join(".codelens").join("analysis-cache");
        let audit_dir = project.as_path().join(".codelens").join("audit");
        let _ = fs::create_dir_all(&memories_dir);
        let _ = fs::create_dir_all(&analysis_dir);
        let _ = fs::create_dir_all(analysis_dir.join("jobs"));
        let _ = fs::create_dir_all(&audit_dir);

        let watcher = FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        )
        .ok();

        let state = Self {
            default_project: project,
            default_symbol_index: symbol_index,
            lsp_pool,
            default_graph_cache: graph_cache,
            default_memories_dir: memories_dir,
            default_analysis_dir: analysis_dir,
            default_audit_dir: audit_dir,
            project_override: std::sync::RwLock::new(None),
            transport_mode: Mutex::new(RuntimeTransportMode::Stdio),
            daemon_mode: Mutex::new(RuntimeDaemonMode::Standard),
            surface: Mutex::new(ToolSurface::Preset(preset)),
            token_budget: std::sync::atomic::AtomicUsize::new(
                crate::tool_defs::default_budget_for_preset(preset),
            ),
            analysis_seq: std::sync::atomic::AtomicU64::new(0),
            job_seq: std::sync::atomic::AtomicU64::new(0),
            metrics: Arc::new(ToolMetricsRegistry::new()),
            recent_tools: Mutex::new(VecDeque::with_capacity(5)),
            recent_preflights: Mutex::new(HashMap::new()),
            analysis_order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_ARTIFACTS)),
            analyses: Mutex::new(HashMap::new()),
            job_order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_JOBS)),
            jobs: Mutex::new(HashMap::new()),
            analysis_queue: OnceLock::new(),
            watcher,
            watcher_maintenance: Mutex::new(HashMap::new()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
        };
        state.configure_transport_mode("stdio");
        state.cleanup_stale_analysis_dirs(Self::now_ms());
        state.cleanup_stale_job_files(Self::now_ms());
        state
    }

    /// Register a secondary project for cross-project queries.
    pub(crate) fn add_secondary_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        let index = Arc::new(SymbolIndex::new(project.clone()));
        // Ensure it's indexed
        index.refresh_all()?;
        let mut map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.insert(name.clone(), SecondaryProject { project, index });
        Ok(name)
    }

    /// Remove a secondary project.
    pub(crate) fn remove_secondary_project(&self, name: &str) -> bool {
        let mut map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.remove(name).is_some()
    }

    /// Get a snapshot of secondary project names and paths.
    pub(crate) fn list_secondary_projects(&self) -> Vec<(String, String)> {
        let map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.iter()
            .map(|(name, sp)| {
                (
                    name.clone(),
                    sp.project.as_path().to_string_lossy().to_string(),
                )
            })
            .collect()
    }

    /// Query symbols in a secondary project by name.
    pub(crate) fn query_secondary_project(
        &self,
        project_name: &str,
        symbol_name: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<codelens_core::SymbolInfo>> {
        let map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let sp = map
            .get(project_name)
            .ok_or_else(|| anyhow::anyhow!("project '{}' not registered", project_name))?;
        sp.index
            .find_symbol(symbol_name, None, false, false, max_results)
    }

    /// Initialize the session store for HTTP mode.
    #[cfg(feature = "http")]
    pub(crate) fn with_session_store(mut self) -> Self {
        self.session_store = Some(crate::server::session::SessionStore::new(
            std::time::Duration::from_secs(30 * 60), // 30 minutes
        ));
        self
    }

    #[cfg(feature = "http")]
    pub(crate) fn active_session_count(&self) -> usize {
        self.session_store
            .as_ref()
            .map(|store| store.len())
            .unwrap_or(0)
    }

    #[cfg(feature = "http")]
    pub(crate) fn session_timeout_seconds(&self) -> u64 {
        self.session_store
            .as_ref()
            .map(|store| store.timeout_secs())
            .unwrap_or(0)
    }

    #[cfg(feature = "http")]
    pub(crate) fn session_resume_supported(&self) -> bool {
        self.session_store.is_some()
    }

    #[cfg(not(feature = "http"))]
    pub(crate) fn active_session_count(&self) -> usize {
        0
    }

    #[cfg(not(feature = "http"))]
    pub(crate) fn session_timeout_seconds(&self) -> u64 {
        0
    }

    #[cfg(not(feature = "http"))]
    pub(crate) fn session_resume_supported(&self) -> bool {
        false
    }

    #[cfg(test)]
    pub(crate) fn set_analysis_created_at_for_test(
        &self,
        analysis_id: &str,
        created_at_ms: u64,
    ) -> Result<(), CodeLensError> {
        if let Some(artifact) = self
            .analyses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(analysis_id)
        {
            artifact.created_at_ms = created_at_ms;
        }
        let summary_path = self.analysis_artifact_dir(analysis_id).join("summary.json");
        let bytes = fs::read(&summary_path)?;
        let mut artifact: AnalysisArtifact = serde_json::from_slice(&bytes)
            .map_err(|error| CodeLensError::Internal(error.into()))?;
        artifact.created_at_ms = created_at_ms;
        let updated = serde_json::to_vec_pretty(&artifact)
            .map_err(|error| CodeLensError::Internal(error.into()))?;
        fs::write(summary_path, updated)?;
        Ok(())
    }
}
