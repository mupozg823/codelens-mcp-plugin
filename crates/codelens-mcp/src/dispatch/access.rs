use crate::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{
    ToolSurface, is_content_mutation_tool, is_deferred_control_tool, is_read_only_surface,
    is_tool_in_surface, preferred_namespaces, preferred_tier_labels, tool_namespace,
    tool_tier_label,
};

/// ADR-0016 hidden-alias escape: a tool is callable irrespective of the active
/// surface's *listing* when it is defined in `tools.toml`, when it is a
/// schemaless edit-core tool carried by a preset membership list (e.g.
/// `rename_symbol`, pending the ADR-0009/D3 re-listing), or when it is a
/// deprecated workflow alias. This is an *additive* allowance on top of surface
/// listing — see `validate_tool_access`. It deliberately does NOT widen the set
/// of dispatchable names beyond what the server already handled under the Full
/// preset (unknown / tombstoned names keep flowing to the dispatch layer's
/// unknown-tool path there); it only lets a *registered-but-unlisted* tool
/// through on the narrower profile surfaces, as a hidden alias.
fn is_registered_tool(name: &str) -> bool {
    crate::tool_defs::tool_definition(name).is_some()
        || crate::tool_defs::deprecated_workflow_alias(name).is_some()
        || crate::tool_defs::whitelist_preset_member_union().contains(name)
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

    // Callability gate (ADR-0016): listing OR registration, not listing alone.
    // A surface-listed tool passes as before (this also preserves the Full
    // preset's "everything the server can handle passes" behavior, so unknown /
    // tombstoned names keep flowing to the dispatch layer's unknown-tool path).
    // A registered-but-unlisted tool passes as a hidden alias — callable, and
    // flagged by a `surface_note` on success (dispatch/mod.rs). Only a name that
    // is neither listed nor registered is rejected here; the mutation gate below
    // keeps its own precedence.
    if !is_tool_in_surface(name, surface) && !is_registered_tool(name) {
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
        && !session.project_binding_is_explicit()
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

/// Check whether the current principal can invoke a tool without recording an
/// audit transition. QueryEngine uses this for a resolved verb target; the
/// outer dispatch owns the audit record for the original request.
pub(crate) fn validate_tool_role(
    state: &AppState,
    name: &str,
    session: &SessionRequestContext,
) -> Result<(), CodeLensError> {
    let required = required_role_for(name);
    let principal_id = resolve_principal_id(session);
    let principal_role = state.principals().resolve(principal_id.as_deref());
    if principal_role.satisfies(required) {
        return Ok(());
    }
    Err(CodeLensError::PermissionDenied {
        principal: principal_id.unwrap_or_else(|| "<default>".to_owned()),
        principal_role: principal_role.as_str().to_owned(),
        tool: name.to_owned(),
        required_role: required.as_str().to_owned(),
    })
}

/// Enforce the role gate for a single dispatch call.
pub(super) fn enforce_role_gate(
    operation: &super::session::OperationAudit<'_>,
) -> Result<(), CodeLensError> {
    let Err(permission_error) =
        validate_tool_role(operation.state, operation.name, operation.session)
    else {
        return Ok(());
    };
    super::session::record_audit_rejection(
        operation,
        "denied",
        crate::runtime_types::LifecycleState::Denied,
        &permission_error,
    )?;
    Err(permission_error)
}

#[cfg(all(test, feature = "http"))]
mod tests {
    use super::*;
    use crate::session_context::{SessionRequestContext, with_http_transport_context};
    use crate::tool_defs::ToolPreset;
    use serde_json::json;

    #[test]
    fn later_live_binding_cannot_authorize_an_implicit_request_snapshot() {
        let _env_guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_override = std::env::var("CODELENS_ALLOW_UNBOUND_MUTATION").ok();
        // SAFETY: process-wide environment access is serialized by TEST_ENV_LOCK.
        unsafe {
            std::env::remove_var("CODELENS_ALLOW_UNBOUND_MUTATION");
        }

        let project_dir = tempfile::tempdir().expect("project dir");
        let project = codelens_engine::ProjectRoot::new(
            project_dir.path().to_str().expect("UTF-8 project path"),
        )
        .expect("project root");
        let state = AppState::new(project, ToolPreset::Balanced).with_session_store();
        state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
        let store = state.session_store.as_ref().expect("session store");
        let live_session = store.create();
        let project_path = state.project().as_path().to_string_lossy().into_owned();
        assert!(store.seed_default_project_path(&live_session.id, &project_path));

        let request_snapshot = with_http_transport_context(|| {
            SessionRequestContext::from_json(&json!({
                "_session_id": live_session.id,
                "_session_trusted_client": true,
                "_session_project_path": project_path,
                "_session_project_binding_source": "daemon_default",
            }))
        });

        // Simulate a concurrent prepare that upgrades live metadata only after
        // this request captured its daemon-default project.
        assert!(store.set_project_path(&request_snapshot.session_id, &project_path));
        assert!(
            state.session_project_binding_explicit(&request_snapshot.session_id),
            "precondition: live metadata was upgraded"
        );

        let result = validate_tool_access(
            "write_memory",
            &request_snapshot,
            ToolSurface::Preset(ToolPreset::Balanced),
            &state,
        );

        // SAFETY: process-wide environment access is serialized by TEST_ENV_LOCK.
        unsafe {
            match previous_override {
                Some(value) => std::env::set_var("CODELENS_ALLOW_UNBOUND_MUTATION", value),
                None => std::env::remove_var("CODELENS_ALLOW_UNBOUND_MUTATION"),
            }
        }

        assert!(matches!(
            result,
            Err(CodeLensError::ProjectBindingRequired { .. })
        ));
    }
}
