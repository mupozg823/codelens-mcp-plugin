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

pub(crate) fn validate_tool_access(
    name: &str,
    session: &SessionRequestContext,
    surface: ToolSurface,
    state: &AppState,
) -> Result<(), CodeLensError> {
    let active_surface = surface.as_label();

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

    Ok(())
}

// ── Role gate (merged from role_gate.rs) ────────────────────────────────────────

use crate::audit_sink::AuditRecord;
use crate::principals::{required_role_for, resolve_principal_id, Role};
use serde_json::Value;

/// Enforce the role gate for a single dispatch call.
pub(super) fn enforce_role_gate(
    state: &AppState,
    name: &str,
    arguments: &Value,
    session: &SessionRequestContext,
    active_surface: &str,
) -> Result<(), CodeLensError> {
    let required = required_role_for(name);
    let principal_id = resolve_principal_id(session);
    let principal_role = state.principals().resolve(principal_id.as_deref());
    if principal_role.satisfies(required) {
        return Ok(());
    }
    record_denied_audit_row(
        state,
        name,
        arguments,
        session,
        active_surface,
        principal_id.as_deref(),
        principal_role,
        required,
    );
    Err(CodeLensError::PermissionDenied {
        principal: principal_id.unwrap_or_else(|| "<default>".to_owned()),
        principal_role: principal_role.as_str().to_owned(),
        tool: name.to_owned(),
        required_role: required.as_str().to_owned(),
    })
}

#[allow(clippy::too_many_arguments)]
fn record_denied_audit_row(
    state: &AppState,
    name: &str,
    arguments: &Value,
    session: &SessionRequestContext,
    active_surface: &str,
    principal_id: Option<&str>,
    principal_role: Role,
    required_role: Role,
) {
    let Some(sink) = state.audit_sink() else {
        return;
    };
    let now_ms = crate::util::now_ms() as i64;
    let args_hash = crate::util::canonical_sha256_hex(arguments);
    let transaction_id = format!("{}-{}-{}", session.session_id, name, &args_hash[..16]);
    let session_metadata = Some(super::session::session_metadata_for(
        state,
        session,
        active_surface,
    ));
    let record = AuditRecord {
        transaction_id,
        timestamp_ms: now_ms,
        principal: principal_id.map(str::to_owned),
        tool: name.to_owned(),
        args_hash,
        apply_status: "denied".to_owned(),
        state_from: None,
        state_to: "Denied".to_owned(),
        evidence_hash: None,
        rollback_restored: None,
        error_message: Some(format!(
            "principal_role={} does not satisfy required_role={}",
            principal_role.as_str(),
            required_role.as_str()
        )),
        session_metadata,
    };
    if let Err(error) = sink.write(&record) {
        tracing::warn!(
            tool = name,
            error = %error,
            "failed to write denied-row audit_sink entry"
        );
    }
}
