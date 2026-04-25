//! ADR-0009 §1 enforcement: every dispatch call passes through
//! [`enforce_role_gate`] before the handler runs. On deny, this module
//! also writes a single `apply_status="denied"` row to the audit sink
//! so operators can post-hoc reconstruct who was rejected, when, and
//! for which tool.
//!
//! The gate consults [`crate::principals::Principals`] (lazily loaded
//! on `AppState`) and [`crate::principals::required_role_for`]. By
//! design the rule is simple: mutation tools require `Refactor`,
//! everything else requires `ReadOnly`, and `Admin` is reserved for
//! future audit / job-control tools (P2-F).

use crate::audit_sink::{canonical_sha256_hex, AuditRecord};
use crate::error::CodeLensError;
use crate::principals::{required_role_for, resolve_principal_id, Role};
use crate::session_context::SessionRequestContext;
use crate::AppState;
use serde_json::Value;

/// Enforce the role gate for a single dispatch call.
///
/// Returns `Ok(())` when the resolved principal's role satisfies the
/// tool's required role. Returns `Err(CodeLensError::PermissionDenied)`
/// otherwise — and writes one `apply_status="denied"` row to the audit
/// sink before returning so the rejection is durably recorded.
pub(super) fn enforce_role_gate(
    state: &AppState,
    name: &str,
    arguments: &Value,
    session: &SessionRequestContext,
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

fn record_denied_audit_row(
    state: &AppState,
    name: &str,
    arguments: &Value,
    session: &SessionRequestContext,
    principal_id: Option<&str>,
    principal_role: Role,
    required_role: Role,
) {
    let Some(sink) = state.audit_sink() else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let args_hash = canonical_sha256_hex(arguments);
    // Same transaction id derivation as `record_audit_outcome` so a
    // future Admin running `audit_log_query(transaction_id)` can join
    // the denied attempt against later retry attempts of the same
    // call.
    let transaction_id = format!("{}-{}-{}", session.session_id, name, &args_hash[..16]);
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
    };
    if let Err(error) = sink.write(&record) {
        tracing::warn!(
            tool = name,
            error = %error,
            "failed to write denied-row audit_sink entry"
        );
    }
}
