#[cfg(feature = "semantic")]
use codelens_core::EmbeddingEngine;
use codelens_core::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::analysis_queue::{
    analysis_job_cost_units, AnalysisJobRequest, AnalysisWorkerQueue, HTTP_ANALYSIS_WORKER_COUNT,
    STDIO_ANALYSIS_WORKER_COUNT,
};
use crate::artifact_store::AnalysisArtifactStore;
use crate::error::CodeLensError;
use crate::mutation_audit;
use crate::preflight_store::RecentPreflightStore;
use crate::runtime_types::JobLifecycle;
use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface};
use serde_json::Value;
use std::collections::VecDeque;

const WATCHER_RECENT_FAILURE_WINDOW_SECS: i64 = 15 * 60;
/// Default preflight TTL: 10 minutes. Override via CODELENS_PREFLIGHT_TTL_SECS.
/// NLAH (arxiv:2603.25723): overly strict verifiers hurt performance by -0.8~-8.4%.
/// Making TTL configurable lets agents tune verification overhead vs safety.
pub(crate) fn preflight_ttl_ms() -> u64 {
    std::env::var("CODELENS_PREFLIGHT_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|secs| secs * 1000)
        .unwrap_or(10 * 60 * 1000) // default 10 min
}

pub(crate) use crate::client_profile::ClientProfile;
pub(crate) use crate::runtime_types::{
    AnalysisArtifact, AnalysisJob, AnalysisReadiness, AnalysisSummary, AnalysisVerifierCheck,
    RecentPreflight, RuntimeDaemonMode, RuntimeTransportMode, WatcherFailureHealth,
};

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

// ── Application state ──────────────────────────────────────────────────

/// Holds project-specific resources that can be swapped at runtime.
struct ProjectOverride {
    project: ProjectRoot,
    symbol_index: Arc<SymbolIndex>,
    graph_cache: Arc<GraphCache>,
    memories_dir: PathBuf,
    #[allow(dead_code)]
    analysis_dir: PathBuf,
    audit_dir: PathBuf,
    #[allow(dead_code)]
    watcher: Option<FileWatcher>,
}

pub(crate) struct AppState {
    // Default project (set at startup, immutable)
    default_project: ProjectRoot,
    default_symbol_index: Arc<SymbolIndex>,
    default_graph_cache: Arc<GraphCache>,
    default_memories_dir: PathBuf,
    default_audit_dir: PathBuf,
    // Runtime project override (set by activate_project)
    project_override: std::sync::RwLock<Option<ProjectOverride>>,
    lsp_pool: std::sync::RwLock<LspSessionPool>,
    transport_mode: Mutex<RuntimeTransportMode>,
    daemon_mode: Mutex<RuntimeDaemonMode>,
    client_profile: ClientProfile,
    surface: Mutex<ToolSurface>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    pub(crate) token_budget: std::sync::atomic::AtomicUsize,
    artifact_store: AnalysisArtifactStore,
    job_store: crate::job_store::AnalysisJobStore,
    pub(crate) metrics: Arc<ToolMetricsRegistry>,
    /// Recent tool call names for context-aware suggestions (max 5).
    recent_tools: Mutex<VecDeque<String>>,
    /// Recent file paths accessed in this session (max 20) for ranking boost.
    recent_files: Mutex<VecDeque<String>>,
    /// Recent analysis IDs for cross-phase context (max 5).
    recent_analysis_ids: Mutex<VecDeque<String>>,
    /// Doom-loop detection: (tool_name, args_hash, consecutive_count)
    doom_loop_counter: Mutex<(String, u64, usize)>,
    preflight_store: RecentPreflightStore,
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

    pub(crate) fn current_project_scope(&self) -> String {
        self.project().as_path().to_string_lossy().to_string()
    }

    pub(crate) fn preflight_ttl_seconds(&self) -> u64 {
        preflight_ttl_ms() / 1000
    }

    fn preflight_key(&self, logical_session: &str) -> String {
        RecentPreflightStore::key(&self.current_project_scope(), logical_session)
    }

    fn clear_recent_preflights(&self) {
        self.preflight_store.clear();
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
        let key = self.preflight_key(logical_session);
        self.preflight_store.record_from_payload(
            key,
            tool_name,
            surface,
            Self::now_ms(),
            self.extract_target_paths(arguments),
            self.extract_symbol_hint(arguments),
            payload,
        );
    }

    pub(crate) fn recent_preflight(&self, logical_session: &str) -> Option<RecentPreflight> {
        self.preflight_store
            .get(&self.preflight_key(logical_session))
    }

    #[cfg(test)]
    pub(crate) fn set_recent_preflight_timestamp_for_test(
        &self,
        logical_session: &str,
        timestamp_ms: u64,
    ) {
        self.preflight_store
            .set_timestamp_for_test(&self.preflight_key(logical_session), timestamp_ms);
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
        self.artifact_store.analysis_dir().to_path_buf()
    }

    #[allow(dead_code)]
    pub(crate) fn artifact_store(&self) -> &AnalysisArtifactStore {
        &self.artifact_store
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
        let lsp_pool_new = LspSessionPool::new(project.clone());
        let watcher = FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        )
        .ok();
        // Reset LSP pool to use the new project root — fixes stale path resolution.
        *self.lsp_pool.write().unwrap_or_else(|p| p.into_inner()) = lsp_pool_new;
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(ProjectOverride {
            project,
            symbol_index,
            graph_cache,
            memories_dir,
            analysis_dir: analysis_dir.clone(),
            audit_dir,
            watcher,
        });
        self.artifact_store.set_analysis_dir(analysis_dir.clone());
        self.job_store.set_jobs_dir(analysis_dir.join("jobs"));
        self.artifact_store.clear();
        self.job_store.clear();
        self.clear_recent_preflights();
        self.artifact_store.cleanup_stale_dirs(Self::now_ms());
        let scope = self.current_project_scope();
        self.job_store
            .cleanup_stale_files(Self::now_ms(), Some(&scope));
        Ok(name)
    }

    /// Reset to the default project.
    #[allow(dead_code)]
    pub(crate) fn reset_project(&self) {
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.artifact_store.clear();
        self.job_store.clear();
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
    pub(crate) fn lsp_pool(&self) -> std::sync::RwLockReadGuard<'_, LspSessionPool> {
        self.lsp_pool.read().unwrap_or_else(|p| p.into_inner())
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

    pub(crate) fn client_profile(&self) -> ClientProfile {
        self.client_profile
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

    /// Doom-loop detection: returns the repeat count if the same tool+args_hash
    /// has been called consecutively. Threshold of 3 triggers a warning.
    pub(crate) fn doom_loop_count(&self, name: &str, args_hash: u64) -> usize {
        let mut counter = self
            .doom_loop_counter
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if counter.0 == name && counter.1 == args_hash {
            counter.2 += 1;
        } else {
            *counter = (name.to_owned(), args_hash, 1);
        }
        counter.2
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

    /// Record a file path as recently accessed (for ranking boost).
    pub(crate) fn record_file_access(&self, path: &str) {
        let mut files = self.recent_files.lock().unwrap_or_else(|p| p.into_inner());
        // Deduplicate: remove if already present, then push to back
        files.retain(|f| f != path);
        if files.len() >= 20 {
            files.pop_front();
        }
        files.push_back(path.to_owned());
    }

    /// Get recently accessed file paths (most recent last).
    pub(crate) fn recent_file_paths(&self) -> Vec<String> {
        self.recent_files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// Record an analysis_id for cross-phase context.
    pub(crate) fn push_recent_analysis_id(&self, id: String) {
        let mut ids = self
            .recent_analysis_ids
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if ids.len() >= 5 {
            ids.pop_front();
        }
        ids.push_back(id);
    }

    /// Get recent analysis IDs (most recent last).
    pub(crate) fn recent_analysis_ids(&self) -> Vec<String> {
        self.recent_analysis_ids
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    // ── Job Store delegations ────────────────────────────────────────────

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
        status: JobLifecycle,
        progress: u8,
        current_step: Option<String>,
        analysis_id: Option<String>,
        error: Option<String>,
    ) -> Result<AnalysisJob, CodeLensError> {
        self.job_store.store(
            kind,
            profile_hint,
            estimated_sections,
            status,
            progress,
            current_step,
            analysis_id,
            error,
            self.current_project_scope(),
        )
    }

    pub(crate) fn list_analysis_jobs(&self, status_filter: Option<&str>) -> Vec<AnalysisJob> {
        let scope = self.current_project_scope();
        self.job_store.list(status_filter, Some(&scope))
    }

    pub(crate) fn get_analysis_job(&self, job_id: &str) -> Option<AnalysisJob> {
        let scope = self.current_project_scope();
        let job = self.job_store.get(job_id, Some(&scope))?;
        // Cross-concern: warm artifact cache when job references an analysis
        if let Some(analysis_id) = job.analysis_id.as_deref() {
            let _ = self.get_analysis(analysis_id);
        }
        Some(job)
    }

    pub(crate) fn cancel_analysis_job(&self, job_id: &str) -> Result<AnalysisJob, CodeLensError> {
        let scope = self.current_project_scope();
        let job = self.job_store.cancel(job_id, Some(&scope))?;
        if job.status == JobLifecycle::Cancelled {
            self.metrics.record_analysis_job_cancelled(0, 0);
        }
        Ok(job)
    }

    pub(crate) fn update_analysis_job(
        &self,
        job_id: &str,
        status: Option<JobLifecycle>,
        progress: Option<u8>,
        current_step: Option<Option<String>>,
        estimated_sections: Option<Vec<String>>,
        analysis_id: Option<Option<String>>,
        error: Option<Option<String>>,
    ) -> Result<AnalysisJob, CodeLensError> {
        let scope = self.current_project_scope();
        self.job_store.update(
            job_id,
            status,
            progress,
            current_step,
            estimated_sections,
            analysis_id,
            error,
            Some(&scope),
        )
    }

    pub(crate) fn record_mutation_audit(
        &self,
        tool: &str,
        surface: &str,
        arguments: &serde_json::Value,
        session: &crate::session_context::SessionRequestContext,
    ) -> Result<(), CodeLensError> {
        mutation_audit::record_mutation_audit(
            &self.audit_dir(),
            Self::now_ms(),
            &self.current_project_scope(),
            self.daemon_mode().as_str(),
            surface,
            tool,
            arguments,
            session,
        )
    }

    // ── Artifact Store delegations ────────────────────────────────────────

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
        let artifact = self.artifact_store.store(
            tool_name,
            self.surface().as_label(),
            self.current_project_scope(),
            cache_key,
            summary,
            top_findings,
            risk_level,
            confidence,
            next_actions,
            blockers,
            readiness,
            verifier_checks,
            sections,
        )?;
        // Cross-phase context: track recent analysis IDs so subsequent
        // tool calls can reference prior analysis results.
        self.push_recent_analysis_id(artifact.id.clone());
        Ok(artifact)
    }

    pub(crate) fn find_reusable_analysis(
        &self,
        tool_name: &str,
        cache_key: &str,
    ) -> Option<AnalysisArtifact> {
        let scope = self.current_project_scope();
        self.artifact_store.find_reusable(
            tool_name,
            cache_key,
            self.surface().as_label(),
            Some(&scope),
        )
    }

    pub(crate) fn get_analysis(&self, analysis_id: &str) -> Option<AnalysisArtifact> {
        let scope = self.current_project_scope();
        self.artifact_store.get(analysis_id, Some(&scope))
    }

    pub(crate) fn list_analysis_summaries(&self) -> Vec<AnalysisSummary> {
        let scope = self.current_project_scope();
        self.artifact_store.list_summaries(Some(&scope))
    }

    pub(crate) fn get_analysis_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.metrics.record_analysis_read(true);
        self.artifact_store.get_section(analysis_id, section)
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
            default_audit_dir: audit_dir,
            project_override: std::sync::RwLock::new(None),
            lsp_pool: std::sync::RwLock::new(LspSessionPool::new(project)),
            transport_mode: Mutex::new(self.transport_mode()),
            daemon_mode: Mutex::new(self.daemon_mode()),
            client_profile: self.client_profile,
            surface: Mutex::new(*self.surface()),
            token_budget: std::sync::atomic::AtomicUsize::new(self.token_budget()),
            artifact_store: AnalysisArtifactStore::new(analysis_dir.clone()),
            job_store: crate::job_store::AnalysisJobStore::new(analysis_dir.join("jobs")),
            metrics: Arc::clone(&self.metrics),
            recent_tools: Mutex::new(VecDeque::with_capacity(5)),
            recent_analysis_ids: Mutex::new(VecDeque::with_capacity(5)),
            doom_loop_counter: Mutex::new((String::new(), 0, 0)),
            recent_files: Mutex::new(VecDeque::with_capacity(20)),
            preflight_store: RecentPreflightStore::new(),
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
        let lsp_pool = std::sync::RwLock::new(LspSessionPool::new(project.clone()));
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

        let state = Self::build(
            project,
            symbol_index,
            lsp_pool,
            graph_cache,
            memories_dir,
            analysis_dir,
            audit_dir,
            preset,
            watcher,
        );
        state.configure_transport_mode("stdio");
        state.artifact_store.cleanup_stale_dirs(Self::now_ms());
        let scope = state.current_project_scope();
        state
            .job_store
            .cleanup_stale_files(Self::now_ms(), Some(&scope));
        state
    }

    /// Lightweight constructor that skips file watcher and stale-file cleanup.
    /// Reduces thread/I/O pressure when many instances run in parallel (e.g. tests).
    #[cfg(test)]
    pub(crate) fn new_minimal(project: ProjectRoot, preset: ToolPreset) -> Self {
        let symbol_index = Arc::new(SymbolIndex::new(project.clone()));
        if symbol_index
            .stats()
            .map(|s| s.indexed_files == 0)
            .unwrap_or(true)
        {
            let _ = symbol_index.refresh_all();
        }
        let lsp_pool = std::sync::RwLock::new(LspSessionPool::new(project.clone()));
        let graph_cache = Arc::new(GraphCache::new(30));
        let memories_dir = project.as_path().join(".codelens").join("memories");
        let analysis_dir = project.as_path().join(".codelens").join("analysis-cache");
        let audit_dir = project.as_path().join(".codelens").join("audit");
        let _ = fs::create_dir_all(&memories_dir);
        let _ = fs::create_dir_all(&analysis_dir);
        let _ = fs::create_dir_all(analysis_dir.join("jobs"));
        let _ = fs::create_dir_all(&audit_dir);

        let state = Self::build(
            project,
            symbol_index,
            lsp_pool,
            graph_cache,
            memories_dir,
            analysis_dir,
            audit_dir,
            preset,
            None,
        );
        state.configure_transport_mode("stdio");
        state
    }

    fn build(
        project: ProjectRoot,
        symbol_index: Arc<SymbolIndex>,
        lsp_pool: std::sync::RwLock<LspSessionPool>,
        graph_cache: Arc<GraphCache>,
        memories_dir: PathBuf,
        analysis_dir: PathBuf,
        audit_dir: PathBuf,
        preset: ToolPreset,
        watcher: Option<FileWatcher>,
    ) -> Self {
        Self {
            default_project: project,
            default_symbol_index: symbol_index,
            lsp_pool,
            default_graph_cache: graph_cache,
            default_memories_dir: memories_dir,
            default_audit_dir: audit_dir,
            project_override: std::sync::RwLock::new(None),
            transport_mode: Mutex::new(RuntimeTransportMode::Stdio),
            daemon_mode: Mutex::new(RuntimeDaemonMode::Standard),
            client_profile: ClientProfile::detect(None),
            surface: Mutex::new(ToolSurface::Preset(preset)),
            token_budget: std::sync::atomic::AtomicUsize::new(
                crate::tool_defs::default_budget_for_preset(preset),
            ),
            artifact_store: AnalysisArtifactStore::new(analysis_dir.clone()),
            job_store: crate::job_store::AnalysisJobStore::new(analysis_dir.join("jobs")),
            metrics: Arc::new(ToolMetricsRegistry::new()),
            recent_tools: Mutex::new(VecDeque::with_capacity(5)),
            recent_analysis_ids: Mutex::new(VecDeque::with_capacity(5)),
            doom_loop_counter: Mutex::new((String::new(), 0, 0)),
            recent_files: Mutex::new(VecDeque::with_capacity(20)),
            preflight_store: RecentPreflightStore::new(),
            analysis_queue: OnceLock::new(),
            watcher,
            watcher_maintenance: Mutex::new(HashMap::new()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
        }
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
        self.artifact_store
            .set_created_at_for_test(analysis_id, created_at_ms)
            .map_err(|e| CodeLensError::Internal(e.into()))
    }
}
