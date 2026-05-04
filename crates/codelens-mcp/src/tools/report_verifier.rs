use super::AppState;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::state::{AnalysisReadiness, AnalysisVerifierCheck};

pub(crate) const VERIFIER_READY: &str = "ready";
pub(crate) const VERIFIER_CAUTION: &str = "caution";
pub(crate) const VERIFIER_BLOCKED: &str = "blocked";

#[derive(Default)]
pub(crate) struct VerifierContract {
    pub(crate) blockers: Vec<String>,
    pub(crate) readiness: AnalysisReadiness,
    pub(crate) verifier_checks: Vec<AnalysisVerifierCheck>,
}

fn push_verifier_check(
    checks: &mut Vec<AnalysisVerifierCheck>,
    check: &str,
    status: &str,
    summary: impl Into<String>,
    evidence_section: Option<&str>,
) {
    if checks.len() >= 6 {
        return;
    }
    checks.push(AnalysisVerifierCheck {
        check: check.to_owned(),
        status: status.to_owned(),
        summary: summary.into(),
        evidence_section: evidence_section.map(ToOwned::to_owned),
    });
}

fn verifier_status_rank(status: &str) -> u8 {
    match status {
        VERIFIER_BLOCKED => 2,
        VERIFIER_CAUTION => 1,
        _ => 0,
    }
}

fn combine_verifier_status<'a>(statuses: &[&'a str]) -> &'a str {
    statuses
        .iter()
        .copied()
        .max_by_key(|status| verifier_status_rank(status))
        .unwrap_or(VERIFIER_READY)
}

fn normalized_touched_files(touched_files: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for file in touched_files {
        let trimmed = file.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !files.iter().any(|existing| existing == trimmed) {
            files.push(trimmed.to_owned());
        }
        if files.len() >= 4 {
            break;
        }
    }
    files
}

