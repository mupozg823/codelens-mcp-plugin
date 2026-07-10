//! Integration tests for mutation primitive responses including ApplyEvidence
//! evidence keys (file_hashes_before/after, apply_status, rollback_report,
//! modified_files, edit_count) — covers Phase 1 G7 acceptance criteria
//! AC-4 (M1/M2/M3) and AC-6 (M5 Hybrid rollback).
//!
//! M4 (mcp-layer TOCTOU) is deferred: the engine's `#[cfg(test)]`
//! `FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY` hook is `pub(crate)` and
//! not accessible from the mcp crate. Engine T3
//! (`apply_full_write_toctou_mismatch_via_inject_hook`) covers TOCTOU at
//! the substrate level.

use super::*;

/// Tests that mutate the `CODELENS_PRINCIPAL` env var must hold this
/// guard. `unsafe { env::set_var(...) }` is process-global, so two
/// such tests running in parallel race on the snapshot the role gate /
/// audit_sink read. The mutex serialises only the env-mutating
/// section; tests that do not touch the env are unaffected.
fn principal_env_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn audit_log_query_denied_for_non_admin_principal() {
    // P2-F: the role gate must reject `audit_log_query` for any
    // principal whose role is below Admin. Default principals.toml
    // resolution lands every id at Refactor (permissive default), so
    // the call must surface a PermissionDenied response without ever
    // reading the sink.
    let project = project_root();
    let state = make_state(&project);
    let response = call_tool(&state, "audit_log_query", json!({}));
    assert_eq!(
        response["success"], false,
        "non-admin must be denied, response={response}"
    );
    assert!(
        response["error"]
            .as_str()
            .is_some_and(|e| e.contains("requires role=Admin")),
        "denial must name Admin as required role, response={response}"
    );
}

/// ADR-0009 §3 (P2-D lifecycle): a successful mutation now writes
/// `state_to=Audited`, `evidence_hash` populated (canonical sha256 of
/// the response payload's data subobject), `principal` set when
/// CODELENS_PRINCIPAL is bound. This builds on the
/// create_text_file_writes_audit_sink_row test from P2-B by checking
/// the new fields. (Vehicle: insert_after_symbol — the line-edit family
/// was tombstoned, #346.)
#[test]
fn audit_outcome_row_carries_evidence_hash_and_correct_terminal_state() {
    let _guard = principal_env_guard();
    let project = project_root();
    let saved_principal = std::env::var("CODELENS_PRINCIPAL").ok();
    unsafe {
        std::env::set_var("CODELENS_PRINCIPAL", "p2d-test-user");
    }
    let state = make_state(&project);
    fs::write(
        project.as_path().join("p2d_audit_target.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let _ = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "p2d_audit_target.py",
            "symbol_name": "alpha",
            "content": "\ndef beta():\n    return 2\n",
        }),
    );
    let sink = state.audit_sink().expect("audit_sink available");
    let rows = sink.query(None, None, 100).expect("query");
    let row = rows
        .iter()
        .find(|r| r.tool == "insert_after_symbol" && r.apply_status == "applied")
        .expect("applied audit row");

    assert_eq!(row.state_to, "Audited", "Ok mutation reaches Audited");
    assert_eq!(row.state_from.as_deref(), Some("Applying"));
    assert!(
        row.evidence_hash.is_some(),
        "P2-D must populate evidence_hash on Ok branch"
    );
    let hash = row.evidence_hash.as_deref().unwrap();
    assert_eq!(
        hash.len(),
        64,
        "evidence_hash must be 64-char sha256 hex, got {hash:?}"
    );
    assert_eq!(
        row.principal.as_deref(),
        Some("p2d-test-user"),
        "principal must reflect CODELENS_PRINCIPAL env"
    );

    unsafe {
        match saved_principal {
            Some(v) => std::env::set_var("CODELENS_PRINCIPAL", v),
            None => std::env::remove_var("CODELENS_PRINCIPAL"),
        }
    }
}

