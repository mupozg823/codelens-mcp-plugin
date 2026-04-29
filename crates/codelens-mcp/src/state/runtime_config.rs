use crate::analysis_queue::{HTTP_ANALYSIS_WORKER_COUNT, STDIO_ANALYSIS_WORKER_COUNT};
use crate::client_profile::{ClientProfile, EffortLevel};
use crate::runtime_types::{RuntimeDaemonMode, RuntimeTransportMode};
use crate::state::AppState;
use crate::tool_defs::{ToolProfile, ToolSurface};
#[cfg(feature = "http")]
use std::sync::Arc;

impl AppState {
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

    pub(crate) fn configure_compat_mode(&self, mode: crate::server::compat::ServerCompatMode) {
        *self
            .compat_mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode;
    }

    #[cfg(feature = "http")]
    pub(crate) fn configure_http_auth(&self, auth: crate::server::auth::HttpAuthConfig) {
        self.http_auth.lock().unwrap().configure(auth);
    }

    #[cfg(feature = "http")]
    pub(crate) fn http_auth(&self) -> Arc<crate::server::auth::HttpAuthState> {
        Arc::clone(&*self.http_auth.lock().unwrap())
    }

    pub(crate) fn compat_mode(&self) -> crate::server::compat::ServerCompatMode {
        *self
            .compat_mode
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
            RuntimeTransportMode::Http | RuntimeTransportMode::Https => HTTP_ANALYSIS_WORKER_COUNT,
            RuntimeTransportMode::Stdio => STDIO_ANALYSIS_WORKER_COUNT,
        }
    }

    pub(crate) fn analysis_cost_budget(&self) -> usize {
        match self.transport_mode() {
            RuntimeTransportMode::Http | RuntimeTransportMode::Https => 3,
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
}
