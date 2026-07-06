use super::transaction::unique_file_paths;
use crate::AppState;
use serde_json::{Value, json};

/// Upper bound on how many edited files a single semantic edit fans diagnostics
/// capture across. Beyond this the delta is reported as `not_captured` with a
/// reason so a large move/rename burst cannot balloon the edit hot path.
pub(super) const MAX_DIAGNOSTIC_CAPTURE_FILES: usize = 8;

/// Outcome of a single-file diagnostics snapshot taken through the shared
/// `get_file_diagnostics` path. `Unavailable` keeps the reason so the response
/// can distinguish "the file has no diagnostics" from "diagnostics could not
/// be checked".
pub(crate) enum DiagnosticsCapture {
    Captured(Vec<Value>),
    Unavailable(String),
}

/// Snapshot diagnostics for one file, reusing the exact LSP `command`/`args`
/// the edit already warmed. The session pool is keyed by (command, args), so
/// passing the same pair reuses the warm session instead of cold-starting a
/// language server on the edit hot path.
fn capture_file_diagnostics(
    state: &AppState,
    file_path: &str,
    command: &str,
    args: &[String],
) -> DiagnosticsCapture {
    let arguments = json!({
        "file_path": file_path,
        "command": command,
        "args": args,
    });
    match super::super::lsp::get_file_diagnostics(state, &arguments) {
        Ok((payload, _meta)) => {
            let diagnostics = payload
                .get("diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            DiagnosticsCapture::Captured(diagnostics)
        }
        Err(error) => DiagnosticsCapture::Unavailable(error.to_string()),
    }
}

/// Unique, non-empty files an edit touches, in sorted order, falling back to
/// the primary file when the transaction enumerates no edits.
pub(super) fn diagnostics_capture_targets(edit_files: &[String], primary: &str) -> Vec<String> {
    let mut targets = unique_file_paths(edit_files);
    if targets.is_empty() {
        targets.push(primary.to_owned());
    }
    targets
}

/// Snapshot each target file, pairing the file path with its capture so the pre
/// and post sets align by file.
pub(super) fn capture_diagnostics_set(
    state: &AppState,
    targets: &[String],
    command: &str,
    args: &[String],
) -> Vec<(String, DiagnosticsCapture)> {
    targets
        .iter()
        .map(|file| {
            (
                file.clone(),
                capture_file_diagnostics(state, file, command, args),
            )
        })
        .collect()
}

/// Response-facing before/after diagnostics plus the edit-introduced subset and
/// a status that keeps "empty" distinct from "clean".
pub(crate) struct DiagnosticsDelta {
    pub(crate) pre: Vec<Value>,
    pub(crate) post: Vec<Value>,
    pub(crate) introduced: Vec<Value>,
    pub(crate) status: &'static str,
    pub(crate) reason: Option<String>,
}

impl DiagnosticsDelta {
    /// The snapshot was intentionally skipped: dry-run preview, or an edit that
    /// never landed on disk. Distinct from `unavailable`.
    pub(crate) fn not_captured() -> Self {
        Self {
            pre: Vec::new(),
            post: Vec::new(),
            introduced: Vec::new(),
            status: "not_captured",
            reason: None,
        }
    }

    fn skipped(reason: String) -> Self {
        Self {
            pre: Vec::new(),
            post: Vec::new(),
            introduced: Vec::new(),
            status: "not_captured",
            reason: Some(reason),
        }
    }
}

/// Skip-or-build wrapper: an edit that fans out past the capture cap reports
/// `not_captured` with a reason on the apply path.
pub(super) fn finalize_diagnostics_delta(
    dry_run: bool,
    over_cap: bool,
    target_count: usize,
    pre: Option<Vec<(String, DiagnosticsCapture)>>,
    post: Option<Vec<(String, DiagnosticsCapture)>>,
) -> DiagnosticsDelta {
    if over_cap && !dry_run {
        return DiagnosticsDelta::skipped(format!(
            "{target_count} edited files exceed the diagnostics capture cap of {MAX_DIAGNOSTIC_CAPTURE_FILES}"
        ));
    }
    build_diagnostics_delta_for_files(pre, post)
}

/// Fold per-file pre/post captures into a single response delta.
pub(crate) fn build_diagnostics_delta_for_files(
    pre: Option<Vec<(String, DiagnosticsCapture)>>,
    post: Option<Vec<(String, DiagnosticsCapture)>>,
) -> DiagnosticsDelta {
    let (Some(pre), Some(post)) = (pre, post) else {
        return DiagnosticsDelta::not_captured();
    };
    if let Some(reason) = first_unavailable_reason(&pre).or_else(|| first_unavailable_reason(&post))
    {
        return DiagnosticsDelta {
            pre: Vec::new(),
            post: Vec::new(),
            introduced: Vec::new(),
            status: "unavailable",
            reason: Some(reason),
        };
    }
    let mut pre_all = Vec::new();
    let mut post_all = Vec::new();
    let mut introduced_all = Vec::new();
    for ((_, pre_capture), (_, post_capture)) in pre.iter().zip(post.iter()) {
        let pre_diags = captured_diagnostics(pre_capture);
        let post_diags = captured_diagnostics(post_capture);
        introduced_all.extend(scope_introduced_diagnostics(pre_diags, post_diags));
        pre_all.extend_from_slice(pre_diags);
        post_all.extend_from_slice(post_diags);
    }
    let status = resolve_delta_status(&introduced_all, &post_all);
    DiagnosticsDelta {
        pre: pre_all,
        post: post_all,
        introduced: introduced_all,
        status,
        reason: None,
    }
}

fn resolve_delta_status(introduced: &[Value], post: &[Value]) -> &'static str {
    if !introduced.is_empty() {
        "introduced"
    } else if post.is_empty() {
        "clean"
    } else {
        "preexisting"
    }
}

