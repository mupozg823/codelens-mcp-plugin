use super::diagnostics::*;
use serde_json::{Value, json};

fn diag(code: &str, message: &str, line: u64) -> Value {
    json!({ "code": code, "message": message, "line": line })
}

fn diag_in(file: &str, code: &str, message: &str, line: u64) -> Value {
    json!({ "file_path": file, "code": code, "message": message, "line": line })
}

fn file_set(file: &str, capture: DiagnosticsCapture) -> Vec<(String, DiagnosticsCapture)> {
    vec![(file.to_owned(), capture)]
}

fn evidence(status: codelens_engine::ApplyStatus) -> codelens_engine::ApplyEvidence {
    codelens_engine::ApplyEvidence {
        status,
        file_hashes_before: std::collections::BTreeMap::new(),
        file_hashes_after: std::collections::BTreeMap::new(),
        rollback_report: Vec::new(),
        modified_files: 1,
        edit_count: 1,
    }
}

#[test]
fn introduced_scoping_flags_only_the_new_error() {
    let pre = vec![diag("E0308", "mismatched types", 5)];
    let post = vec![
        diag("E0308", "mismatched types", 5),
        diag("E0412", "cannot find type `Foo`", 42),
    ];
    let introduced = scope_introduced_diagnostics(&pre, &post);
    assert_eq!(
        introduced,
        vec![diag("E0412", "cannot find type `Foo`", 42)]
    );
}

#[test]
fn introduced_scoping_ignores_shifted_but_unchanged_diagnostics() {
    let pre = vec![diag("E0308", "mismatched types", 10)];
    let post = vec![diag("E0308", "mismatched types", 13)];
    assert!(scope_introduced_diagnostics(&pre, &post).is_empty());
}

#[test]
fn introduced_scoping_counts_duplicate_identities() {
    let pre = vec![diag("W0", "unused variable", 3)];
    let post = vec![
        diag("W0", "unused variable", 3),
        diag("W0", "unused variable", 90),
    ];
    let introduced = scope_introduced_diagnostics(&pre, &post);
    assert_eq!(introduced, vec![diag("W0", "unused variable", 90)]);
}

#[test]
fn status_clean_when_no_diagnostics_after_edit() {
    let delta = build_diagnostics_delta_for_files(
        Some(file_set(
            "a.rs",
            DiagnosticsCapture::Captured(vec![diag("E0308", "boom", 1)]),
        )),
        Some(file_set("a.rs", DiagnosticsCapture::Captured(Vec::new()))),
    );
    assert_eq!(delta.status, "clean");
    assert!(delta.introduced.is_empty());
    assert_eq!(delta.reason, None);
}

#[test]
fn status_introduced_when_edit_adds_a_diagnostic() {
    let delta = build_diagnostics_delta_for_files(
        Some(file_set("a.rs", DiagnosticsCapture::Captured(Vec::new()))),
        Some(file_set(
            "a.rs",
            DiagnosticsCapture::Captured(vec![diag("E0412", "no type", 7)]),
        )),
    );
    assert_eq!(delta.status, "introduced");
    assert_eq!(delta.introduced, vec![diag("E0412", "no type", 7)]);
}

#[test]
fn status_preexisting_when_diagnostics_remain_but_none_new() {
    let delta = build_diagnostics_delta_for_files(
        Some(file_set(
            "a.rs",
            DiagnosticsCapture::Captured(vec![diag("E0308", "boom", 1)]),
        )),
        Some(file_set(
            "a.rs",
            DiagnosticsCapture::Captured(vec![diag("E0308", "boom", 1)]),
        )),
    );
    assert_eq!(delta.status, "preexisting");
    assert!(delta.introduced.is_empty());
}

#[test]
fn status_not_captured_when_snapshot_skipped() {
    let delta = build_diagnostics_delta_for_files(None, None);
    assert_eq!(delta.status, "not_captured");
    assert!(delta.pre.is_empty() && delta.post.is_empty());
    assert_eq!(delta.reason, None);
}

