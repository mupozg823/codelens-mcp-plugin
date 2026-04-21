use super::*;
use std::sync::Arc;

impl AppState {
    /// RFC 3339 UTC timestamp when the daemon started — captured
    /// once at `AppState::build` and inherited by worker clones.
    /// Phase 4b (§capability-reporting): exposed in
    /// `get_capabilities` so downstream callers can detect "daemon
    /// has been running since before the binary was rebuilt".
    pub(crate) fn daemon_started_at(&self) -> &str {
        &self.runtime_config.daemon_started_at
    }

    pub(super) fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub(super) fn active_project_context(&self) -> Option<Arc<ProjectRuntimeContext>> {
        project_runtime::active_project_context(self)
    }

    pub(super) fn build_project_runtime_context(
        project: ProjectRoot,
        start_watcher: bool,
    ) -> anyhow::Result<ProjectRuntimeContext> {
        project_runtime::build_project_runtime_context(project, start_watcher)
    }

    pub(super) fn activate_project_context(&self, context: Option<Arc<ProjectRuntimeContext>>) {
        project_runtime::activate_project_context(self, context)
    }

    pub(crate) fn watcher_failure_health(&self) -> WatcherFailureHealth {
        watcher_health::watcher_failure_health(self)
    }

    pub(crate) fn prune_index_failures(&self) -> Result<WatcherFailureHealth, CodeLensError> {
        watcher_health::prune_index_failures(self)
    }

    /// Phase P2 accessor for the process-wide workflow result cache.
    /// Clones the `Arc` so callers can hold on to the cache across
    /// await points without borrowing `self`.
    pub(crate) fn workflow_cache(&self) -> Arc<workflow_cache::WorkflowAnalysisCache> {
        Arc::clone(&self.analysis_runtime.workflow_cache)
    }

    /// Phase P2 cheap hash of the current project state. Built from
    /// the indexed file count + the symbol-index stats so any
    /// addition/removal/refresh changes the value. Used as the
    /// third component of the cache key so entries computed against
    /// an older state never serve newer callers.
    pub(crate) fn workflow_project_state_hash(&self) -> u64 {
        use std::hash::{DefaultHasher, Hasher};
        let mut hasher = DefaultHasher::new();
        if let Ok(stats) = self.symbol_index().stats() {
            hasher.write_usize(stats.indexed_files);
            hasher.write_usize(stats.supported_files);
            hasher.write_usize(stats.stale_files);
        } else {
            hasher.write_u8(0xFF);
        }
        hasher.write(self.project().as_path().as_os_str().as_encoded_bytes());
        hasher.finish()
    }

    /// Acquire active tool surface with poison recovery.
    pub(crate) fn surface(&self) -> std::sync::MutexGuard<'_, ToolSurface> {
        self.runtime_config
            .surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn set_surface(&self, surface: ToolSurface) {
        *self
            .runtime_config
            .surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = surface;
    }

    pub(crate) fn configure_daemon_mode(&self, daemon_mode: RuntimeDaemonMode) {
        *self
            .runtime_config
            .daemon_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = daemon_mode;
    }

    pub(crate) fn configure_coordination_mode(&self, coordination_mode: RuntimeCoordinationMode) {
        *self
            .runtime_config
            .coordination_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = coordination_mode;
    }

    pub(crate) fn configure_transport_mode(&self, transport: &str) {
        let mode = RuntimeTransportMode::from_str(transport);
        *self
            .runtime_config
            .transport_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode;
        self.telemetry_runtime.metrics.record_analysis_worker_pool(
            self.analysis_worker_limit(),
            self.analysis_cost_budget(),
            mode.as_str(),
        );
    }

    pub(crate) fn transport_mode(&self) -> RuntimeTransportMode {
        *self
            .runtime_config
            .transport_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn daemon_mode(&self) -> RuntimeDaemonMode {
        *self
            .runtime_config
            .daemon_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn coordination_mode(&self) -> RuntimeCoordinationMode {
        *self
            .runtime_config
            .coordination_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn client_profile(&self) -> ClientProfile {
        self.runtime_config.client_profile
    }

    pub(crate) fn effort_level(&self) -> EffortLevel {
        match self
            .runtime_config
            .effort_level
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => EffortLevel::Low,
            1 => EffortLevel::Medium,
            3 => EffortLevel::XHigh,
            _ => EffortLevel::High,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_effort_level(&self, level: EffortLevel) {
        let val = match level {
            EffortLevel::Low => 0u8,
            EffortLevel::Medium => 1,
            EffortLevel::High => 2,
            EffortLevel::XHigh => 3,
        };
        self.runtime_config
            .effort_level
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

    pub(crate) fn clone_for_worker(&self) -> Self {
        let project = self.project();
        let symbol_index = self.symbol_index();
        let graph_cache = self.graph_cache();
        let memories_dir = self.memories_dir();
        let analysis_dir = self.analysis_dir();
        let audit_dir = self.audit_dir();
        let lsp_pool = self.lsp_pool();
        Self {
            project_runtime: self
                .project_runtime
                .clone_for_worker(WorkerProjectRuntimeSeed {
                    project: project.clone(),
                    symbol_index,
                    graph_cache,
                    lsp_pool,
                    memories_dir,
                    analysis_dir: analysis_dir.clone(),
                    audit_dir,
                }),
            runtime_config: self.runtime_config.clone_for_worker(),
            analysis_runtime: self.analysis_runtime.clone_for_worker(&analysis_dir),
            telemetry_runtime: self.telemetry_runtime.clone_for_worker(),
            session_signals: SessionSignalsService::new(),
            coordination_runtime: self.coordination_runtime.clone_for_worker(),
        }
    }

    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let context = Self::build_project_runtime_context(project, true)
            .expect("startup project context should initialize");

        let state = Self::build(context, preset);
        state.configure_transport_mode("stdio");
        state
            .analysis_runtime
            .artifact_store
            .cleanup_stale_dirs(Self::now_ms());
        let scope = state.current_project_scope();
        state
            .analysis_runtime
            .job_store
            .cleanup_stale_files(Self::now_ms(), Some(&scope));
        state
    }

    #[cfg(test)]
    pub(crate) fn new_minimal(project: ProjectRoot, preset: ToolPreset) -> Self {
        let context = Self::build_project_runtime_context(project, false)
            .expect("test project context should initialize");

        let state = Self::build(context, preset);
        state.configure_transport_mode("stdio");
        state
    }

    fn build(context: ProjectRuntimeContext, preset: ToolPreset) -> Self {
        let default_analysis_dir = context.analysis_dir.clone();
        Self {
            project_runtime: ProjectRuntimeService::from_context(context),
            runtime_config: RuntimeConfigService::new(preset),
            analysis_runtime: AnalysisRuntimeService::new(&default_analysis_dir),
            telemetry_runtime: TelemetryRuntimeService::new(),
            session_signals: SessionSignalsService::new(),
            coordination_runtime: CoordinationRuntimeService::new(),
        }
    }
}