fn first_unavailable_reason(set: &[(String, DiagnosticsCapture)]) -> Option<String> {
    set.iter().find_map(|(_, capture)| match capture {
        DiagnosticsCapture::Unavailable(reason) => Some(reason.clone()),
        DiagnosticsCapture::Captured(_) => None,
    })
}

fn captured_diagnostics(capture: &DiagnosticsCapture) -> &[Value] {
    match capture {
        DiagnosticsCapture::Captured(diagnostics) => diagnostics,
        DiagnosticsCapture::Unavailable(_) => &[],
    }
}

/// Diagnostics present after the edit that have no counterpart from before it.
pub(crate) fn scope_introduced_diagnostics(pre: &[Value], post: &[Value]) -> Vec<Value> {
    let mut consumed = vec![false; pre.len()];
    let mut introduced = Vec::new();
    for post_diag in post {
        let post_identity = diagnostic_identity(post_diag);
        let post_line = diagnostic_line(post_diag);
        let mut best: Option<(usize, u64)> = None;
        for (index, pre_diag) in pre.iter().enumerate() {
            if consumed[index] || diagnostic_identity(pre_diag) != post_identity {
                continue;
            }
            let distance = match (post_line, diagnostic_line(pre_diag)) {
                (Some(a), Some(b)) => a.abs_diff(b),
                _ => u64::MAX,
            };
            if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                best = Some((index, distance));
            }
        }
        match best {
            Some((index, _)) => consumed[index] = true,
            None => introduced.push(post_diag.clone()),
        }
    }
    introduced
}

fn diagnostic_identity(diagnostic: &Value) -> (String, String) {
    let code = diagnostic
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let message = diagnostic
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    (code, message)
}

fn diagnostic_line(diagnostic: &Value) -> Option<u64> {
    diagnostic.get("line").and_then(Value::as_u64)
}

/// Whether the transaction actually landed on disk. `Applied` is the only
/// status that mutated files.
pub(crate) fn edit_applied_from_evidence(
    evidence: Option<&codelens_engine::ApplyEvidence>,
) -> bool {
    matches!(
        evidence.map(|ev| ev.status),
        Some(codelens_engine::ApplyStatus::Applied)
    )
}
