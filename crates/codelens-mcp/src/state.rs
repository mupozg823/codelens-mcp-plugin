#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::analysis_queue::{
    AnalysisWorkerQueue, HTTP_ANALYSIS_WORKER_COUNT, STDIO_ANALYSIS_WORKER_COUNT,
};
use crate::artifact_store::AnalysisArtifactStore;
use crate::error::CodeLensError;
use crate::preflight_store::RecentPreflightStore;
use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface};
use serde_json::Value;

mod analysis;
mod preflight;
mod project_runtime;
mod session_runtime;
mod watcher_health;

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

pub(crate) use crate::client_profile::{ClientProfile, EffortLevel};
pub(crate) use crate::runtime_types::{
    AnalysisArtifact, AnalysisJob, AnalysisReadiness, AnalysisVerifierCheck, RuntimeDaemonMode,
    RuntimeTransportMode, WatcherFailureHealth,
};

pub(super) fn push_unique_string(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

pub(super) fn normalize_path_for_project(project_root: &Path, path: &str) -> String {
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

const PROJECT_CONTEXT_CACHE_LIMIT: usize = 4;

use self::project_runtime::{ProjectContextCache, ProjectRuntimeContext};

pub(crate) struct AppState {
    // Default project (set at startup, immutable)
    default_project: ProjectRoot,
    default_symbol_index: Arc<SymbolIndex>,
    default_graph_cache: Arc<GraphCache>,
    default_lsp_pool: Arc<LspSessionPool>,
    default_memories_dir: PathBuf,
    default_analysis_dir: PathBuf,
    default_audit_dir: PathBuf,
    default_watcher: Option<FileWatcher>,
    // Runtime project override (set by activate_project)
    project_override: std::sync::RwLock<Option<Arc<ProjectRuntimeContext>>>,
    project_context_cache: Mutex<ProjectContextCache>,
    transport_mode: Mutex<RuntimeTransportMode>,
    daemon_mode: Mutex<RuntimeDaemonMode>,
    client_profile: ClientProfile,
    effort_level: std::sync::atomic::AtomicU8,
    surface: Mutex<ToolSurface>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    pub(crate) token_budget: std::sync::atomic::AtomicUsize,
    artifact_store: AnalysisArtifactStore,
    job_store: crate::job_store::AnalysisJobStore,
    pub(crate) metrics: Arc<ToolMetricsRegistry>,
    /// Recent tool call names for context-aware suggestions (max 5).
    recent_tools: crate::recent_buffer::RecentRingBuffer,
    /// Recent file paths accessed in this session (max 20) for ranking boost.
    recent_files: crate::recent_buffer::RecentRingBuffer,
    /// Recent analysis IDs for cross-phase context (max 5).
    recent_analysis_ids: crate::recent_buffer::RecentRingBuffer,
    /// Doom-loop detection: (tool_name, args_hash, consecutive_count, first_occurrence_ms)
    doom_loop_counter: Mutex<(String, u64, usize, u64)>,
    preflight_store: RecentPreflightStore,
    analysis_queue: OnceLock<AnalysisWorkerQueue>,
    watcher_maintenance: Mutex<HashMap<String, usize>>,
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    project_execution_lock: Mutex<()>,
    #[cfg(feature = "semantic")]
    pub(crate) embedding: std::sync::RwLock<Option<EmbeddingEngine>>,
    /// Lazy-loaded SCIP precise backend, cached after first access.
    #[cfg(feature = "scip-backend")]
    scip_backend: OnceLock<Option<Arc<codelens_engine::ScipBackend>>>,
    /// Secondary (read-only) project indexes for cross-project queries.
    pub(crate) secondary_projects: Mutex<HashMap<String, SecondaryProject>>,
    #[cfg(feature = "http")]
    pub(crate) session_store: Option<crate::server::session::SessionStore>,
    /// Phase 4b (§capability-reporting follow-up): wall-clock time
    /// when the daemon started, as an RFC 3339 UTC string. Exposed
    /// by `get_capabilities` alongside `binary_build_time` so
    /// downstream tooling can detect "daemon is running an image
    /// older than the disk binary" — the Phase 4a failure mode.
    daemon_started_at: String,
}

/// A read-only project registered for cross-project queries.
pub(crate) struct SecondaryProject {
    pub project: ProjectRoot,
    pub index: Arc<SymbolIndex>,
}

/// Phase 4b (§capability-reporting follow-up): format the current
/// wall-clock time as an RFC 3339 UTC string
/// (`YYYY-MM-DDTHH:MM:SSZ`). Used to stamp `daemon_started_at` at
/// `AppState::build` so `get_capabilities` can report how long the
/// daemon has been alive vs the binary's own build timestamp.
///
/// Pure integer arithmetic, no `chrono` dependency — mirrors the
/// same algorithm as `build.rs::format_iso8601_utc`.
fn now_rfc3339_utc() -> String {
    let unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (unix_seconds / 86_400) as i64;
    let secs_in_day = unix_seconds % 86_400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + (if m <= 2 { 1 } else { 0 });
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

impl AppState {
    /// RFC 3339 UTC timestamp when the daemon started — captured
    /// once at `AppState::build` and inherited by worker clones.
    /// Phase 4b (§capability-reporting): exposed in
    /// `get_capabilities` so downstream callers can detect "daemon
    /// has been running since before the binary was rebuilt".
    pub(crate) fn daemon_started_at(&self) -> &str {
        &self.daemon_started_at
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub(crate) fn current_project_scope(&self) -> String {
        self.project().as_path().to_string_lossy().to_string()
    }

    pub(crate) fn project_scope_for_session(
        &self,
        session: &crate::session_context::SessionRequestContext,
    ) -> String {
        session
            .project_path
            .clone()
            .unwrap_or_else(|| self.current_project_scope())
    }

    pub(crate) fn project_scope_for_arguments(&self, arguments: &Value) -> String {
        let session = crate::session_context::SessionRequestContext::from_json(arguments);
        self.project_scope_for_session(&session)
    }

    fn default_project_scope(&self) -> String {
        self.default_project.as_path().to_string_lossy().to_string()
    }

    fn active_project_context(&self) -> Option<Arc<ProjectRuntimeContext>> {
        project_runtime::active_project_context(self)
    }

    fn build_project_runtime_context(
        project: ProjectRoot,
        start_watcher: bool,
    ) -> anyhow::Result<ProjectRuntimeContext> {
        project_runtime::build_project_runtime_context(project, start_watcher)
    }

    fn activate_project_context(&self, context: Option<Arc<ProjectRuntimeContext>>) {
        project_runtime::activate_project_context(self, context)
    }

    pub(crate) fn execution_surface(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> ToolSurface {
        session_runtime::execution_surface(self, _session)
    }

    pub(crate) fn execution_token_budget(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> usize {
        session_runtime::execution_token_budget(self, _session)
    }

    #[cfg(feature = "http")]
    pub(crate) fn set_session_surface_and_budget(
        &self,
        session_id: &str,
        surface: ToolSurface,
        budget: usize,
    ) {
        session_runtime::set_session_surface_and_budget(self, session_id, surface, budget)
    }

    pub(crate) fn push_recent_tool_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
        name: &str,
    ) {
        session_runtime::push_recent_tool_for_session(self, _session, name);
    }

    pub(crate) fn recent_tools_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> Vec<String> {
        session_runtime::recent_tools_for_session(self, _session)
    }

    pub(crate) fn record_file_access_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
        path: &str,
    ) {
        session_runtime::record_file_access_for_session(self, _session, path);
    }

    pub(crate) fn recent_file_paths_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> Vec<String> {
        session_runtime::recent_file_paths_for_session(self, _session)
    }

    /// Returns (consecutive_count, is_rapid_burst).
    /// `is_rapid_burst` is true when 3+ identical calls happen within 10 seconds,
    /// indicating an agent retry loop rather than deliberate repeated usage.
    pub(crate) fn doom_loop_count_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
        name: &str,
        args_hash: u64,
    ) -> (usize, bool) {
        session_runtime::doom_loop_count_for_session(self, _session, name, args_hash)
    }

    #[cfg(feature = "http")]
    pub(crate) fn bind_project_to_session(&self, session_id: &str, project_path: &str) {
        session_runtime::bind_project_to_session(self, session_id, project_path);
    }

    #[cfg(feature = "http")]
    pub(crate) fn ensure_session_project<'a>(
        &'a self,
        session: &crate::session_context::SessionRequestContext,
    ) -> Result<Option<std::sync::MutexGuard<'a, ()>>, CodeLensError> {
        session_runtime::ensure_session_project(self, session)
    }

    #[cfg(not(feature = "http"))]
    pub(crate) fn ensure_session_project<'a>(
        &'a self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> Result<Option<std::sync::MutexGuard<'a, ()>>, CodeLensError> {
        session_runtime::ensure_session_project(self, _session)
    }

    // ── Embedding engine accessors ──────────────────────────────────────

    /// Get or initialize embedding engine for the current project.
    /// Fast path (read lock) if already initialized; slow path (write lock) for first init.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_engine(
        &self,
    ) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        // Fast path: already initialized
        {
            let guard = self.embedding.read().unwrap_or_else(|p| p.into_inner());
            if guard.is_some() {
                return guard;
            }
        }
        // Slow path: initialize under write lock
        {
            let mut wguard = self.embedding.write().unwrap_or_else(|p| p.into_inner());
            if wguard.is_none() {
                let project = self.project();
                *wguard = EmbeddingEngine::new(&project)
                    .map_err(|e| tracing::error!("EmbeddingEngine init failed: {e}"))
                    .ok();
            }
        }
        self.embedding.read().unwrap_or_else(|p| p.into_inner())
    }

    /// Read-only access to embedding state without triggering initialization.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_ref(&self) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        self.embedding.read().unwrap_or_else(|p| p.into_inner())
    }

    /// Drop the current embedding engine (called on project switch).
    #[cfg(feature = "semantic")]
    pub(crate) fn reset_embedding(&self) {
        let mut guard = self.embedding.write().unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }

    /// Lazy-loaded SCIP backend. Loads the SCIP index on first access
    /// and caches it for subsequent calls. Returns None if no index found.
    #[cfg(feature = "scip-backend")]
    pub(crate) fn scip(&self) -> Option<&codelens_engine::ScipBackend> {
        self.scip_backend
            .get_or_init(|| {
                let project = self.project();
                codelens_engine::ScipBackend::detect(project.as_path())
                    .and_then(|path| {
                        tracing::info!(path = %path.display(), "loading SCIP index");
                        codelens_engine::ScipBackend::load(&path)
                            .inspect_err(|e| {
                                tracing::warn!(error = %e, "failed to load SCIP index");
                            })
                            .ok()
                    })
                    .map(Arc::new)
            })
            .as_ref()
            .map(|arc| arc.as_ref())
    }

    // ── Active project accessors (check override, fallback to default) ──

    /// Get the active project root. Clones the ProjectRoot (just a PathBuf).
    pub(crate) fn project(&self) -> ProjectRoot {
        self.active_project_context()
            .map(|context| context.project.clone())
            .unwrap_or_else(|| self.default_project.clone())
    }

    /// Get the active symbol index.
    pub(crate) fn symbol_index(&self) -> Arc<SymbolIndex> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.symbol_index))
            .unwrap_or_else(|| Arc::clone(&self.default_symbol_index))
    }

    pub(crate) fn watcher_failure_health(&self) -> WatcherFailureHealth {
        watcher_health::watcher_failure_health(self)
    }

    pub(crate) fn prune_index_failures(&self) -> Result<WatcherFailureHealth, CodeLensError> {
        watcher_health::prune_index_failures(self)
    }

    /// Get the active graph cache.
    pub(crate) fn graph_cache(&self) -> Arc<GraphCache> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.graph_cache))
            .unwrap_or_else(|| Arc::clone(&self.default_graph_cache))
    }

    /// Get the active memories directory.
    pub(crate) fn memories_dir(&self) -> PathBuf {
        self.active_project_context()
            .map(|context| context.memories_dir.clone())
            .unwrap_or_else(|| self.default_memories_dir.clone())
    }

    /// Get the active analysis cache directory.
    pub(crate) fn analysis_dir(&self) -> PathBuf {
        self.active_project_context()
            .map(|context| context.analysis_dir.clone())
            .unwrap_or_else(|| self.default_analysis_dir.clone())
    }

    #[allow(dead_code)]
    pub(crate) fn artifact_store(&self) -> &AnalysisArtifactStore {
        &self.artifact_store
    }

    pub(crate) fn audit_dir(&self) -> PathBuf {
        self.active_project_context()
            .map(|context| context.audit_dir.clone())
            .unwrap_or_else(|| self.default_audit_dir.clone())
    }

    pub(crate) fn watcher_stats(&self) -> Option<codelens_engine::WatcherStats> {
        self.active_project_context()
            .as_ref()
            .and_then(|context| context.watcher.as_ref().map(FileWatcher::stats))
            .or_else(|| self.default_watcher.as_ref().map(FileWatcher::stats))
    }

    pub(crate) fn watcher_running(&self) -> bool {
        self.watcher_stats()
            .map(|stats| stats.running)
            .unwrap_or(false)
    }

    /// Switch the active project at runtime. Creates a new index and graph cache.
    pub(crate) fn switch_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let scope = project.as_path().to_string_lossy().to_string();
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        if scope == self.default_project_scope() {
            self.activate_project_context(None);
            return Ok(name);
        }

        if let Some(current) = self.active_project_context()
            && current.project.as_path() == project.as_path()
        {
            return Ok(name);
        }

        let context = {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(&scope) {
                cached
            } else {
                let built = Arc::new(Self::build_project_runtime_context(project, true)?);
                cache.insert(scope.clone(), Arc::clone(&built));
                let active_scope = self.current_project_scope();
                let protected = [self.default_project_scope(), active_scope, scope.clone()];
                let protected_refs = protected.iter().map(String::as_str).collect::<Vec<_>>();
                let _evicted =
                    cache.evict_until_within_limit(PROJECT_CONTEXT_CACHE_LIMIT, &protected_refs);
                built
            }
        };
        self.activate_project_context(Some(context));
        Ok(name)
    }

    /// Reset to the default project.
    #[allow(dead_code)]
    pub(crate) fn reset_project(&self) {
        self.activate_project_context(None);
    }

    /// Check if running on the default project.
    #[allow(dead_code)]
    pub(crate) fn is_default_project(&self) -> bool {
        self.active_project_context().is_none()
    }

    /// Access the LSP session pool. Pool uses internal per-session locking.
    pub(crate) fn lsp_pool(&self) -> Arc<LspSessionPool> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.lsp_pool))
            .unwrap_or_else(|| Arc::clone(&self.default_lsp_pool))
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

    pub(crate) fn effort_level(&self) -> EffortLevel {
        match self.effort_level.load(std::sync::atomic::Ordering::Relaxed) {
            0 => EffortLevel::Low,
            1 => EffortLevel::Medium,
            _ => EffortLevel::High,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_effort_level(&self, level: EffortLevel) {
        let val = match level {
            EffortLevel::Low => 0u8,
            EffortLevel::Medium => 1,
            EffortLevel::High => 2,
        };
        self.effort_level
            .store(val, std::sync::atomic::Ordering::Relaxed);
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
            | Some(ToolProfile::WorkflowFirst)
            | None => 1,
        }
    }

    /// Access the tool metrics registry.
    pub(crate) fn metrics(&self) -> &ToolMetricsRegistry {
        self.metrics.as_ref()
    }

    /// Record a tool call in the recent tools ring buffer.
    pub(crate) fn push_recent_tool(&self, name: &str) {
        self.recent_tools.push(name.to_owned());
    }

    /// Doom-loop detection: returns (repeat_count, is_rapid_burst).
    /// Threshold of 3 triggers a warning. `is_rapid_burst` is true when
    /// 3+ identical calls occur within 10 seconds (agent retry loop).
    pub(crate) fn doom_loop_count(&self, name: &str, args_hash: u64) -> (usize, bool) {
        let mut counter = self
            .doom_loop_counter
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let now = Self::now_ms();
        if counter.0 == name && counter.1 == args_hash {
            counter.2 += 1;
        } else {
            *counter = (name.to_owned(), args_hash, 1, now);
        }
        let is_rapid = counter.2 >= 3 && (now.saturating_sub(counter.3) < 10_000);
        (counter.2, is_rapid)
    }

    /// Get the recent tool call names (up to 5).
    pub(crate) fn recent_tools(&self) -> Vec<String> {
        self.recent_tools.snapshot()
    }

    /// Record a file path as recently accessed (for ranking boost).
    pub(crate) fn record_file_access(&self, path: &str) {
        self.recent_files.push_dedup(path);
    }

    /// Get recently accessed file paths (most recent last).
    pub(crate) fn recent_file_paths(&self) -> Vec<String> {
        self.recent_files.snapshot()
    }

    /// Record an analysis_id for cross-phase context.
    pub(crate) fn push_recent_analysis_id(&self, id: String) {
        self.recent_analysis_ids.push(id);
    }

    /// Get recent analysis IDs (most recent last).
    pub(crate) fn recent_analysis_ids(&self) -> Vec<String> {
        self.recent_analysis_ids.snapshot()
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
        let lsp_pool = self.lsp_pool();
        Self {
            default_project: project.clone(),
            default_symbol_index: symbol_index,
            default_graph_cache: graph_cache,
            default_lsp_pool: lsp_pool,
            default_memories_dir: memories_dir,
            default_analysis_dir: analysis_dir.clone(),
            default_audit_dir: audit_dir,
            default_watcher: None,
            project_override: std::sync::RwLock::new(None),
            project_context_cache: Mutex::new(ProjectContextCache::default()),
            transport_mode: Mutex::new(self.transport_mode()),
            daemon_mode: Mutex::new(self.daemon_mode()),
            client_profile: self.client_profile,
            effort_level: std::sync::atomic::AtomicU8::new(
                self.effort_level.load(std::sync::atomic::Ordering::Relaxed),
            ),
            surface: Mutex::new(*self.surface()),
            token_budget: std::sync::atomic::AtomicUsize::new(self.token_budget()),
            artifact_store: AnalysisArtifactStore::new(analysis_dir.clone()),
            job_store: crate::job_store::AnalysisJobStore::new(analysis_dir.join("jobs")),
            metrics: Arc::clone(&self.metrics),
            recent_tools: crate::recent_buffer::RecentRingBuffer::new(5),
            recent_analysis_ids: crate::recent_buffer::RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new((String::new(), 0, 0, 0)),
            recent_files: crate::recent_buffer::RecentRingBuffer::new(20),
            preflight_store: RecentPreflightStore::new(),
            analysis_queue: OnceLock::new(),
            watcher_maintenance: Mutex::new(HashMap::new()),
            project_execution_lock: Mutex::new(()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "scip-backend")]
            scip_backend: OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
            // Phase 4b: workers inherit the parent daemon's start
            // time so `get_capabilities` stays consistent across
            // clones.
            daemon_started_at: self.daemon_started_at.clone(),
        }
    }

    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let context = Self::build_project_runtime_context(project, true)
            .expect("startup project context should initialize");

        let state = Self::build(context, preset);
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
        let context = Self::build_project_runtime_context(project, false)
            .expect("test project context should initialize");

        let state = Self::build(context, preset);
        state.configure_transport_mode("stdio");
        state
    }

    fn build(context: ProjectRuntimeContext, preset: ToolPreset) -> Self {
        let default_project = context.project.clone();
        let default_symbol_index = Arc::clone(&context.symbol_index);
        let default_graph_cache = Arc::clone(&context.graph_cache);
        let default_lsp_pool = Arc::clone(&context.lsp_pool);
        let default_memories_dir = context.memories_dir.clone();
        let default_analysis_dir = context.analysis_dir.clone();
        let default_audit_dir = context.audit_dir.clone();
        let default_watcher = context.watcher;
        Self {
            default_project,
            default_symbol_index,
            default_graph_cache,
            default_lsp_pool,
            default_memories_dir,
            default_analysis_dir: default_analysis_dir.clone(),
            default_audit_dir,
            default_watcher,
            project_override: std::sync::RwLock::new(None),
            project_context_cache: Mutex::new(ProjectContextCache::default()),
            transport_mode: Mutex::new(RuntimeTransportMode::Stdio),
            daemon_mode: Mutex::new(RuntimeDaemonMode::Standard),
            client_profile: ClientProfile::detect(None),
            effort_level: std::sync::atomic::AtomicU8::new(match EffortLevel::detect() {
                EffortLevel::Low => 0,
                EffortLevel::Medium => 1,
                EffortLevel::High => 2,
            }),
            surface: Mutex::new(ToolSurface::Preset(preset)),
            token_budget: std::sync::atomic::AtomicUsize::new(
                crate::tool_defs::default_budget_for_preset(preset),
            ),
            artifact_store: AnalysisArtifactStore::new(default_analysis_dir.clone()),
            job_store: crate::job_store::AnalysisJobStore::new(default_analysis_dir.join("jobs")),
            metrics: Arc::new(ToolMetricsRegistry::new()),
            recent_tools: crate::recent_buffer::RecentRingBuffer::new(5),
            recent_analysis_ids: crate::recent_buffer::RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new((String::new(), 0, 0, 0)),
            recent_files: crate::recent_buffer::RecentRingBuffer::new(20),
            preflight_store: RecentPreflightStore::new(),
            analysis_queue: OnceLock::new(),
            watcher_maintenance: Mutex::new(HashMap::new()),
            project_execution_lock: Mutex::new(()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "scip-backend")]
            scip_backend: OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
            daemon_started_at: now_rfc3339_utc(),
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
    ) -> anyhow::Result<Vec<codelens_engine::SymbolInfo>> {
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
}

// ── Free functions (extracted from AppState for SRP) ─────────────────

/// Extract a symbol name hint from tool arguments by checking a priority
/// list of common field names (`name_path`, `symbol`, `symbol_name`,
/// `name`, `function_name`).
///
/// This is a pure function that does not need `AppState` — it was
/// originally an `&self` method but never referenced `self`.
pub(crate) fn extract_symbol_hint(arguments: &Value) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::fixtures::temp_project_root;

    #[test]
    fn switch_project_reuses_cached_symbol_index_and_lsp_pool() {
        let default_project = temp_project_root("default");
        let project_a = temp_project_root("a");
        let project_b = temp_project_root("b");
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);

        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();
        let first_index = state.symbol_index();
        let first_lsp_pool = state.lsp_pool();

        state
            .switch_project(project_b.as_path().to_str().unwrap())
            .unwrap();
        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();

        let reused_index = state.symbol_index();
        let reused_lsp_pool = state.lsp_pool();

        assert!(Arc::ptr_eq(&first_index, &reused_index));
        assert!(Arc::ptr_eq(&first_lsp_pool, &reused_lsp_pool));
    }
}
