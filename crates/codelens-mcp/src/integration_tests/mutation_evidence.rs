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
fn replace_lines_tool_response_includes_evidence() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(project.as_path().join("evidence_replace.txt"), "a\nb\nc\n").unwrap();
    let result = call_tool(
        &state,
        "replace_lines",
        json!({
            "relative_path": "evidence_replace.txt",
            "start_line": 2,
            "end_line": 3,
            "new_content": "REPLACED\n",
        }),
    );
    let data = &result["data"];
    assert_eq!(
        data["apply_status"], "applied",
        "expected apply_status=applied, response={result}"
    );
    assert_eq!(data["modified_files"], 1, "modified_files=1 single file");
    assert_eq!(data["edit_count"], 1, "edit_count=1 single edit");
    assert!(
        data["file_hashes_before"].is_object(),
        "file_hashes_before object present, got {:?}",
        data["file_hashes_before"]
    );
    assert!(
        data["file_hashes_after"].is_object(),
        "file_hashes_after object present"
    );
    assert!(
        data["rollback_report"]
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "rollback_report empty on happy path, got {:?}",
        data["rollback_report"]
    );
    // Phase 0 envelope still present.
    assert_eq!(
        data["edit_authority"]["kind"], "raw_fs",
        "Phase 0 raw_fs envelope intact"
    );
}

#[test]
fn create_text_file_tool_response_includes_evidence() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({
            "relative_path": "evidence_fresh.txt",
            "content": "hello\n",
            "overwrite": false,
        }),
    );
    let data = &result["data"];
    assert_eq!(
        data["apply_status"], "applied",
        "expected apply_status=applied, response={result}"
    );
    assert_eq!(data["modified_files"], 1);
    assert_eq!(data["edit_count"], 1);
    let after = data["file_hashes_after"]
        .as_object()
        .expect("file_hashes_after object");
    assert!(
        after.contains_key("evidence_fresh.txt"),
        "after has fresh entry, got keys: {:?}",
        after.keys().collect::<Vec<_>>()
    );
    let before = data["file_hashes_before"]
        .as_object()
        .expect("file_hashes_before object");
    assert!(
        !before.contains_key("evidence_fresh.txt"),
        "before has no entry for new file, got keys: {:?}",
        before.keys().collect::<Vec<_>>()
    );
}

#[test]
fn mutation_response_surfaces_invalidated_paths() {
    // P2-E: every mutation tool response carries the list of file
    // paths whose engine caches were invalidated, so the agent can
    // act on stale-cache risk without an extra round-trip.
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({
            "relative_path": "p2e_invalidation.txt",
            "content": "fresh\n",
            "overwrite": false,
        }),
    );
    let data = &result["data"];
    let invalidated = data["invalidated_paths"]
        .as_array()
        .expect("invalidated_paths must be present and an array");
    assert_eq!(
        invalidated.len(),
        1,
        "single-file mutation must report one invalidated path, got {invalidated:?}"
    );
    assert_eq!(invalidated[0], "p2e_invalidation.txt");
}

#[test]
fn add_import_tool_response_includes_evidence() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(
        project.as_path().join("evidence_module.py"),
        "def existing():\n    pass\n",
    )
    .unwrap();
    let result = call_tool(
        &state,
        "add_import",
        json!({
            "file_path": "evidence_module.py",
            "import_statement": "import os",
        }),
    );
    let data = &result["data"];
    assert_eq!(
        data["apply_status"], "applied",
        "expected apply_status=applied, response={result}"
    );
    assert_eq!(data["modified_files"], 1);
    assert!(
        data["file_hashes_before"].is_object(),
        "file_hashes_before object present"
    );
    assert!(
        data["file_hashes_after"].is_object(),
        "file_hashes_after object present"
    );
}

