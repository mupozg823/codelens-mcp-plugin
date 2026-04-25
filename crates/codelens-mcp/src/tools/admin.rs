//! ADR-0009 §2 + §6: Admin tools.
//!
//! Currently exposes `audit_log_query` — a read-only window into the
//! `audit_log.sqlite` rows written by every mutation call. Requires
//! `Admin` role (see `crate::principals::required_role_for`).

use super::{optional_string, optional_usize, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use serde_json::{json, Value};

/// Query the durable audit log.
///
/// Filters:
/// - `transaction_id` — narrow to one mutation call
/// - `since_ms` — earliest `timestamp_ms` to include (epoch millis)
/// - `limit` — max rows to return (default 100)
///
/// When the audit sink is unavailable (e.g. SQLite open failed at
/// startup) the tool returns an empty `rows` array and a
/// `sink_available=false` flag rather than `Err` — operators inspecting
/// the audit trail need to be able to ask the question even when the
/// sink itself is the broken thing.
pub fn audit_log_query(state: &AppState, arguments: &Value) -> ToolResult {
    let transaction_id = optional_string(arguments, "transaction_id");
    let since_ms = arguments.get("since_ms").and_then(|v| v.as_i64());
    let limit = optional_usize(arguments, "limit", 100).clamp(1, 1000);

    let Some(sink) = state.audit_sink() else {
        return Ok((
            json!({
                "sink_available": false,
                "rows": [],
                "filters": {
                    "transaction_id": transaction_id,
                    "since_ms": since_ms,
                    "limit": limit,
                },
            }),
            success_meta(BackendKind::Config, 1.0),
        ));
    };

    let rows = sink
        .query(transaction_id, since_ms, limit)
        .map_err(|error| CodeLensError::Internal(error.context("audit_log_query")))?;

    let serialised: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "transaction_id": r.transaction_id,
                "timestamp_ms": r.timestamp_ms,
                "principal": r.principal,
                "tool": r.tool,
                "args_hash": r.args_hash,
                "apply_status": r.apply_status,
                "state_from": r.state_from,
                "state_to": r.state_to,
                "evidence_hash": r.evidence_hash,
                "rollback_restored": r.rollback_restored,
                "error_message": r.error_message,
            })
        })
        .collect();

    Ok((
        json!({
            "sink_available": true,
            "rows": serialised,
            "filters": {
                "transaction_id": transaction_id,
                "since_ms": since_ms,
                "limit": limit,
            },
        }),
        success_meta(BackendKind::Config, 1.0),
    ))
}
