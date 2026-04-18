//! Session management, project-scope resolution, and HTTP session store
//! accessors for `AppState`.
//!
//! Pure move from `state.rs` — no logic changes.

use serde_json::Value;

use crate::error::CodeLensError;

use super::AppState;
use super::session_runtime;

impl AppState {
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

    pub(super) fn default_project_scope(&self) -> String {
        self.default_project.as_path().to_string_lossy().to_string()
    }

    pub(crate) fn execution_surface(
        &self,
        _session: &crate::session_context::SessionRequestContext,
    ) -> crate::tool_defs::ToolSurface {
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
        surface: crate::tool_defs::ToolSurface,
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

    #[cfg(feature = "http")]
    pub(crate) fn notify_tools_list_changed(
        &self,
        session: &crate::session_context::SessionRequestContext,
    ) {
        session_runtime::notify_tools_list_changed(self, session);
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

    /// Decide whether a write operation should target the per-session store
    /// or fall through to global AppState. Returns true only when:
    ///   (a) the request carries a real (non-"local") session id, AND
    ///   (b) the session store has been initialized (`with_session_store`).
    ///
    /// Every write path that has an `if !session.is_local()` branch must use
    /// this helper instead — otherwise a non-local session_id from a test or
    /// a caller running without `with_session_store` silently no-ops (the
    /// session-scoped setter runs, finds no matching session entry, and
    /// returns without writing anywhere).
    #[cfg(feature = "http")]
    pub(crate) fn should_route_to_session(
        &self,
        session: &crate::session_context::SessionRequestContext,
    ) -> bool {
        !session.is_local() && self.session_store.is_some()
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
