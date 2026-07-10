use crate::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{
    ToolSurface, is_content_mutation_tool, is_deferred_control_tool, is_read_only_surface,
    is_tool_callable_in_surface, preferred_namespaces, preferred_tier_labels, tool_namespace,
    tool_tier_label,
};

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
    // Ranked bootstrap slice (default_visible_rank): advertised in every
    // default tools/list, so a call must not bounce off the expansion gate.
    if crate::tool_defs::tool_default_listed(name) {
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
    // Same bootstrap-slice bypass as the namespace gate above.
    if crate::tool_defs::tool_default_listed(name) {
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

    if !crate::tool_defs::experimental_tool_enabled(name) {
        let feature = crate::tool_defs::experimental_feature_for_tool(name).unwrap_or("unknown");
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` requires experimental feature `{feature}`"
        )));
    }

    if !is_tool_callable_in_surface(name, surface) {
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

    // #347 hard gate — must stay the LAST check so trusted-client,
    // daemon-mode, and read-only-surface errors keep precedence. A
    // store-backed HTTP session without an explicit project binding may
    // target the daemon's default project instead of the caller's repo,
    // so content mutations are blocked pre-execution; read tools keep the
    // advisory `project_binding` hint (dispatch/mod.rs). Escape hatch:
    // CODELENS_ALLOW_UNBOUND_MUTATION=1 restores advisory-only behavior.
    #[cfg(feature = "http")]
    if is_content_mutation_tool(name)
        && state.should_route_to_session(session)
        && !state.session_project_binding_explicit(session.session_id.as_str())
        && crate::env_compat::env_var_bool("CODELENS_ALLOW_UNBOUND_MUTATION") != Some(true)
    {
        return Err(CodeLensError::ProjectBindingRequired {
            tool: name.to_owned(),
        });
    }

    Ok(())
}

// ── Role gate (merged from role_gate.rs) ────────────────────────────────────────

use crate::principals::{required_role_for, resolve_principal_id};
/// Enforce the role gate for a single dispatch call.
pub(super) fn enforce_role_gate(
    operation: &super::session::OperationAudit<'_>,
) -> Result<(), CodeLensError> {
    let state = operation.state;
    let name = operation.name;
    let session = operation.session;
    let required = required_role_for(name);
    let principal_id = resolve_principal_id(session);
    let principal_role = state.principals().resolve(principal_id.as_deref());
    if principal_role.satisfies(required) {
        return Ok(());
    }
    let permission_error = CodeLensError::PermissionDenied {
        principal: principal_id.unwrap_or_else(|| "<default>".to_owned()),
        principal_role: principal_role.as_str().to_owned(),
        tool: name.to_owned(),
        required_role: required.as_str().to_owned(),
    };
    super::session::record_audit_rejection(
        operation,
        "denied",
        crate::runtime_types::LifecycleState::Denied,
        &permission_error,
    )?;
    Err(permission_error)
}