fn browser_or_ssr_sensitive(
    touched_files: &[String],
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
) -> bool {
    let has_frontend_file = touched_files.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        p.contains("/templates/")
            || p.contains("/components/")
            || p.contains("/pages/")
            || p.contains("/views/")
            || p.contains("/ui/")
            || p.ends_with(".tsx")
            || p.ends_with(".jsx")
            || p.ends_with(".vue")
            || p.ends_with(".svelte")
            || p.ends_with(".html")
            || p.ends_with(".css")
            || p.ends_with(".scss")
            || p.ends_with(".astro")
    });
    if !has_frontend_file {
        return false;
    }
    let combined = format!(
        "{} {} {}",
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    [
        "browser", "frontend", "layout", "modal", "render", "route", "ssr",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

/// Issue #226: classify a single LSP diagnostic for the
/// readiness-blocking decision. `hint` and `information` severities
/// — and rust-analyzer's `inactive-code` in particular — are
/// expected, advisory metadata for cfg-gated branches and similar
/// situations; treating them as blockers trains agents to distrust
/// the readiness signal.
///
/// Returns `true` only when the diagnostic represents an actual
/// problem the caller should resolve before mutating: i.e. severity
/// is `error` or `warning`, or the severity label is missing
/// (defensive default for unknown LSP servers).
fn is_blocking_diagnostic(diag: &Value) -> bool {
    // rust-analyzer emits cfg-gated branches as `inactive-code` hints —
    // explicit downgrade so the verdict is not driven by feature gate
    // bookkeeping.
    if diag
        .get("code")
        .and_then(|v| v.as_str())
        .is_some_and(|c| c.eq_ignore_ascii_case("inactive-code"))
    {
        return false;
    }
    let label = diag
        .get("severity_label")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase);
    match label.as_deref() {
        Some("hint") | Some("information") | Some("info") => false,
        Some("warning") | Some("warn") | Some("error") | Some("err") => true,
        // No severity label at all → treat as blocking (defensive
        // default; unknown LSP servers should not silently be ignored).
        Some(_) | None => true,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_verifier_contract(
    state: &AppState,
    tool_name: &str,
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
    sections: &mut BTreeMap<String, Value>,
    touched_files: &[String],
    symbol_hint: Option<&str>,
) -> VerifierContract {
    let touched_files = normalized_touched_files(touched_files);
    let mut contract = VerifierContract::default();

    let mut diagnostic_rows = Vec::new();
    let mut diagnostic_errors = Vec::new();
    let mut diagnostic_count = 0usize;
    let mut blocking_diagnostic_count = 0usize;
    let mut informational_diagnostic_count = 0usize;
    for file in touched_files.iter().take(3) {
        match super::lsp::get_file_diagnostics(
            state,
            &json!({"file_path": file, "max_results": 20}),
        ) {
            Ok((payload, _meta)) => {
                let count = payload
                    .get("count")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default() as usize;
                diagnostic_count += count;
                let diagnostics_array = payload
                    .get("diagnostics")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                // Issue #226: rust-analyzer emits `inactive-code` etc.
                // as `severity_label: "hint"` for cfg-gated branches,
                // which is informational metadata, not a blocker.
                // Partition diagnostics so the readiness verdict is
                // driven by warning/error severities only.
                let mut file_blocking = 0usize;
                let mut file_informational = 0usize;
                if let Some(items) = diagnostics_array.as_array() {
                    for diag in items {
                        if is_blocking_diagnostic(diag) {
                            file_blocking += 1;
                        } else {
                            file_informational += 1;
                        }
                    }
                }
                blocking_diagnostic_count += file_blocking;
                informational_diagnostic_count += file_informational;
                diagnostic_rows.push(json!({
                    "file_path": file,
                    "count": count,
                    "blocking_count": file_blocking,
                    "informational_count": file_informational,
                    "diagnostics": diagnostics_array,
                }));
            }
            Err(error) => diagnostic_errors.push(json!({
                "file_path": file,
                "error": error.to_string(),
            })),
        }
    }
    let diagnostics_section = if !diagnostic_rows.is_empty() || !diagnostic_errors.is_empty() {
        sections.insert(
            "verifier_diagnostics".to_owned(),
            json!({
                "files": diagnostic_rows,
                "errors": diagnostic_errors,
                "blocking_count": blocking_diagnostic_count,
                "informational_count": informational_diagnostic_count,
            }),
        );
        Some("verifier_diagnostics")
    } else {
        None
    };
    let diagnostics_status = if blocking_diagnostic_count > 0 {
        crate::util::push_unique_string(
            &mut contract.blockers,
            "Resolve reported diagnostics before mutating the touched files",
        );
        VERIFIER_BLOCKED
    } else if !touched_files.is_empty() && diagnostics_section.is_none() {
        VERIFIER_CAUTION
    } else {
        VERIFIER_READY
    };
    contract.readiness.diagnostics_ready = diagnostics_status.to_owned();
    let diagnostics_summary = if blocking_diagnostic_count > 0 {
        format!(
            "{blocking_diagnostic_count} blocking diagnostic(s) reported across touched files (plus {informational_diagnostic_count} informational hint(s)).",
        )
    } else if informational_diagnostic_count > 0 {
        format!(
            "No blocking diagnostics; {informational_diagnostic_count} informational hint(s) (cfg-gated inactive-code, deprecation notes, etc.) surfaced for context only.",
        )
    } else if !touched_files.is_empty() && diagnostics_section.is_none() {
        "Diagnostics unavailable for touched files; treat edits as provisional.".to_owned()
    } else if touched_files.is_empty() {
        "No touched files were available for diagnostics checks.".to_owned()
    } else {
        format!(
            "No diagnostics reported for {} touched file(s).",
            touched_files.len()
        )
    };
    let _ = diagnostic_count; // currently retained only for the rows summary
    push_verifier_check(
        &mut contract.verifier_checks,
        "diagnostic_verifier",
        diagnostics_status,
        diagnostics_summary,
        diagnostics_section,
    );

    let mut reference_details = json!({
        "tool_name": tool_name,
        "symbol": symbol_hint,
    });
    let mut reference_status = VERIFIER_READY;
    let mut reference_summary = "Reference safety signals look stable.".to_owned();
    if tool_name == "safe_rename_report" || tool_name == "unresolved_reference_check" {
        let symbol_match_count = sections
            .get("symbol_matches")
            .and_then(|value| value.get("count"))
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let reference_count = sections
            .get("references")
            .and_then(|value| value.get("count"))
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let preview_error = sections
            .get("rename_preview")
            .and_then(|value| value.get("preview_error"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        reference_details["symbol_match_count"] = json!(symbol_match_count);
        reference_details["reference_count"] = json!(reference_count);
        if let Some(error) = preview_error.as_deref() {
            reference_details["preview_error"] = json!(error);
        }
        if symbol_match_count == 0 {
            reference_status = VERIFIER_BLOCKED;
            crate::util::push_unique_string(
                &mut contract.blockers,
                "No exact symbol match found; resolve the target before renaming",
            );
            reference_summary =
                "Rename target did not resolve to an exact symbol match.".to_owned();
        } else if symbol_match_count > 1 {
            reference_status = VERIFIER_BLOCKED;
            crate::util::push_unique_string(
                &mut contract.blockers,
                "Ambiguous symbol match; narrow the target before renaming",
            );
            reference_summary =
                format!("{symbol_match_count} exact matches found; rename target is ambiguous.");
        } else if preview_error.is_some() {
            reference_status = VERIFIER_BLOCKED;
            crate::util::push_unique_string(
                &mut contract.blockers,
                "Rename preview failed; inspect the preview error before mutating references",
            );
            reference_summary = "Rename preview raised an error.".to_owned();
        } else if reference_count == 0 {
            reference_status = if tool_name == "safe_rename_report" {
                crate::util::push_unique_string(
                    &mut contract.blockers,
                    "Reference set is empty; verify call sites manually before renaming",
                );
                VERIFIER_BLOCKED
            } else {
                VERIFIER_CAUTION
            };
            reference_summary =
                "Reference coverage is sparse; verify the target manually before refactoring."
                    .to_owned();
        } else {
            reference_summary =
                format!("{reference_count} classified reference(s) available for review.");
        }
    } else if tool_name == "impact_report" {
        let impacted = sections
            .get("impact_rows")
            .and_then(|value| value.get("impacts"))
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let high_impact = impacted.iter().any(|row| {
            let is_additive = row
                .get("change_kind")
                .and_then(|v| v.as_str())
                .is_some_and(|k| k == "additive");
            !is_additive
                && row
                    .get("affected_files")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default()
                    >= 8
        });
        reference_details["impact_rows"] = json!(impacted.len());
        if high_impact {
            reference_status = VERIFIER_CAUTION;
            reference_summary =
                "Large blast radius detected for breaking change; expand importer evidence before broad edits."
                    .to_owned();
        } else if impacted.is_empty() {
            reference_status = VERIFIER_CAUTION;
            reference_summary =
                "No impact rows were produced; verify importers manually before editing."
                    .to_owned();
        } else {
            reference_summary = format!("Impact rows available for {} file(s).", impacted.len());
        }
    } else if tool_name == "refactor_safety_report" {
        let symbol_error = sections
            .get("symbol_impact")
            .and_then(|value| value.get("error"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let boundary_risk = sections
            .get("module_boundary")
            .and_then(|value| value.get("risk_level"))
            .and_then(|value| value.as_str())
            .unwrap_or("low");
        reference_details["boundary_risk"] = json!(boundary_risk);
        if symbol_error.is_some() {
            reference_status = VERIFIER_BLOCKED;
            crate::util::push_unique_string(
                &mut contract.blockers,
                "Symbol impact could not be resolved; fix reference analysis before refactoring",
            );
            reference_summary =
                "Symbol impact lookup failed for the requested refactor target.".to_owned();
        } else if boundary_risk == "high" || boundary_risk == "medium" {
            reference_status = VERIFIER_CAUTION;
            reference_summary =
                "Boundary analysis shows elevated structural risk; verify call paths before mutating."
                    .to_owned();
        }
    } else if matches!(
        tool_name,
        "analyze_change_request" | "verify_change_readiness"
    ) {
        let ranked_files = sections
            .get("ranked_files")
            .and_then(|value| value.get("ranked_files"))
            .and_then(|value| value.as_array())
            .map(|value| value.len())
            .unwrap_or_default();
        reference_details["ranked_file_count"] = json!(ranked_files);
        if ranked_files == 0 {
            reference_status = VERIFIER_CAUTION;
            reference_summary =
                "No ranked file anchors were found; confirm the edit anchor manually.".to_owned();
        } else {
            reference_summary =
                format!("{ranked_files} ranked file anchor(s) available for the change.");
        }
    }
    let references_section = if reference_details.as_object().is_some() {
        sections.insert("verifier_references".to_owned(), reference_details);
        Some("verifier_references")
    } else {
        None
    };
    contract.readiness.reference_safety = reference_status.to_owned();
    push_verifier_check(
        &mut contract.verifier_checks,
        "reference_verifier",
        reference_status,
        reference_summary,
        references_section,
    );

    let mut related_tests = Vec::new();
    for path in touched_files.iter().take(2) {
        if let Ok((payload, _meta)) =
            super::filesystem::find_tests(state, &json!({"path": path, "max_results": 10}))
        {
            related_tests.push(json!({
                "path": path,
                "tests": payload.get("tests").cloned().unwrap_or_else(|| json!([])),
                "count": payload.get("count").cloned().unwrap_or_else(|| json!(0)),
            }));
        }
    }
    if related_tests.is_empty()
        && let Some(existing) = sections.get("related_tests")
    {
        related_tests.push(existing.clone());
    }
    let sensitive = browser_or_ssr_sensitive(&touched_files, summary, top_findings, next_actions);
    let related_test_count = related_tests
        .iter()
        .map(|entry| {
            entry
                .get("count")
                .and_then(|value| value.as_u64())
                .unwrap_or_default() as usize
        })
        .sum::<usize>();
    let tests_section = if !related_tests.is_empty() || sensitive {
        sections.insert(
            "verifier_test_readiness".to_owned(),
            json!({
                "files": touched_files,
                "related_tests": related_tests,
                "browser_or_ssr_sensitive": sensitive,
            }),
        );
        Some("verifier_test_readiness")
    } else {
        None
    };
    let test_status = if sensitive || related_test_count == 0 {
        VERIFIER_CAUTION
    } else {
        VERIFIER_READY
    };
    contract.readiness.test_readiness = test_status.to_owned();
    let test_summary = if sensitive {
        "Browser/SSR-sensitive paths detected; hand off to UI or SSR verification.".to_owned()
    } else if related_test_count == 0 {
        "No nearby test targets were found; keep validation scope explicit.".to_owned()
    } else {
        format!("{related_test_count} related test target(s) found near the change.")
    };
    push_verifier_check(
        &mut contract.verifier_checks,
        "test_readiness_verifier",
        test_status,
        test_summary,
        tests_section,
    );

    let overlapping_claims = sections
        .get("coordination_overlaps")
        .and_then(|value| value.get("claims"))
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if !overlapping_claims.is_empty() {
        push_verifier_check(
            &mut contract.verifier_checks,
            "coordination_overlap_verifier",
            VERIFIER_CAUTION,
            format!(
                "{} overlapping claim(s) detected from other sessions; coordinate worktree or file ownership before mutating.",
                overlapping_claims.len()
            ),
            Some("coordination_overlaps"),
        );
    }

    contract.blockers.truncate(5);
    let mutation_status = if !contract.blockers.is_empty() {
        VERIFIER_BLOCKED
    } else {
        combine_verifier_status(&[
            contract.readiness.diagnostics_ready.as_str(),
            contract.readiness.reference_safety.as_str(),
            contract.readiness.test_readiness.as_str(),
            if overlapping_claims.is_empty() {
                VERIFIER_READY
            } else {
                VERIFIER_CAUTION
            },
        ])
    };
    contract.readiness.mutation_ready = mutation_status.to_owned();
    let mutation_summary = match mutation_status {
        VERIFIER_BLOCKED => {
            "Blockers remain; keep the workflow in preflight until they are resolved."
        }
        VERIFIER_CAUTION if !overlapping_claims.is_empty() => {
            "Overlapping advisory claims detected; coordinate branch, worktree, or file ownership before mutating."
        }
        VERIFIER_CAUTION => "Proceed only with targeted edits and explicit verification steps.",
        _ => "No blocker-level signals found; mutation path is ready for a narrow change.",
    };
    push_verifier_check(
        &mut contract.verifier_checks,
        "mutation_readiness_verifier",
        mutation_status,
        mutation_summary,
        None,
    );
    contract
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn is_high_impact(impacts: &[serde_json::Value]) -> bool {
        impacts.iter().any(|row| {
            let is_additive = row
                .get("change_kind")
                .and_then(|v| v.as_str())
                .is_some_and(|k| k == "additive");
            !is_additive
                && row
                    .get("affected_files")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_default()
                    >= 8
        })
    }

    #[test]
    fn additive_change_not_high_impact() {
        let impacts = vec![json!({
            "path": "types/index.ts",
            "affected_files": 130,
            "change_kind": "additive",
        })];
        assert!(!is_high_impact(&impacts));
    }

    #[test]
    fn breaking_change_is_high_impact() {
        let impacts = vec![json!({
            "path": "types/index.ts",
            "affected_files": 130,
            "change_kind": "mixed",
        })];
        assert!(is_high_impact(&impacts));
    }

    #[test]
    fn small_breaking_change_not_high_impact() {
        let impacts = vec![json!({
            "path": "utils.ts",
            "affected_files": 3,
            "change_kind": "mixed",
        })];
        assert!(!is_high_impact(&impacts));
    }

    #[test]
    fn missing_change_kind_treated_as_breaking() {
        let impacts = vec![json!({
            "path": "lib.rs",
            "affected_files": 10,
        })];
        assert!(is_high_impact(&impacts));
    }

    /// Issue #226 regression: rust-analyzer `inactive-code` hints
    /// (cfg-gated branches under default test/check builds) must not
    /// be classified as blockers, otherwise `review_changes` flips
    /// to `mutation_ready: blocked` on a perfectly clean diff.
    #[test]
    fn inactive_code_hint_is_not_blocking() {
        let diag = json!({
            "severity_label": "hint",
            "code": "inactive-code",
            "source": "rust-analyzer",
            "message": "code is inactive due to #[cfg] directives: feature = \"http\" is disabled",
        });
        assert!(
            !super::is_blocking_diagnostic(&diag),
            "rust-analyzer inactive-code hint must not be a readiness blocker"
        );
    }

    /// `severity_label: \"hint\"` from any source is informational —
    /// even when the code is something other than `inactive-code`.
    #[test]
    fn generic_hint_severity_is_not_blocking() {
        for label in ["hint", "Hint", "HINT", "information", "info"] {
            let diag = json!({
                "severity_label": label,
                "code": "deprecated_signature",
                "source": "tsserver",
                "message": "deprecated overload",
            });
            assert!(
                !super::is_blocking_diagnostic(&diag),
                "severity_label `{label}` must downgrade to informational"
            );
        }
    }

    /// Real warnings and errors must still drive readiness to blocked
    /// — the relaxation for hints must not silence genuine problems.
    #[test]
    fn warning_and_error_severities_remain_blocking() {
        for label in ["warning", "warn", "error", "err"] {
            let diag = json!({
                "severity_label": label,
                "code": "E0061",
                "source": "rustc",
                "message": "type mismatch",
            });
            assert!(
                super::is_blocking_diagnostic(&diag),
                "severity_label `{label}` must remain a blocker"
            );
        }
    }

    /// Defensive default: an LSP that omits `severity_label` is
    /// treated as a blocker — silent acceptance would let unknown
    /// servers slip past the readiness verdict.
    #[test]
    fn missing_severity_label_treated_as_blocking() {
        let diag = json!({
            "code": "U0001",
            "source": "unknown-lsp",
            "message": "unknown problem",
        });
        assert!(super::is_blocking_diagnostic(&diag));
    }
}
