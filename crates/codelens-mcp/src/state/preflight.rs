use serde_json::Value;

use crate::preflight_store::RecentPreflightStore;
use crate::runtime_types::RecentPreflight;

use super::AppState;

impl AppState {
    pub(crate) fn preflight_ttl_seconds(&self) -> u64 {
        super::preflight_ttl_ms() / 1000
    }

    fn preflight_key_for_scope(&self, scope: &str, logical_session: &str) -> String {
        RecentPreflightStore::key(scope, logical_session)
    }

    fn preflight_key_for_session(
        &self,
        session: &crate::session_context::SessionRequestContext,
        logical_session: &str,
    ) -> String {
        self.preflight_key_for_scope(&self.project_scope_for_session(session), logical_session)
    }

    pub(crate) fn clear_recent_preflights(&self) {
        self.session_signals.preflight_store.clear();
    }

    pub(crate) fn normalize_target_path(&self, path: &str) -> String {
        super::normalize_path_for_project(self.project().as_path(), path)
    }

    pub(crate) fn extract_target_paths(&self, arguments: &Value) -> Vec<String> {
        let mut targets = Vec::new();

        for key in ["file_path", "relative_path", "path", "target_file"] {
            if let Some(path) = arguments.get(key).and_then(|value| value.as_str()) {
                super::push_unique_string(&mut targets, self.normalize_target_path(path));
            }
        }

        if let Some(paths) = arguments
            .get("changed_files")
            .and_then(|value| value.as_array())
        {
            for value in paths {
                if let Some(path) = value.as_str() {
                    super::push_unique_string(&mut targets, self.normalize_target_path(path));
                } else if let Some(path) = value.get("path").and_then(|item| item.as_str()) {
                    super::push_unique_string(&mut targets, self.normalize_target_path(path));
                } else if let Some(path) = value.get("file").and_then(|item| item.as_str()) {
                    super::push_unique_string(&mut targets, self.normalize_target_path(path));
                }
            }
        }

        if let Some(paths) = arguments.get("paths").and_then(|value| value.as_array()) {
            for value in paths {
                if let Some(path) = value.as_str() {
                    super::push_unique_string(&mut targets, self.normalize_target_path(path));
                } else if let Some(path) = value.get("path").and_then(|item| item.as_str()) {
                    super::push_unique_string(&mut targets, self.normalize_target_path(path));
                }
            }
        }

        targets
    }

    pub(crate) fn record_recent_preflight_from_payload(
        &self,
        tool_name: &str,
        surface: &str,
        logical_session: &str,
        arguments: &Value,
        payload: &Value,
    ) {
        let key = self.preflight_key_for_scope(
            &self.project_scope_for_arguments(arguments),
            logical_session,
        );
        self.session_signals.preflight_store.record_from_payload(
            key,
            tool_name,
            surface,
            Self::now_ms(),
            self.extract_target_paths(arguments),
            super::extract_symbol_hint(arguments),
            payload,
        );
    }

    pub(crate) fn recent_preflight_for_session(
        &self,
        session: &crate::session_context::SessionRequestContext,
        logical_session: &str,
    ) -> Option<RecentPreflight> {
        self.session_signals
            .preflight_store
            .get(&self.preflight_key_for_session(session, logical_session))
    }

    #[cfg(test)]
    pub(crate) fn set_recent_preflight_timestamp_for_test(
        &self,
        logical_session: &str,
        timestamp_ms: u64,
    ) {
        self.session_signals.preflight_store.set_timestamp_for_test(
            &self.preflight_key_for_scope(&self.current_project_scope(), logical_session),
            timestamp_ms,
        );
    }
}
