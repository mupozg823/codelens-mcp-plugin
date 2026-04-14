use crate::AppState;
use crate::client_profile::ClientProfile;
use crate::error::{CodeLensError, ToolAccessFailure};
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{
    ToolProfile, ToolSurface, default_budget_for_profile, is_content_mutation_tool,
    is_deferred_control_tool, is_read_only_surface, is_tool_in_surface, preferred_namespaces,
    preferred_tier_labels, tool_definition, tool_namespace, tool_tier_label,
};

fn detected_client_profile(state: &AppState, session: &SessionRequestContext) -> ClientProfile {
    session
        .client_name
        .as_deref()
        .map(|name| ClientProfile::detect(Some(name)))
        .unwrap_or_else(|| state.client_profile())
}

fn is_deferred_namespace_access_allowed(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
) -> bool {
    if !session.deferred_loading || session.is_local() {
        return true;
    }
    if is_deferred_control_tool(name) {
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
    if is_deferred_control_tool(name) {
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

fn should_auto_expand_hidden_tool_access(
    state: &AppState,
    session: &SessionRequestContext,
) -> bool {
    session.deferred_loading
        && !session.full_tool_exposure
        && matches!(
            detected_client_profile(state, session),
            ClientProfile::Codex
        )
}

fn codex_surface_promotion_target(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
    state: &AppState,
) -> Option<(ToolSurface, usize)> {
    if matches!(surface, ToolSurface::Profile(ToolProfile::WorkflowFirst))
        && !is_content_mutation_tool(name)
        && matches!(
            detected_client_profile(state, session),
            ClientProfile::Codex
        )
    {
        let promoted_surface = ToolSurface::Profile(ToolProfile::BuilderMinimal);
        if is_tool_in_surface(name, promoted_surface) {
            let budget = default_budget_for_profile(ToolProfile::BuilderMinimal)
                .max(detected_client_profile(state, session).default_budget());
            return Some((promoted_surface, budget));
        }
    }
    None
}

pub(crate) fn validate_tool_access(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
    state: &AppState,
) -> Result<ToolSurface, CodeLensError> {
    if tool_definition(name).is_none() {
        return Ok(surface);
    }

    let mut effective_surface = surface;
    let mut active_surface = effective_surface.as_label();

    if !is_tool_in_surface(name, effective_surface) {
        if let Some((promoted_surface, promoted_budget)) =
            codex_surface_promotion_target(name, session, effective_surface, state)
        {
            state.set_execution_surface_and_budget(session, promoted_surface, promoted_budget);
            effective_surface = promoted_surface;
            active_surface = effective_surface.as_label();
        }
    }

    if !is_tool_in_surface(name, effective_surface) {
        return Err(ToolAccessFailure::NotAvailableInActiveSurface {
            tool_name: name.to_owned(),
            active_surface: active_surface.to_owned(),
        }
        .into());
    }

    let mut namespace_allowed =
        is_deferred_namespace_access_allowed(name, session, effective_surface);
    let mut tier_allowed = is_deferred_tier_access_allowed(name, session, effective_surface);
    if (!namespace_allowed || !tier_allowed)
        && should_auto_expand_hidden_tool_access(state, session)
    {
        let namespace = (!namespace_allowed).then_some(tool_namespace(name));
        let tier = (!tier_allowed).then_some(tool_tier_label(name));
        if state.record_deferred_axes_for_session(session, namespace, tier) {
            state.metrics().record_deferred_namespace_expansion();
            namespace_allowed = true;
            tier_allowed = true;
        }
    }

    if !namespace_allowed {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(ToolAccessFailure::HiddenByDeferredNamespace {
            tool_name: name.to_owned(),
            namespace: tool_namespace(name).to_owned(),
        }
        .into());
    }

    if !tier_allowed {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(ToolAccessFailure::HiddenByDeferredTier {
            tool_name: name.to_owned(),
            tier: tool_tier_label(name).to_owned(),
        }
        .into());
    }

    if is_content_mutation_tool(name)
        && matches!(
            state.daemon_mode(),
            crate::state::RuntimeDaemonMode::MutationEnabled
        )
        && !session.trusted_client
    {
        return Err(ToolAccessFailure::TrustedHttpRequired {
            tool_name: name.to_owned(),
            daemon_mode: state.daemon_mode().as_str().to_owned(),
        }
        .into());
    }

    if is_content_mutation_tool(name) && !state.mutation_allowed_in_runtime() {
        return Err(ToolAccessFailure::DaemonModeBlocked {
            tool_name: name.to_owned(),
            daemon_mode: state.daemon_mode().as_str().to_owned(),
        }
        .into());
    }

    if is_read_only_surface(effective_surface) && is_content_mutation_tool(name) {
        return Err(ToolAccessFailure::ReadOnlySurfaceBlocked {
            tool_name: name.to_owned(),
            active_surface: active_surface.to_owned(),
        }
        .into());
    }

    Ok(effective_surface)
}