/// ADR-0009 §3 (P2-D lifecycle): when a mutation tool returns Err
/// (pre-substrate validation rejected the call, e.g. line out of
/// range), the audit log gets a `state_to=Failed`, `apply_status=failed`
/// row carrying the error message. Mirrors the success-path audit row
/// shape so `audit_log_query` (P2-F) can reconstruct full lifecycle.
#[test]
fn audit_failure_row_recorded_for_error_response() {
    let project = project_root();
    let state = make_state(&project);

    // insert_after_symbol on a symbol that does not exist forces the
    // engine primitive to bail. The Err propagates back to dispatch's
    // match arm. (Old vehicle delete_lines was tombstoned, #346.)
    fs::write(
        project.as_path().join("p2d_failure_target.py"),
        "def only():\n    return 1\n",
    )
    .unwrap();
    let err_response = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "p2d_failure_target.py",
            "symbol_name": "ghost_symbol_does_not_exist",
            "content": "\ndef nope():\n    return 0\n",
        }),
    );
    assert_eq!(
        err_response.get("success").and_then(|v| v.as_bool()),
        Some(false),
        "missing-symbol insert_after_symbol must surface success=false, got {err_response}"
    );

    let sink = state.audit_sink().expect("audit_sink available");
    let rows = sink.query(None, None, 100).expect("query");
    let failure_row = rows
        .iter()
        .find(|r| r.tool == "insert_after_symbol" && r.apply_status == "failed")
        .expect("failed audit row must exist");

    assert_eq!(
        failure_row.state_to, "Failed",
        "Err response writes terminal Failed"
    );
    assert_eq!(failure_row.state_from.as_deref(), Some("Verifying"));
    assert!(
        failure_row.evidence_hash.is_none(),
        "no evidence on Err branch"
    );
    let msg = failure_row
        .error_message
        .as_deref()
        .expect("error_message populated on Err");
    assert!(
        msg.contains("not found") && msg.contains("ghost_symbol_does_not_exist"),
        "error_message must reflect the missing-symbol cause, got {msg:?}"
    );
}

#[test]
fn missing_required_parameter_is_recorded_as_failed_operation() {
    let project = project_root();
    let state = make_state(&project);

    let _response = call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "missing-content"}),
    );

    let sink = state.audit_sink().expect("audit sink available");
    let rows = sink.query(None, None, 100).expect("query");
    let row = rows
        .iter()
        .find(|row| row.tool == "write_memory" && row.apply_status == "failed")
        .expect("required-parameter failure must be audited");
    assert_eq!(row.state_to, "Failed");
    assert!(uuid::Uuid::parse_str(&row.operation_id).is_ok());
    assert!(
        row.error_message
            .as_deref()
            .is_some_and(|message| message.contains("content"))
    );
}

#[test]
fn mutation_fails_closed_when_audit_sink_cannot_open() {
    let project = project_root();
    let codelens_dir = project.as_path().join(".codelens");
    fs::create_dir_all(&codelens_dir).unwrap();
    fs::write(
        codelens_dir.join("principals.toml"),
        "[default]\nrole = \"Refactor\"\n",
    )
    .unwrap();
    fs::write(codelens_dir.join("audit"), "not a directory").unwrap();
    let target = project.as_path().join("audit_fail_closed.py");
    fs::write(&target, "def alpha():\n    return 1\n").unwrap();
    let state = make_state(&project);

    let response = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "audit_fail_closed.py",
            "symbol_name": "alpha",
            "content": "\ndef beta():\n    return 2\n",
        }),
    );

    assert_eq!(
        response["success"], false,
        "audit failure must reject mutation"
    );
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "def alpha():\n    return 1\n",
        "mutation must not run when its audit operation cannot be opened"
    );
}

#[test]
fn surface_denial_is_recorded_under_one_uuid_operation() {
    let project = project_root();
    let state = make_state(&project);
    let profile = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );
    assert_eq!(profile["success"], true);

    let denied = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "blocked.py",
            "symbol_name": "alpha",
            "content": "\ndef beta():\n    pass\n",
        }),
    );
    assert_eq!(denied["success"], false);

    let sink = state.audit_sink().expect("audit sink available");
    let rows = sink.query(None, None, 100).expect("query rows");
    let row = rows
        .iter()
        .find(|row| row.tool == "insert_after_symbol")
        .expect("surface denial must be audited");
    assert_eq!(row.apply_status, "denied");
    assert_eq!(row.state_to, "Denied");
    assert!(uuid::Uuid::parse_str(&row.operation_id).is_ok());
}

