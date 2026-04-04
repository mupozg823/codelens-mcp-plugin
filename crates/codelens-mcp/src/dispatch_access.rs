use crate::AppState;
use crate::error::CodeLensError;
use crate::tool_defs::{
    ToolSurface, is_content_mutation_tool, is_read_only_surface, is_tool_in_surface,
    preferred_namespaces, preferred_tier_labels, tool_namespace, tool_tier_label,
};

fn session_loaded_namespaces(arguments: &serde_json::Value) -> Vec<&str> {
    arguments
        .get("_session_loaded_namespaces")
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn session_loaded_tiers(arguments: &serde_json::Value) -> Vec<&str> {
    arguments
        .get("_session_loaded_tiers")
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn session_full_tool_exposure(arguments: &serde_json::Value) -> bool {
    arguments
        .get("_session_full_tool_exposure")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn is_deferred_namespace_access_allowed(
    name: &str,
    arguments: &serde_json::Value,
    surface: ToolSurface,
) -> bool {
    let deferred_requested = arguments
        .get("_session_deferred_tool_loading")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !deferred_requested || crate::dispatch::logical_session_id(arguments) == "local" {
        return true;
    }
    if session_full_tool_exposure(arguments) {
        return true;
    }
    let namespace = tool_namespace(name);
    let preferred = preferred_namespaces(surface);
    if preferred.contains(&namespace) {
        return true;
    }
    session_loaded_namespaces(arguments).contains(&namespace)
}

fn is_deferred_tier_access_allowed(
    name: &str,
    arguments: &serde_json::Value,
    surface: ToolSurface,
) -> bool {
    let deferred_requested = arguments
        .get("_session_deferred_tool_loading")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !deferred_requested || crate::dispatch::logical_session_id(arguments) == "local" {
        return true;
    }
    if session_full_tool_exposure(arguments) {
        return true;
    }
    let tier = tool_tier_label(name);
    let preferred = preferred_tier_labels(surface);
    if preferred.contains(&tier) {
        return true;
    }
    session_loaded_tiers(arguments).contains(&tier)
}

pub(crate) fn validate_tool_access(
    name: &str,
    arguments: &serde_json::Value,
    surface: ToolSurface,
    state: &AppState,
) -> Result<(), CodeLensError> {
    let active_surface = surface.as_label();

    if !is_tool_in_surface(name, surface) {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is not available in active surface `{active_surface}`"
        )));
    }

    if !is_deferred_namespace_access_allowed(name, arguments, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in namespace `{}`. Call `tools/list` with `{{\"namespace\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_namespace(name),
            tool_namespace(name)
        )));
    }

    if !is_deferred_tier_access_allowed(name, arguments, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in tier `{}`. Call `tools/list` with `{{\"tier\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_tier_label(name),
            tool_tier_label(name)
        )));
    }

    let session_trusted_client = arguments
        .get("_session_trusted_client")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if is_content_mutation_tool(name)
        && matches!(
            state.daemon_mode(),
            crate::state::RuntimeDaemonMode::MutationEnabled
        )
        && !session_trusted_client
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
