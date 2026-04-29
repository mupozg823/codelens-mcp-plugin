use super::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::ToolSurface;

#[cfg(feature = "http")]
pub(super) fn http_session_state(
    state: &AppState,
    session: &SessionRequestContext,
) -> Option<std::sync::Arc<crate::server::session::SessionState>> {
    if session.is_local() {
        return None;
    }
    state
        .session_store
        .as_ref()
        .and_then(|store| store.get(&session.session_id))
}

#[cfg(feature = "http")]
pub(super) fn execution_surface(state: &AppState, session: &SessionRequestContext) -> ToolSurface {
    if let Some(session_state) = http_session_state(state, session) {
        return session_state.surface();
    }
    *state.surface()
}

#[cfg(not(feature = "http"))]
pub(super) fn execution_surface(state: &AppState, _session: &SessionRequestContext) -> ToolSurface {
    *state.surface()
}

#[cfg(feature = "http")]
pub(super) fn execution_token_budget(state: &AppState, session: &SessionRequestContext) -> usize {
    if let Some(session_state) = http_session_state(state, session) {
        return session_state.token_budget();
    }
    state.token_budget()
}

#[cfg(not(feature = "http"))]
pub(super) fn execution_token_budget(state: &AppState, _session: &SessionRequestContext) -> usize {
    state.token_budget()
}

#[cfg(feature = "http")]
pub(super) fn set_session_surface_and_budget(
    state: &AppState,
    session_id: &str,
    surface: ToolSurface,
    budget: usize,
) {
    if let Some(store) = &state.session_store {
        if let Some(session) = store.get(session_id) {
            session.set_surface(surface);
            session.set_token_budget(budget);
        }
    }
}

#[cfg(feature = "http")]
pub(super) fn notify_tools_list_changed(state: &AppState, session: &SessionRequestContext) {
    if let Some(session_state) = http_session_state(state, session) {
        let _ =
            session_state.notify_jsonrpc("notifications/tools/list_changed", serde_json::json!({}));
    }
}

pub(super) fn push_recent_tool_for_session(
    state: &AppState,
    _session: &SessionRequestContext,
    name: &str,
) {
    #[cfg(feature = "http")]
    if let Some(session_state) = http_session_state(state, _session) {
        session_state.push_recent_tool(name);
        return;
    }
    state.push_recent_tool(name);
}

pub(super) fn recent_tools_for_session(
    state: &AppState,
    _session: &SessionRequestContext,
) -> Vec<String> {
    #[cfg(feature = "http")]
    if let Some(session_state) = http_session_state(state, _session) {
        return session_state.recent_tools();
    }
    state.recent_tools()
}

pub(super) fn record_file_access_for_session(
    state: &AppState,
    _session: &SessionRequestContext,
    path: &str,
) {
    #[cfg(feature = "http")]
    if let Some(session_state) = http_session_state(state, _session) {
        session_state.record_file_access(path);
        return;
    }
    state.record_file_access(path);
}

pub(super) fn recent_file_paths_for_session(
    state: &AppState,
    _session: &SessionRequestContext,
) -> Vec<String> {
    #[cfg(feature = "http")]
    if let Some(session_state) = http_session_state(state, _session) {
        return session_state.recent_file_paths();
    }
    state.recent_file_paths()
}

pub(super) fn doom_loop_count_for_session(
    state: &AppState,
    _session: &SessionRequestContext,
    name: &str,
    args_hash: u64,
) -> (usize, bool) {
    #[cfg(feature = "http")]
    if let Some(session_state) = http_session_state(state, _session) {
        return session_state.doom_loop_count(name, args_hash);
    }
    state.doom_loop_count(_session.session_id.as_str(), name, args_hash)
}

#[cfg(feature = "http")]
pub(super) fn bind_project_to_session(state: &AppState, session_id: &str, project_path: &str) {
    if let Some(store) = &state.session_store {
        if let Some(session) = store.get(session_id) {
            session.set_project_path(project_path);
        }
    }
}

#[cfg(feature = "http")]
pub(super) fn ensure_session_project<'a>(
    state: &'a AppState,
    session: &SessionRequestContext,
) -> Result<Option<std::sync::MutexGuard<'a, ()>>, CodeLensError> {
    let Some(bound_project) = session.project_path.as_deref() else {
        return Ok(None);
    };
    let guard = state
        .project_execution_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = state.current_project_scope();
    if current != bound_project {
        state.switch_project(bound_project).map_err(|error| {
            CodeLensError::Validation(format!(
                "session project `{bound_project}` is not active and automatic rebind failed: {error}"
            ))
        })?;
    }
    Ok(Some(guard))
}

#[cfg(not(feature = "http"))]
pub(super) fn ensure_session_project<'a>(
    _state: &'a AppState,
    _session: &SessionRequestContext,
) -> Result<Option<std::sync::MutexGuard<'a, ()>>, CodeLensError> {
    Ok(None)
}
