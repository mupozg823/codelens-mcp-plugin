use super::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::ToolSurface;

#[derive(Clone, Debug, Default)]
pub(crate) struct LogicalSessionRuntime {
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub requested_profile: Option<String>,
    pub deferred_tool_loading: Option<bool>,
    pub project_path: Option<String>,
    pub loaded_namespaces: Vec<String>,
    pub loaded_tiers: Vec<String>,
    pub full_tool_exposure: Option<bool>,
    pub surface: Option<ToolSurface>,
    pub token_budget: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LogicalSessionMetadataUpdate {
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub requested_profile: Option<String>,
    pub deferred_tool_loading: Option<bool>,
    pub project_path: Option<String>,
    pub loaded_namespaces: Option<Vec<String>>,
    pub loaded_tiers: Option<Vec<String>>,
    pub full_tool_exposure: Option<bool>,
}

fn normalize_axes(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

pub(super) fn logical_session_snapshot(
    state: &AppState,
    session_id: &str,
) -> Option<LogicalSessionRuntime> {
    if session_id.is_empty() || session_id == "local" {
        return None;
    }
    state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(session_id)
        .cloned()
}

pub(super) fn upsert_logical_session_metadata(
    state: &AppState,
    session_id: &str,
    metadata: LogicalSessionMetadataUpdate,
) {
    if session_id.is_empty() || session_id == "local" {
        return;
    }
    let mut sessions = state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = sessions.entry(session_id.to_owned()).or_default();
    if let Some(client_name) = metadata.client_name {
        current.client_name = Some(client_name);
    }
    if let Some(client_version) = metadata.client_version {
        current.client_version = Some(client_version);
    }
    if let Some(requested_profile) = metadata.requested_profile {
        current.requested_profile = Some(requested_profile);
    }
    if let Some(deferred_tool_loading) = metadata.deferred_tool_loading {
        current.deferred_tool_loading = Some(deferred_tool_loading);
    }
    if let Some(project_path) = metadata.project_path {
        current.project_path = Some(project_path);
    }
    if let Some(mut loaded_namespaces) = metadata.loaded_namespaces {
        normalize_axes(&mut loaded_namespaces);
        current.loaded_namespaces = loaded_namespaces;
    }
    if let Some(mut loaded_tiers) = metadata.loaded_tiers {
        normalize_axes(&mut loaded_tiers);
        current.loaded_tiers = loaded_tiers;
    }
    if let Some(full_tool_exposure) = metadata.full_tool_exposure {
        current.full_tool_exposure = Some(full_tool_exposure);
    }
}

fn logical_session_surface(
    state: &AppState,
    session: &SessionRequestContext,
) -> Option<ToolSurface> {
    logical_session_snapshot(state, &session.session_id).and_then(|runtime| runtime.surface)
}

fn logical_session_budget(state: &AppState, session: &SessionRequestContext) -> Option<usize> {
    logical_session_snapshot(state, &session.session_id).and_then(|runtime| runtime.token_budget)
}

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
    if let Some(surface) = logical_session_surface(state, session) {
        return surface;
    }
    *state.surface()
}

#[cfg(not(feature = "http"))]
pub(super) fn execution_surface(state: &AppState, _session: &SessionRequestContext) -> ToolSurface {
    if let Some(surface) = logical_session_surface(state, _session) {
        return surface;
    }
    *state.surface()
}

#[cfg(feature = "http")]
pub(super) fn execution_token_budget(state: &AppState, session: &SessionRequestContext) -> usize {
    if let Some(session_state) = http_session_state(state, session) {
        return session_state.token_budget();
    }
    if let Some(budget) = logical_session_budget(state, session) {
        return budget;
    }
    state.token_budget()
}

#[cfg(not(feature = "http"))]
pub(super) fn execution_token_budget(state: &AppState, _session: &SessionRequestContext) -> usize {
    if let Some(budget) = logical_session_budget(state, _session) {
        return budget;
    }
    state.token_budget()
}

#[cfg(feature = "http")]
pub(super) fn set_session_surface_and_budget(
    state: &AppState,
    session_id: &str,
    surface: ToolSurface,
    budget: usize,
) {
    if let Some(store) = &state.session_store
        && let Some(session) = store.get(session_id)
    {
        session.set_surface(surface);
        session.set_token_budget(budget);
    }
}

#[cfg(feature = "http")]
pub(super) fn set_execution_surface_and_budget(
    state: &AppState,
    session: &SessionRequestContext,
    surface: ToolSurface,
    budget: usize,
) {
    if session.is_local() {
        state.set_surface(surface);
        state.set_token_budget(budget);
        return;
    }
    if http_session_state(state, session).is_some() {
        set_session_surface_and_budget(state, &session.session_id, surface, budget);
        return;
    }
    let mut sessions = state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = sessions.entry(session.session_id.clone()).or_default();
    current.surface = Some(surface);
    current.token_budget = Some(budget);
}

#[cfg(not(feature = "http"))]
pub(super) fn set_execution_surface_and_budget(
    state: &AppState,
    session: &SessionRequestContext,
    surface: ToolSurface,
    budget: usize,
) {
    if session.is_local() {
        state.set_surface(surface);
        state.set_token_budget(budget);
        return;
    }
    let mut sessions = state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = sessions.entry(session.session_id.clone()).or_default();
    current.surface = Some(surface);
    current.token_budget = Some(budget);
}

#[cfg(feature = "http")]
pub(super) fn record_deferred_axes_for_session(
    state: &AppState,
    session: &SessionRequestContext,
    namespace: Option<&str>,
    tier: Option<&str>,
) -> bool {
    if session.is_local() {
        return false;
    }
    if let Some(session_state) = http_session_state(state, session) {
        let mut expanded = false;
        if let Some(namespace) = namespace {
            session_state.record_loaded_namespace(namespace);
            expanded = true;
        }
        if let Some(tier) = tier {
            session_state.record_loaded_tier(tier);
            expanded = true;
        }
        return expanded;
    }
    let mut sessions = state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = sessions.entry(session.session_id.clone()).or_default();
    let mut expanded = false;
    if let Some(namespace) = namespace
        && !current
            .loaded_namespaces
            .iter()
            .any(|value| value == namespace)
    {
        current.loaded_namespaces.push(namespace.to_owned());
        expanded = true;
    }
    if let Some(tier) = tier
        && !current.loaded_tiers.iter().any(|value| value == tier)
    {
        current.loaded_tiers.push(tier.to_owned());
        expanded = true;
    }
    normalize_axes(&mut current.loaded_namespaces);
    normalize_axes(&mut current.loaded_tiers);
    expanded
}

#[cfg(feature = "http")]
pub(super) fn enable_full_tool_exposure_for_session(
    state: &AppState,
    session: &SessionRequestContext,
) -> bool {
    if session.is_local() {
        return false;
    }
    let Some(store) = &state.session_store else {
        let mut sessions = state
            .logical_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = sessions.entry(session.session_id.clone()).or_default();
        current.full_tool_exposure = Some(true);
        return true;
    };
    let Some(session_state) = store.get(&session.session_id) else {
        let mut sessions = state
            .logical_sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = sessions.entry(session.session_id.clone()).or_default();
        current.full_tool_exposure = Some(true);
        return true;
    };
    session_state.enable_full_tool_exposure();
    true
}

#[cfg(not(feature = "http"))]
pub(super) fn record_deferred_axes_for_session(
    state: &AppState,
    session: &SessionRequestContext,
    namespace: Option<&str>,
    tier: Option<&str>,
) -> bool {
    if session.is_local() {
        return false;
    }
    let mut sessions = state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = sessions.entry(session.session_id.clone()).or_default();
    let mut expanded = false;
    if let Some(namespace) = namespace
        && !current
            .loaded_namespaces
            .iter()
            .any(|value| value == namespace)
    {
        current.loaded_namespaces.push(namespace.to_owned());
        expanded = true;
    }
    if let Some(tier) = tier
        && !current.loaded_tiers.iter().any(|value| value == tier)
    {
        current.loaded_tiers.push(tier.to_owned());
        expanded = true;
    }
    normalize_axes(&mut current.loaded_namespaces);
    normalize_axes(&mut current.loaded_tiers);
    expanded
}

#[cfg(not(feature = "http"))]
pub(super) fn enable_full_tool_exposure_for_session(
    state: &AppState,
    session: &SessionRequestContext,
) -> bool {
    if session.is_local() {
        return false;
    }
    let mut sessions = state
        .logical_sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = sessions.entry(session.session_id.clone()).or_default();
    current.full_tool_exposure = Some(true);
    true
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
    state.doom_loop_count(name, args_hash)
}

#[cfg(feature = "http")]
pub(super) fn bind_project_to_session(state: &AppState, session_id: &str, project_path: &str) {
    if let Some(store) = &state.session_store
        && let Some(session) = store.get(session_id)
    {
        session.set_project_path(project_path);
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