/// ADR-0009 §3 (P2-D lifecycle): a successful mutation now writes
/// `state_to=Audited`, `evidence_hash` populated (canonical sha256 of
/// the response payload's data subobject), `principal` set when
/// CODELENS_PRINCIPAL is bound. This builds on the
/// create_text_file_writes_audit_sink_row test from P2-B by checking
/// the new fields.
#[test]
fn audit_outcome_row_carries_evidence_hash_and_correct_terminal_state() {
    let _guard = principal_env_guard();
    let project = project_root();
    let saved_principal = std::env::var("CODELENS_PRINCIPAL").ok();
    unsafe {
        std::env::set_var("CODELENS_PRINCIPAL", "p2d-test-user");
    }
    let state = make_state(&project);
    let _ = call_tool(
        &state,
        "create_text_file",
        json!({
            "relative_path": "p2d_audit_target.txt",
            "content": "p2d\n",
            "overwrite": false,
        }),
    );
    let sink = state.audit_sink().expect("audit_sink available");
    let rows = sink.query(None, None, 100).expect("query");
    let row = rows
        .iter()
        .find(|r| r.tool == "create_text_file" && r.apply_status == "applied")
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

    // delete_lines with start_line=99 in a 1-line file forces the
    // engine primitive to bail with `start_line out of range`. The
    // Err propagates back to dispatch's match arm.
    fs::write(project.as_path().join("p2d_failure_target.txt"), "only\n").unwrap();
    let err_response = call_tool(
        &state,
        "delete_lines",
        json!({
            "relative_path": "p2d_failure_target.txt",
            "start_line": 99,
            "end_line": 100,
        }),
    );
    assert_eq!(
        err_response.get("success").and_then(|v| v.as_bool()),
        Some(false),
        "out-of-range delete_lines must surface success=false, got {err_response}"
    );

    let sink = state.audit_sink().expect("audit_sink available");
    let rows = sink.query(None, None, 100).expect("query");
    let failure_row = rows
        .iter()
        .find(|r| r.tool == "delete_lines" && r.apply_status == "failed")
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
        msg.contains("out of range") || msg.contains("start_line"),
        "error_message must reflect the error cause, got {msg:?}"
    );
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
        "create_text_file",
        json!({
            "relative_path": "role_gate_target.txt",
            "content": "should-not-be-written\n",
            "overwrite": false,
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
        .find(|r| r.tool == "create_text_file" && r.apply_status == "denied")
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

/// ADR-0009 §2 (P2-B wiring): a successful mutation tool call writes
/// exactly one row to the durable audit_sink (SQLite) with
/// `apply_status="applied"`, transition `Applying → Audited`, and a
/// `transaction_id` derived from session + tool + args_hash. The legacy
/// jsonl audit (`mutation_audit.rs`) still runs in parallel; this test
/// only asserts the new SQLite path.
#[test]
fn create_text_file_writes_audit_sink_row() {
    let project = project_root();
    let state = make_state(&project);
    let _ = call_tool(
        &state,
        "create_text_file",
        json!({
            "relative_path": "audit_evidence_target.txt",
            "content": "audit-row-proof\n",
            "overwrite": false,
        }),
    );
    let sink = state
        .audit_sink()
        .expect("audit_sink must be available for default project");
    let rows = sink
        .query(None, None, 100)
        .expect("audit_sink query should succeed");
    let matching: Vec<_> = rows
        .into_iter()
        .filter(|r| r.tool == "create_text_file")
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly 1 audit row for create_text_file, got {}",
        matching.len()
    );
    let row = &matching[0];
    assert_eq!(row.apply_status, "applied");
    assert_eq!(row.state_from.as_deref(), Some("Applying"));
    assert_eq!(row.state_to, "Audited");
    assert!(
        !row.transaction_id.is_empty(),
        "transaction_id must be populated"
    );
    assert!(
        !row.args_hash.is_empty(),
        "args_hash must be populated (canonical sha256)"
    );
    assert_eq!(row.args_hash.len(), 64, "sha256 hex is 64 chars");
}

/// M5: Hybrid rollback contract — when fs::write fails, response is Ok
/// (not Err) with apply_status="rolled_back" and error_message synthesised
/// from rollback_report[].reason.
///
/// Test setup: chmod the target file to 0o444 (read-only). Phase 1 capture
/// (read) succeeds, Phase 2 verify (read) succeeds, Phase 3 fs::write fails
/// with EACCES. Substrate restore-write also fails (same 0o444), so
/// rollback_report[0].restored=false. Either way, the Hybrid contract
/// surface is honoured.
#[cfg(unix)]
#[test]
fn replace_lines_tool_e4_rollback_response_shape() {
    use std::os::unix::fs::PermissionsExt;
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("evidence_ro.txt");
    fs::write(&path, "original\n").unwrap();
    // Force fs::write failure: file is r--r--r--, no write bit for anyone.
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&path, perms).unwrap();

    let result = call_tool(
        &state,
        "replace_lines",
        json!({
            "relative_path": "evidence_ro.txt",
            "start_line": 1,
            "end_line": 2,
            "new_content": "new\n",
        }),
    );

    // Restore perms before assertions so tempdir cleanup works.
    let mut restore = fs::metadata(&path).unwrap().permissions();
    restore.set_mode(0o644);
    fs::set_permissions(&path, restore).unwrap();

    let data = &result["data"];
    assert_eq!(
        data["apply_status"], "rolled_back",
        "Hybrid: apply failure surfaces as Ok+apply_status=rolled_back, response={result}"
    );
    assert!(
        data["error_message"].is_string(),
        "Hybrid: error_message must be present, got {:?}",
        data["error_message"]
    );
    let report = data["rollback_report"]
        .as_array()
        .expect("rollback_report array");
    assert_eq!(
        report.len(),
        1,
        "rollback_report has 1 entry for the single file"
    );
    assert_eq!(
        report[0]["file_path"], "evidence_ro.txt",
        "rollback entry references the target file"
    );
    // Phase 0 envelope still present even on rollback path.
    assert_eq!(
        data["edit_authority"]["kind"], "raw_fs",
        "Phase 0 raw_fs envelope intact on rollback response"
    );
}
