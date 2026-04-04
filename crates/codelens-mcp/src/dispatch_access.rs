use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{
    is_content_mutation_tool, is_read_only_surface, is_tool_in_surface, preferred_namespaces,
    preferred_tier_labels, tool_namespace, tool_tier_label, ToolSurface,
};
use crate::AppState;

fn is_deferred_namespace_access_allowed(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
) -> bool {
    if !session.deferred_loading || session.is_local() {
        return true;
    }
    if session.full_tool_exposure {
        return true;
    }
    let namespace = tool_namespace(name);
    let preferred = preferred_namespaces(surface);
    if preferred.contains(&namespace) {
        return true;
    }
    session
        .loaded_namespaces
        .iter()
        .any(|value| value == namespace)
}

fn is_deferred_tier_access_allowed(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
) -> bool {
    if !session.deferred_loading || session.is_local() {
        return true;
    }
    if session.full_tool_exposure {
        return true;
    }
    let tier = tool_tier_label(name);
    let preferred = preferred_tier_labels(surface);
    if preferred.contains(&tier) {
        return true;
    }
    session.loaded_tiers.iter().any(|value| value == tier)
}

pub(crate) fn validate_tool_access(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
    state: &AppState,
) -> Result<(), CodeLensError> {
    let active_surface = surface.as_label();

    if !is_tool_in_surface(name, surface) {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is not available in active surface `{active_surface}`"
        )));
    }

    if !is_deferred_namespace_access_allowed(name, session, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in namespace `{}`. Call `tools/list` with `{{\"namespace\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_namespace(name),
            tool_namespace(name)
        )));
    }

    if !is_deferred_tier_access_allowed(name, session, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in tier `{}`. Call `tools/list` with `{{\"tier\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_tier_label(name),
            tool_tier_label(name)
        )));
    }

    if is_content_mutation_tool(name)
        && matches!(
            state.daemon_mode(),
            crate::state::RuntimeDaemonMode::MutationEnabled
        )
        && !session.trusted_client
    {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` requires a trusted HTTP client in daemon mode `{}`",
            state.daemon_mode().as_str()
        )));
    }

    if is_content_mutation_tool(name) && !state.mutation_allowed_in_runtime() {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is blocked by daemon mode `{}`",
            state.daemon_mode().as_str()
        )));
    }

    if is_read_only_surface(surface) && is_content_mutation_tool(name) {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is blocked in read-only surface `{active_surface}`"
        )));
    }

    Ok(())
}
