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
    if let Some(session) = state
        .session_store
        .as_ref()
        .and_then(|store| store.get(session_id))
    {
        session.set_surface(surface);
        session.set_token_budget(budget);
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
pub(super) fn bind_project_to_session(
    state: &AppState,
    session_id: &str,
    project_path: &str,
) -> bool {
    state
        .session_store
        .as_ref()
        .map(|store| store.set_project_path(session_id, project_path))
        .unwrap_or(false)
}

#[cfg(feature = "http")]
pub(super) fn ensure_session_project(
    state: &AppState,
    session: &SessionRequestContext,
) -> Result<Option<crate::state::project_runtime::RequestProjectGuard>, CodeLensError> {
    let mut bound_project_opt = session.project_path.clone();
    if bound_project_opt.is_none()
        && !session.is_local()
        && let Some(session_state) = http_session_state(state, session)
    {
        bound_project_opt = session_state.client_metadata().project_path;
    }
    let Some(bound_project) = bound_project_opt.as_deref() else {
        // Still scope the request: a binding this request makes for itself
        // must end with it.
        return Ok(Some(super::project_runtime::preserve_request_project()));
    };
    // #357: bind the request thread to the session's project instead of
    // switching the daemon-global override under a global mutex. Concurrent
    // sessions bound to different projects no longer serialize on one lock,
    // and a switch no longer clears another session's artifact/job/preflight
    // state mid-flight.
    let guard = state
        .bind_request_project_scope(bound_project)
        .map_err(|error| match error.downcast::<CodeLensError>() {
            // Preserve a structured rejection (e.g. HomeRootRejected) so the
            // client still receives its machine-readable recovery hint.
            Ok(structured) => structured,
            Err(other) => CodeLensError::Validation(format!(
                "session project `{bound_project}` is not active and automatic rebind failed: {other}"
            )),
        })?;
    Ok(Some(guard))
}

#[cfg(not(feature = "http"))]
pub(super) fn ensure_session_project(
    _state: &AppState,
    _session: &SessionRequestContext,
) -> Result<Option<crate::state::project_runtime::RequestProjectGuard>, CodeLensError> {
    Ok(None)
}

#[cfg(all(test, feature = "http"))]
mod request_scope_tests {
    use crate::AppState;
    use crate::session_context::SessionRequestContext;
    use crate::tool_defs::ToolPreset;

    fn temp_project(label: &str) -> codelens_engine::ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-request-scope-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "pub fn scope_probe() {}\n").unwrap();
        codelens_engine::ProjectRoot::new_exact(&dir).unwrap()
    }

    #[test]
    fn a_rebind_in_an_unbound_session_ends_with_its_request() {
        // `activate_project` rebinds the request thread in place. A session
        // with no project yet got no request guard, so the binding stayed on
        // the pooled thread and the next request there, from any unbound
        // session or a sessionless caller, read that project.
        let state = AppState::new_minimal(temp_project("default"), ToolPreset::Balanced)
            .with_session_store();
        let default_root = state.project().as_path().to_path_buf();
        let other = temp_project("other");
        let unbound = SessionRequestContext {
            session_id: "00000000-0000-4000-8000-0000000000aa".to_owned(),
            ..Default::default()
        };

        {
            let _request = state.ensure_session_project(&unbound).unwrap();
            state
                .rebind_request_project_scope(other.as_path().to_str().unwrap())
                .unwrap();
            assert_eq!(state.project().as_path(), other.as_path());
        }

        assert_eq!(
            state.project().as_path(),
            default_root,
            "the next request on this thread must not inherit the rebind"
        );
    }
}