#[test]
fn status_unavailable_distinguished_from_empty() {
    let pre_fail = build_diagnostics_delta_for_files(
        Some(file_set(
            "a.rs",
            DiagnosticsCapture::Unavailable("no lsp mapping".into()),
        )),
        Some(file_set("a.rs", DiagnosticsCapture::Captured(Vec::new()))),
    );
    assert_eq!(pre_fail.status, "unavailable");
    assert_eq!(pre_fail.reason.as_deref(), Some("no lsp mapping"));

    let post_fail = build_diagnostics_delta_for_files(
        Some(file_set("a.rs", DiagnosticsCapture::Captured(Vec::new()))),
        Some(file_set(
            "a.rs",
            DiagnosticsCapture::Unavailable("server crashed".into()),
        )),
    );
    assert_eq!(post_fail.status, "unavailable");
    assert_eq!(post_fail.reason.as_deref(), Some("server crashed"));
}

#[test]
fn multi_file_delta_flags_new_diagnostic_in_destination_file() {
    let pre = vec![
        (
            "src.rs".to_owned(),
            DiagnosticsCapture::Captured(Vec::new()),
        ),
        (
            "dst.rs".to_owned(),
            DiagnosticsCapture::Captured(Vec::new()),
        ),
    ];
    let post = vec![
        (
            "src.rs".to_owned(),
            DiagnosticsCapture::Captured(Vec::new()),
        ),
        (
            "dst.rs".to_owned(),
            DiagnosticsCapture::Captured(vec![diag_in("dst.rs", "E0432", "unresolved import", 3)]),
        ),
    ];
    let delta = build_diagnostics_delta_for_files(Some(pre), Some(post));
    assert_eq!(delta.status, "introduced");
    assert_eq!(
        delta.introduced,
        vec![diag_in("dst.rs", "E0432", "unresolved import", 3)]
    );
    assert_eq!(delta.introduced[0]["file_path"], json!("dst.rs"));
}

#[test]
fn multi_file_unavailable_when_any_file_uncheckable() {
    let pre = vec![
        ("a.rs".to_owned(), DiagnosticsCapture::Captured(Vec::new())),
        (
            "b.rs".to_owned(),
            DiagnosticsCapture::Unavailable("no lsp".to_owned()),
        ),
    ];
    let post = vec![
        ("a.rs".to_owned(), DiagnosticsCapture::Captured(Vec::new())),
        ("b.rs".to_owned(), DiagnosticsCapture::Captured(Vec::new())),
    ];
    let delta = build_diagnostics_delta_for_files(Some(pre), Some(post));
    assert_eq!(delta.status, "unavailable");
    assert_eq!(delta.reason.as_deref(), Some("no lsp"));
}

#[test]
fn finalize_reports_not_captured_with_reason_over_cap() {
    let delta = finalize_diagnostics_delta(false, true, 12, None, None);
    assert_eq!(delta.status, "not_captured");
    assert!(delta.reason.as_deref().unwrap().contains("exceed"));
}

#[test]
fn finalize_delegates_within_cap() {
    let delta = finalize_diagnostics_delta(true, false, 1, None, None);
    assert_eq!(delta.status, "not_captured");
    assert_eq!(delta.reason, None);
}

#[test]
fn capture_targets_fall_back_to_primary_when_empty() {
    assert_eq!(
        diagnostics_capture_targets(&[], "main.rs"),
        vec!["main.rs".to_owned()]
    );
}

#[test]
fn capture_targets_dedup_edit_files() {
    let files = vec!["b.rs".to_owned(), "a.rs".to_owned(), "b.rs".to_owned()];
    assert_eq!(
        diagnostics_capture_targets(&files, "main.rs"),
        vec!["a.rs".to_owned(), "b.rs".to_owned()]
    );
}

#[test]
fn edit_applied_true_only_for_applied_status() {
    assert!(edit_applied_from_evidence(Some(&evidence(
        codelens_engine::ApplyStatus::Applied
    ))));
    assert!(!edit_applied_from_evidence(Some(&evidence(
        codelens_engine::ApplyStatus::RolledBack
    ))));
    assert!(!edit_applied_from_evidence(Some(&evidence(
        codelens_engine::ApplyStatus::NoOp
    ))));
    assert!(!edit_applied_from_evidence(None));
}
