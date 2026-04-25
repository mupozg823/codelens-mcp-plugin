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