/// ADR-0009 §1 (P2-C role gate): when `CODELENS_PRINCIPAL` env var is
/// set and `principals.toml` maps that principal to `ReadOnly`, every
/// mutation tool call is denied with a JSON-RPC -32008 error AND an
/// `apply_status="denied"` row is appended to the audit log. Read-only
/// tools (e.g. `find_symbol`) still succeed.
///
/// `permissive_default` (no `principals.toml`, no env var) is covered
/// implicitly by every other test in this file — they would all fail
/// if the gate ever defaulted to `ReadOnly`.
#[test]
fn role_gate_denies_mutation_for_read_only_principal() {
    let _guard = principal_env_guard();
    use crate::audit_sink::AuditSink;
    use crate::principals::Principals;

    let project = project_root();

    // Drop a `principals.toml` in the project's `.codelens/` so
    // `Principals::discover` finds it via the audit_dir parent.
    let codelens_dir = project.as_path().join(".codelens");
    std::fs::create_dir_all(&codelens_dir).unwrap();
    std::fs::write(
        codelens_dir.join("principals.toml"),
        r#"
[default]
role = "Refactor"

[principal."ci-bot"]
role = "ReadOnly"
"#,
    )
    .unwrap();

    // Sanity-check the loader path before touching dispatch.
    let parsed =
        Principals::discover(&codelens_dir.join("audit")).expect("principals.toml must parse");
    assert_eq!(parsed.resolve(Some("ci-bot")).as_str(), "ReadOnly");

    // Bind the request principal to ci-bot via env so role_gate
    // resolves it on the dispatch path.
    let saved_principal = std::env::var("CODELENS_PRINCIPAL").ok();
    unsafe {
        std::env::set_var("CODELENS_PRINCIPAL", "ci-bot");
    }

    let state = make_state(&project);

    // Mutation tool MUST be denied.
    let denied = call_tool(
        &state,
        "replace_symbol_body",
        json!({
            "relative_path": "role_gate_target.py",
            "symbol_name": "alpha",
            "new_body": "    return 2",
        }),
    );
    assert_eq!(
        denied.get("success").and_then(|v| v.as_bool()),
        Some(false),
        "denied response must have success=false, got {denied}"
    );
    let error_msg = denied
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default();
    assert!(
        error_msg.contains("Permission denied"),
        "error message must mention Permission denied, got {error_msg:?}"
    );
    assert!(
        error_msg.contains("ci-bot") && error_msg.contains("ReadOnly"),
        "error message must include principal id and role, got {error_msg:?}"
    );

    // ReadOnly tool MUST still succeed (find_symbol exists and is
    // gated to ReadOnly).
    let allowed = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "anything",
        }),
    );
    // We do not assert on shape — only that the call did not get a
    // PermissionDenied error.
    let allowed_err = allowed
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default();
    assert!(
        !allowed_err.contains("Permission denied"),
        "ReadOnly tool must NOT be denied for ReadOnly principal, got error={allowed_err:?}"
    );

    // Audit row for the denial must exist.
    let sink = AuditSink::open(&state.audit_dir()).expect("audit_sink open");
    let rows = sink.query(None, None, 100).expect("query");
    let denied_row = rows
        .iter()
        .find(|r| r.tool == "replace_symbol_body" && r.apply_status == "denied")
        .expect("denied audit row must exist");
    assert_eq!(denied_row.state_to, "Denied");
    assert_eq!(denied_row.principal.as_deref(), Some("ci-bot"));
    assert!(
        denied_row
            .error_message
            .as_deref()
            .unwrap_or("")
            .contains("ReadOnly"),
        "denied row error_message must reference principal_role, got {:?}",
        denied_row.error_message
    );

    // Restore env so other tests are unaffected.
    unsafe {
        match saved_principal {
            Some(v) => std::env::set_var("CODELENS_PRINCIPAL", v),
            None => std::env::remove_var("CODELENS_PRINCIPAL"),
        }
    }
}
