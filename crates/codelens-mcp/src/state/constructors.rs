use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use codelens_engine::ProjectRoot;

use crate::agent_coordination::AgentCoordinationStore;
use crate::artifact_store::AnalysisArtifactStore;
use crate::client_profile::{ClientProfile, EffortLevel};
use crate::orchestration_store::OrchestrationStore;
use crate::preflight_store::RecentPreflightStore;
use crate::recent_buffer::RecentRingBuffer;
use crate::runtime_types::{RuntimeDaemonMode, RuntimeTransportMode};
use crate::sparse_symbol_cache::SparseSymbolCache;
use crate::state::project_runtime::{ProjectContext, ProjectContextCache};
use crate::state::{AppState, now_rfc3339_utc};
use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::{ToolPreset, ToolSurface};

impl AppState {
    pub(crate) fn clone_for_worker(&self) -> Self {
        // #357: pin worker clones to the daemon's own DEFAULT project —
        // never to accessor methods, which resolve through the caller's
        // request-scoped binding (the first session to trigger queue init
        // would otherwise become every worker's implicit root). Jobs bind
        // their own scope per run via `bind_request_project_scope`.
        Self {
            default_context: Arc::clone(&self.default_context),
            project_override: std::sync::RwLock::new(None),
            project_context_cache: Arc::clone(&self.project_context_cache),
            transport_mode: Mutex::new(self.transport_mode()),
            daemon_mode: Mutex::new(self.daemon_mode()),
            client_profile: self.client_profile,
            effort_level: std::sync::atomic::AtomicU8::new(
                self.effort_level.load(std::sync::atomic::Ordering::Relaxed),
            ),
            surface: Mutex::new(*self.surface()),
            token_budget: std::sync::atomic::AtomicUsize::new(self.token_budget()),
            artifact_store: Arc::clone(&self.artifact_store),
            job_store: Arc::clone(&self.job_store),
            metrics: Arc::clone(&self.metrics),
            recent_tools: RecentRingBuffer::new(5),
            recent_analysis_ids: RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new(HashMap::new()),
            recent_files: RecentRingBuffer::new(20),
            preflight_store: RecentPreflightStore::new(),
            orchestration_store: Arc::clone(&self.orchestration_store),
            coord_store: Arc::clone(&self.coord_store),
            job_service: OnceLock::new(),
            sparse_symbol_cache: Arc::clone(&self.sparse_symbol_cache),
            watcher_maintenance: Mutex::new(HashMap::new()),
            local_full_tool_exposure: std::sync::atomic::AtomicBool::new(false),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "semantic")]
            embedding_root: Mutex::new(None),
            #[cfg(feature = "scip-backend")]
            scip_backends: Mutex::new(HashMap::new()),
            #[cfg(feature = "http")]
            session_store: None,
            #[cfg(feature = "http")]
            http_auth: std::sync::Mutex::new(Arc::clone(&*self.http_auth.lock().unwrap())),
            compat_mode: Mutex::new(self.compat_mode()),
            daemon_started_at: self.daemon_started_at.clone(),
            audit_sinks: Mutex::new(HashMap::new()),
            principals_by_audit_dir: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        Self::try_new(project, preset).expect("startup project context should initialize")
    }

    pub(crate) fn try_new(project: ProjectRoot, preset: ToolPreset) -> anyhow::Result<Self> {
        let context = Self::build_project_runtime_context(project, true)?;

        let state = Self::build(context, preset);
        state.configure_transport_mode("stdio");
        state
            .artifact_store
            .cleanup_stale_dirs(crate::util::now_ms());
        let scope = state.current_project_scope();
        let now_ms = crate::util::now_ms();
        if let Err(error) = state.job_store.recover_stale_running(
            now_ms,
            Some(&scope),
            crate::job_store::stale_job_heartbeat_ms(),
        ) {
            tracing::error!(%error, "failed to recover stale analysis jobs");
        }
        state.job_store.cleanup_stale_files(now_ms, Some(&scope));
        Ok(state)
    }

    /// Lightweight constructor that skips file watcher and stale-file cleanup.
    /// Reduces thread/I/O pressure when many instances run in parallel (e.g. tests).
    #[cfg(test)]
    pub(crate) fn new_minimal(project: ProjectRoot, preset: ToolPreset) -> Self {
        Self::try_new_minimal(project, preset).expect("test project context should initialize")
    }

    #[cfg(test)]
    pub(crate) fn try_new_minimal(
        project: ProjectRoot,
        preset: ToolPreset,
    ) -> anyhow::Result<Self> {
        let context = Self::build_project_runtime_context(project, false)?;

        let state = Self::build(context, preset);
        state.configure_transport_mode("stdio");
        Ok(state)
    }

    fn build(context: ProjectContext, preset: ToolPreset) -> Self {
        let default_analysis_dir = context.analysis_dir.clone();
        let default_context = Arc::new(context);
        Self {
            default_context,
            project_override: std::sync::RwLock::new(None),
            project_context_cache: Arc::new(Mutex::new(ProjectContextCache::default())),
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
            artifact_store: Arc::new(AnalysisArtifactStore::new(default_analysis_dir.clone())),
            job_store: Arc::new(crate::job_store::AnalysisJobStore::new(
                default_analysis_dir.join("jobs"),
            )),
            metrics: Arc::new(ToolMetricsRegistry::new()),
            recent_tools: RecentRingBuffer::new(5),
            recent_analysis_ids: RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new(HashMap::new()),
            recent_files: RecentRingBuffer::new(20),
            preflight_store: RecentPreflightStore::new(),
            orchestration_store: Arc::new(OrchestrationStore::new()),
            coord_store: Arc::new(AgentCoordinationStore::new()),
            job_service: OnceLock::new(),
            sparse_symbol_cache: Arc::new(SparseSymbolCache::new()),
            watcher_maintenance: Mutex::new(HashMap::new()),
            local_full_tool_exposure: std::sync::atomic::AtomicBool::new(false),
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "semantic")]
            embedding_root: Mutex::new(None),
            #[cfg(feature = "scip-backend")]
            scip_backends: Mutex::new(HashMap::new()),
            #[cfg(feature = "http")]
            session_store: None,
            #[cfg(feature = "http")]
            http_auth: std::sync::Mutex::new(Arc::new(
                crate::server::auth::HttpAuthState::default(),
            )),
            compat_mode: Mutex::new(crate::server::compat::ServerCompatMode::Default),
            daemon_started_at: now_rfc3339_utc(),
            audit_sinks: Mutex::new(HashMap::new()),
            principals_by_audit_dir: Mutex::new(HashMap::new()),
        }
    }
}
