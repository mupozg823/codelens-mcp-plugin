#[cfg(feature = "semantic")]
use codelens_core::EmbeddingEngine;
use codelens_core::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::analysis_queue::{
    AnalysisJobRequest, AnalysisWorkerQueue, HTTP_ANALYSIS_WORKER_COUNT,
    STDIO_ANALYSIS_WORKER_COUNT, analysis_job_cost_units,
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

const PROJECT_CONTEXT_CACHE_LIMIT: usize = 4;

/// Holds project-specific resources that can be reused across rebinds.
struct ProjectRuntimeContext {
    project: ProjectRoot,
    symbol_index: Arc<SymbolIndex>,
    graph_cache: Arc<GraphCache>,
    lsp_pool: Arc<LspSessionPool>,
    memories_dir: PathBuf,
    analysis_dir: PathBuf,
    audit_dir: PathBuf,
    /// Keeps the watcher alive so it continues to receive file-system events.
    watcher: Option<FileWatcher>,
}

#[derive(Default)]
struct ProjectContextCache {
    entries: HashMap<String, Arc<ProjectRuntimeContext>>,
    access_order: VecDeque<String>,
}

impl ProjectContextCache {
    fn get(&mut self, scope: &str) -> Option<Arc<ProjectRuntimeContext>> {
        let context = self.entries.get(scope).cloned()?;
        self.touch(scope);
        Some(context)
    }

    fn insert(&mut self, scope: String, context: Arc<ProjectRuntimeContext>) {
        self.entries.insert(scope.clone(), context);
        self.touch(&scope);
    }

    fn touch(&mut self, scope: &str) {
        self.access_order.retain(|entry| entry != scope);
        self.access_order.push_back(scope.to_owned());
    }

    fn evict_until_within_limit(
        &mut self,
        limit: usize,
        protected_scopes: &[&str],
    ) -> Vec<Arc<ProjectRuntimeContext>> {
        let mut evicted = Vec::new();
        while self.entries.len() > limit {
            let Some(oldest) = self.access_order.pop_front() else {
                break;
            };
            if protected_scopes.iter().any(|scope| *scope == oldest) {
                self.access_order.push_back(oldest);
                if self
                    .access_order
                    .iter()
                    .all(|scope| protected_scopes.iter().any(|protected| protected == &scope.as_str()))
                {
                    break;
                }
                continue;
            }
            if let Some(context) = self.entries.remove(&oldest) {
                evicted.push(context);
            }
        }
        evicted
    }
}

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
    watcher_maintenance: Mutex<HashMap<String, usize>>,
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    project_execution_lock: Mutex<()>,
    #[cfg(feature = "semantic")]
    pub(crate) embedding: std::sync::RwLock<Option<EmbeddingEngine>>,
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

    fn default_project_scope(&self) -> String {
        self.default_project.as_path().to_string_lossy().to_string()
    }

    fn active_project_context(&self) -> Option<Arc<ProjectRuntimeContext>> {
        self.project_override
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .cloned()
    }

    fn build_project_runtime_context(
        project: ProjectRoot,
        start_watcher: bool,
    ) -> anyhow::Result<ProjectRuntimeContext> {
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
        let lsp_pool = Arc::new(LspSessionPool::new(project.clone()));
        let watcher = if start_watcher {
            FileWatcher::start(
                project.as_path(),
                Arc::clone(&symbol_index),
                Arc::clone(&graph_cache),
            )
            .ok()
        } else {
            None
        };
        Ok(ProjectRuntimeContext {
            project,
            symbol_index,
            graph_cache,
            lsp_pool,
            memories_dir,
            analysis_dir,
            audit_dir,
            watcher,
        })
    }

    fn activate_project_context(&self, context: Option<Arc<ProjectRuntimeContext>>) {
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = context.clone();
        let analysis_dir = context
            .as_ref()
            .map(|override_ctx| override_ctx.analysis_dir.clone())
            .unwrap_or_else(|| self.default_analysis_dir.clone());
        self.artifact_store.set_analysis_dir(analysis_dir.clone());
        self.job_store.set_jobs_dir(analysis_dir.join("jobs"));
        self.artifact_store.clear();
        self.job_store.clear();
        self.clear_recent_preflights();
        #[cfg(feature = "semantic")]
        self.reset_embedding();
        self.artifact_store.cleanup_stale_dirs(Self::now_ms());
        let scope = self.current_project_scope();
        self.job_store
            .cleanup_stale_files(Self::now_ms(), Some(&scope));
    }

    #[cfg(feature = "http")]
    fn http_session_state(
        &self,
        session: &crate::session_context::SessionRequestContext,
    ) -> Option<Arc<crate::server::session::SessionState>> {
        if session.is_local() {
            return None;
        }
        self.session_store
            .as_ref()
            .and_then(|store| store.get(&session.session_id))
    }

    pub(crate) fn execution_surface(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> ToolSurface {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            return session_state.surface();
        }
        *self.surface()
    }

    pub(crate) fn execution_token_budget(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> usize {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            return session_state.token_budget();
        }
        self.token_budget()
    }

    #[cfg(feature = "http")]
    pub(crate) fn set_session_surface_and_budget(
        &self,
        session_id: &str,
        surface: ToolSurface,
        budget: usize,
    ) {
        if let Some(store) = &self.session_store
            && let Some(session) = store.get(session_id)
        {
            session.set_surface(surface);
            session.set_token_budget(budget);
        }
    }

    pub(crate) fn push_recent_tool_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
        name: &str,
    ) {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            session_state.push_recent_tool(name);
            return;
        }
        self.push_recent_tool(name);
    }

    pub(crate) fn recent_tools_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> Vec<String> {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            return session_state.recent_tools();
        }
        self.recent_tools()
    }

    pub(crate) fn record_file_access_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
        path: &str,
    ) {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            session_state.record_file_access(path);
            return;
        }
        self.record_file_access(path);
    }

    pub(crate) fn recent_file_paths_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> Vec<String> {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            return session_state.recent_file_paths();
        }
        self.recent_file_paths()
    }

    pub(crate) fn doom_loop_count_for_session(
        &self,
        _session: &crate::session_context::SessionRequestContext,
        name: &str,
        args_hash: u64,
    ) -> usize {
        #[cfg(feature = "http")]
        if let Some(session_state) = self.http_session_state(_session) {
            return session_state.doom_loop_count(name, args_hash);
        }
        self.doom_loop_count(name, args_hash)
    }

    #[cfg(feature = "http")]
    pub(crate) fn bind_project_to_session(&self, session_id: &str, project_path: &str) {
        if let Some(store) = &self.session_store
            && let Some(session) = store.get(session_id)
        {
            session.set_project_path(project_path);
        }
    }

    #[cfg(feature = "http")]
    pub(crate) fn ensure_session_project<'a>(
        &'a self,
        session: &crate::session_context::SessionRequestContext,
    ) -> Result<Option<std::sync::MutexGuard<'a, ()>>, CodeLensError> {
        let Some(bound_project) = session.project_path.as_deref() else {
            return Ok(None);
        };
        let guard = self
            .project_execution_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = self.current_project_scope();
        if current != bound_project {
            self.switch_project(bound_project).map_err(|error| {
                CodeLensError::Validation(format!(
                    "session project `{bound_project}` is not active and automatic rebind failed: {error}"
                ))
            })?;
        }
        Ok(Some(guard))
    }

    #[cfg(not(feature = "http"))]
    pub(crate) fn ensure_session_project<'a>(
        &'a self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> Result<Option<std::sync::MutexGuard<'a, ()>>, CodeLensError> {
        Ok(None)
    }

    pub(crate) fn preflight_ttl_seconds(&self) -> u64 {
        preflight_ttl_ms() / 1000
    }

    fn preflight_key(&self, logical_session: &str) -> String {
        RecentPreflightStore::key(&self.current_project_scope(), logical_session)
    }

    pub(crate) fn clear_recent_preflights(&self) {
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

    pub(crate) fn watcher_stats(&self) -> Option<codelens_core::WatcherStats> {
        self.active_project_context()
            .as_ref()
            .and_then(|context| context.watcher.as_ref().map(FileWatcher::stats))
            .or_else(|| self.default_watcher.as_ref().map(FileWatcher::stats))
    }

    pub(crate) fn watcher_running(&self) -> bool {
        self.watcher_stats().map(|stats| stats.running).unwrap_or(false)
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
            watcher_maintenance: Mutex::new(HashMap::new()),
            project_execution_lock: Mutex::new(()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "http")]
            session_store: None,
        }
    }

    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let context = Self::build_project_runtime_context(project, true)
            .expect("startup project context should initialize");

        let state = Self::build(
            context,
            preset,
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
        let context = Self::build_project_runtime_context(project, false)
            .expect("test project context should initialize");

        let state = Self::build(
            context,
            preset,
        );
        state.configure_transport_mode("stdio");
        state
    }

    fn build(
        context: ProjectRuntimeContext,
        preset: ToolPreset,
    ) -> Self {
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
            surface: Mutex::new(ToolSurface::Preset(preset)),
            token_budget: std::sync::atomic::AtomicUsize::new(
                crate::tool_defs::default_budget_for_preset(preset),
            ),
            artifact_store: AnalysisArtifactStore::new(default_analysis_dir.clone()),
            job_store: crate::job_store::AnalysisJobStore::new(default_analysis_dir.join("jobs")),
            metrics: Arc::new(ToolMetricsRegistry::new()),
            recent_tools: Mutex::new(VecDeque::with_capacity(5)),
            recent_analysis_ids: Mutex::new(VecDeque::with_capacity(5)),
            doom_loop_counter: Mutex::new((String::new(), 0, 0)),
            recent_files: Mutex::new(VecDeque::with_capacity(20)),
            preflight_store: RecentPreflightStore::new(),
            analysis_queue: OnceLock::new(),
            watcher_maintenance: Mutex::new(HashMap::new()),
            project_execution_lock: Mutex::new(()),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project_root(label: &str) -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-state-{label}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "fn sample() {}\n").unwrap();
        ProjectRoot::new(&dir).unwrap()
    }

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
