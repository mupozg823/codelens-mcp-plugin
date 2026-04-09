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

fn push_unique(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
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
    let combined = format!(
        "{} {} {} {}",
        touched_files.join(" "),
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    [
        "browser", "frontend", "layout", "modal", "render", "route", "ssr", "ui",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

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
                diagnostic_rows.push(json!({
                    "file_path": file,
                    "count": count,
                    "diagnostics": payload.get("diagnostics").cloned().unwrap_or_else(|| json!([])),
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
            }),
        );
        Some("verifier_diagnostics")
    } else {
        None
    };
    let diagnostics_status = if diagnostic_count > 0 {
        push_unique(
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
    let diagnostics_summary = if diagnostic_count > 0 {
        format!("{diagnostic_count} diagnostic(s) reported across touched files.")
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
            push_unique(
                &mut contract.blockers,
                "No exact symbol match found; resolve the target before renaming",
            );
            reference_summary =
                "Rename target did not resolve to an exact symbol match.".to_owned();
        } else if symbol_match_count > 1 {
            reference_status = VERIFIER_BLOCKED;
            push_unique(
                &mut contract.blockers,
                "Ambiguous symbol match; narrow the target before renaming",
            );
            reference_summary =
                format!("{symbol_match_count} exact matches found; rename target is ambiguous.");
        } else if preview_error.is_some() {
            reference_status = VERIFIER_BLOCKED;
            push_unique(
                &mut contract.blockers,
                "Rename preview failed; inspect the preview error before mutating references",
            );
            reference_summary = "Rename preview raised an error.".to_owned();
        } else if reference_count == 0 {
            reference_status = if tool_name == "safe_rename_report" {
                push_unique(
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
            push_unique(
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

    contract.blockers.truncate(5);
    let mutation_status = if !contract.blockers.is_empty() {
        VERIFIER_BLOCKED
    } else {
        combine_verifier_status(&[
            contract.readiness.diagnostics_ready.as_str(),
            contract.readiness.reference_safety.as_str(),
            contract.readiness.test_readiness.as_str(),
        ])
    };
    contract.readiness.mutation_ready = mutation_status.to_owned();
    let mutation_summary = match mutation_status {
        VERIFIER_BLOCKED => {
            "Blockers remain; keep the workflow in preflight until they are resolved."
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
}
