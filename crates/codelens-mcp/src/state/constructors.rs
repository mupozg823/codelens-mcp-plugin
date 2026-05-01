use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use codelens_engine::ProjectRoot;

use crate::agent_coordination::AgentCoordinationStore;
use crate::artifact_store::AnalysisArtifactStore;
use crate::client_profile::{ClientProfile, EffortLevel};
use crate::preflight_store::RecentPreflightStore;
use crate::recent_buffer::RecentRingBuffer;
use crate::runtime_types::{RuntimeDaemonMode, RuntimeTransportMode};
use crate::state::project_runtime::{ProjectContextCache, ProjectRuntimeContext};
use crate::state::{AppState, now_rfc3339_utc};
use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::{ToolPreset, ToolSurface};

impl AppState {
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
            recent_tools: RecentRingBuffer::new(5),
            recent_analysis_ids: RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new(HashMap::new()),
            recent_files: RecentRingBuffer::new(20),
            preflight_store: RecentPreflightStore::new(),
            coord_store: Arc::clone(&self.coord_store),
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
            #[cfg(feature = "http")]
            http_auth: std::sync::Mutex::new(Arc::clone(&*self.http_auth.lock().unwrap())),
            compat_mode: Mutex::new(self.compat_mode()),
            daemon_started_at: self.daemon_started_at.clone(),
            audit_sinks: Mutex::new(HashMap::new()),
            principals_by_audit_dir: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let context = Self::build_project_runtime_context(project, true)
            .expect("startup project context should initialize");

        let state = Self::build(context, preset);
        state.configure_transport_mode("stdio");
        state.artifact_store.cleanup_stale_dirs(crate::util::now_ms());
        let scope = state.current_project_scope();
        state
            .job_store
            .cleanup_stale_files(crate::util::now_ms(), Some(&scope));
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
            recent_tools: RecentRingBuffer::new(5),
            recent_analysis_ids: RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new(HashMap::new()),
            recent_files: RecentRingBuffer::new(20),
            preflight_store: RecentPreflightStore::new(),
            coord_store: Arc::new(AgentCoordinationStore::new()),
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
