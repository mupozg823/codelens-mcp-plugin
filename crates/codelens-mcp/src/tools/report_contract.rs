use super::{success_meta, AppState, ToolResult};
use crate::protocol::BackendKind;
use crate::state::{AnalysisReadiness, AnalysisVerifierCheck};
use crate::tool_defs::{ToolProfile, ToolSurface};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const VERIFIER_READY: &str = "ready";
const VERIFIER_CAUTION: &str = "caution";
const VERIFIER_BLOCKED: &str = "blocked";

#[derive(Default)]
struct VerifierContract {
    blockers: Vec<String>,
    readiness: AnalysisReadiness,
    verifier_checks: Vec<AnalysisVerifierCheck>,
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

fn build_verifier_contract(
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
            row.get("affected_files")
                .and_then(|value| value.as_u64())
                .unwrap_or_default()
                >= 8
        });
        reference_details["impact_rows"] = json!(impacted.len());
        if high_impact {
            reference_status = VERIFIER_CAUTION;
            reference_summary =
                "Large blast radius detected; expand importer evidence before broad edits."
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
    if related_tests.is_empty() {
        if let Some(existing) = sections.get("related_tests") {
            related_tests.push(existing.clone());
        }
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

pub(super) fn make_handle_response(
    state: &AppState,
    tool_name: &str,
    cache_key: Option<String>,
    summary: String,
    top_findings: Vec<String>,
    confidence: f64,
    next_actions: Vec<String>,
    mut sections: BTreeMap<String, Value>,
    touched_files: Vec<String>,
    symbol_hint: Option<String>,
) -> ToolResult {
    let risk_level = infer_risk_level(&summary, &top_findings, &next_actions);
    let ci_audit = matches!(*state.surface(), ToolSurface::Profile(ToolProfile::CiAudit));
    let verifier = build_verifier_contract(
        state,
        tool_name,
        &summary,
        &top_findings,
        &next_actions,
        &mut sections,
        &touched_files,
        symbol_hint.as_deref(),
    );
    if let Some(cache_key) = cache_key.as_deref() {
        if let Some(artifact) = state.find_reusable_analysis(tool_name, cache_key) {
            state.metrics().record_analysis_cache_hit();
            let data = build_handle_payload(
                tool_name,
                &artifact.id,
                &artifact.summary,
                &artifact.top_findings,
                &artifact.risk_level,
                artifact.confidence,
                &artifact.next_actions,
                &artifact.blockers,
                &artifact.readiness,
                &artifact.verifier_checks,
                &artifact.available_sections,
                true,
                ci_audit,
            );
            state.metrics().record_quality_contract_emitted(
                data["quality_focus"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
                data["recommended_checks"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
                data["performance_watchpoints"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
            );
            state.metrics().record_verifier_contract_emitted(
                data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
                data["verifier_checks"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
            );
            return Ok((data, success_meta(BackendKind::Hybrid, artifact.confidence)));
        }
    }
    let artifact = state.store_analysis(
        tool_name,
        cache_key,
        summary.clone(),
        top_findings.clone(),
        risk_level.to_owned(),
        confidence,
        next_actions.clone(),
        verifier.blockers.clone(),
        verifier.readiness.clone(),
        verifier.verifier_checks.clone(),
        sections,
    )?;
    let data = build_handle_payload(
        tool_name,
        &artifact.id,
        &artifact.summary,
        &artifact.top_findings,
        &artifact.risk_level,
        artifact.confidence,
        &artifact.next_actions,
        &artifact.blockers,
        &artifact.readiness,
        &artifact.verifier_checks,
        &artifact.available_sections,
        false,
        ci_audit,
    );
    state.metrics().record_quality_contract_emitted(
        data["quality_focus"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        data["recommended_checks"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        data["performance_watchpoints"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
    );
    state.metrics().record_verifier_contract_emitted(
        data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
        data["verifier_checks"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
    );
    Ok((data, success_meta(BackendKind::Hybrid, confidence)))
}

fn build_handle_payload(
    tool_name: &str,
    analysis_id: &str,
    summary: &str,
    top_findings: &[String],
    risk_level: &str,
    confidence: f64,
    next_actions: &[String],
    blockers: &[String],
    readiness: &AnalysisReadiness,
    verifier_checks: &[AnalysisVerifierCheck],
    available_sections: &[String],
    reused: bool,
    ci_audit: bool,
) -> Value {
    let normalized_verifier_checks = if verifier_checks.is_empty() {
        vec![
            AnalysisVerifierCheck {
                check: "diagnostic_verifier".to_owned(),
                status: readiness.diagnostics_ready.clone(),
                summary: "Refresh diagnostics evidence before trusting a reused artifact."
                    .to_owned(),
                evidence_section: None,
            },
            AnalysisVerifierCheck {
                check: "reference_verifier".to_owned(),
                status: readiness.reference_safety.clone(),
                summary: "Refresh reference evidence before mutating reused analysis targets."
                    .to_owned(),
                evidence_section: None,
            },
            AnalysisVerifierCheck {
                check: "test_readiness_verifier".to_owned(),
                status: readiness.test_readiness.clone(),
                summary: "Refresh test-readiness evidence before relying on a reused artifact."
                    .to_owned(),
                evidence_section: None,
            },
            AnalysisVerifierCheck {
                check: "mutation_readiness_verifier".to_owned(),
                status: readiness.mutation_ready.clone(),
                summary: if blockers.is_empty() {
                    "Reused artifact needs fresh verifier evidence before mutation.".to_owned()
                } else {
                    "Blockers remain on the reused artifact; refresh evidence before mutation."
                        .to_owned()
                },
                evidence_section: None,
            },
        ]
    } else {
        verifier_checks.to_vec()
    };
    let quality_focus = infer_quality_focus(tool_name, summary, top_findings);
    let recommended_checks = infer_recommended_checks(
        tool_name,
        summary,
        top_findings,
        next_actions,
        available_sections,
    );
    let performance_watchpoints =
        infer_performance_watchpoints(summary, top_findings, next_actions);
    let mut payload = json!({
        "analysis_id": analysis_id,
        "summary": summary,
        "top_findings": top_findings,
        "risk_level": risk_level,
        "confidence": confidence,
        "next_actions": next_actions,
        "blockers": blockers,
        "blocker_count": blockers.len(),
        "readiness": readiness,
        "verifier_checks": normalized_verifier_checks,
        "quality_focus": quality_focus,
        "recommended_checks": recommended_checks,
        "performance_watchpoints": performance_watchpoints,
        "available_sections": available_sections,
        "reused": reused,
    });
    fn status_to_score(s: &str) -> f64 {
        match s {
            "ready" => 1.0,
            "caution" => 0.5,
            _ => 0.0,
        }
    }
    let readiness_score = (status_to_score(&readiness.diagnostics_ready)
        + status_to_score(&readiness.reference_safety)
        + status_to_score(&readiness.test_readiness)
        + status_to_score(&readiness.mutation_ready))
        / 4.0;
    payload["readiness_score"] = json!(readiness_score);
    if ci_audit {
        payload["schema_version"] = json!("codelens-ci-audit-v1");
        payload["report_kind"] = json!(tool_name);
        payload["profile"] = json!("ci-audit");
        payload["machine_summary"] = json!({
            "finding_count": top_findings.len(),
            "next_action_count": next_actions.len(),
            "section_count": available_sections.len(),
            "blocker_count": blockers.len(),
            "verifier_check_count": payload["verifier_checks"].as_array().map(|v| v.len()).unwrap_or(0),
            "ready_check_count": payload["verifier_checks"].as_array().map(|checks| checks.iter().filter(|check| check.get("status") == Some(&json!(VERIFIER_READY))).count()).unwrap_or(0),
            "blocked_check_count": payload["verifier_checks"].as_array().map(|checks| checks.iter().filter(|check| check.get("status") == Some(&json!(VERIFIER_BLOCKED))).count()).unwrap_or(0),
            "quality_focus_count": payload["quality_focus"].as_array().map(|v| v.len()).unwrap_or(0),
            "recommended_check_count": payload["recommended_checks"].as_array().map(|v| v.len()).unwrap_or(0),
            "performance_watchpoint_count": payload["performance_watchpoints"].as_array().map(|v| v.len()).unwrap_or(0),
        });
        payload["evidence_handles"] = json!(available_sections
            .iter()
            .map(|section| json!({
                "section": section,
                "uri": format!("codelens://analysis/{analysis_id}/{section}"),
            }))
            .collect::<Vec<_>>());
    }
    payload
}

fn infer_quality_focus(tool_name: &str, summary: &str, top_findings: &[String]) -> Vec<String> {
    let combined = format!("{} {}", summary, top_findings.join(" ")).to_ascii_lowercase();
    let mut focus = Vec::new();
    let mut push_unique = |value: &str| {
        if !focus.iter().any(|existing| existing == value) {
            focus.push(value.to_owned());
        }
    };

    push_unique("correctness");
    if matches!(
        tool_name,
        "analyze_change_request"
            | "verify_change_readiness"
            | "impact_report"
            | "refactor_safety_report"
            | "safe_rename_report"
            | "unresolved_reference_check"
    ) {
        push_unique("regression_safety");
    }
    if combined.contains("http")
        || combined.contains("browser")
        || combined.contains("ui")
        || combined.contains("render")
        || combined.contains("frontend")
        || combined.contains("layout")
    {
        push_unique("user_experience");
    }
    if combined.contains("coupling")
        || combined.contains("circular")
        || combined.contains("refactor")
        || combined.contains("boundary")
    {
        push_unique("maintainability");
    }
    if combined.contains("search")
        || combined.contains("embedding")
        || combined.contains("watch")
        || combined.contains("latency")
        || combined.contains("performance")
    {
        push_unique("performance");
    }
    focus
}

fn infer_recommended_checks(
    tool_name: &str,
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
    available_sections: &[String],
) -> Vec<String> {
    let combined = format!(
        "{} {} {} {}",
        tool_name,
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    let mut checks = Vec::new();
    let mut push_unique = |value: &str| {
        if !checks.iter().any(|existing| existing == value) {
            checks.push(value.to_owned());
        }
    };

    push_unique("run targeted tests for affected files or symbols");
    push_unique("run diagnostics or lint on touched files before finalizing");

    if available_sections
        .iter()
        .any(|section| section == "related_tests")
    {
        push_unique("expand related_tests and execute the highest-signal subset");
    }
    if combined.contains("rename") || combined.contains("refactor") {
        push_unique("verify references and call sites after the refactor preview");
    }
    if combined.contains("http")
        || combined.contains("browser")
        || combined.contains("ui")
        || combined.contains("frontend")
        || combined.contains("layout")
        || combined.contains("render")
    {
        push_unique("exercise the user-facing flow in a browser or UI harness");
    }
    if combined.contains("search")
        || combined.contains("embedding")
        || combined.contains("latency")
        || combined.contains("performance")
    {
        push_unique("compare hot-path latency or throughput before and after the change");
    }
    if combined.contains("dead code") || combined.contains("delete") {
        push_unique("confirm the candidate is unused in tests, runtime paths, and CI scripts");
    }
    checks
}

fn infer_performance_watchpoints(
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
) -> Vec<String> {
    let combined = format!(
        "{} {} {}",
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    let mut watchpoints = Vec::new();
    let mut push_unique = |value: &str| {
        if !watchpoints.iter().any(|existing| existing == value) {
            watchpoints.push(value.to_owned());
        }
    };

    if combined.contains("search") || combined.contains("embedding") || combined.contains("query") {
        push_unique("watch ranking quality, latency, and cache-hit behavior on search paths");
    }
    if combined.contains("http") || combined.contains("server") || combined.contains("route") {
        push_unique("watch request latency, concurrency, and error-rate changes on hot routes");
    }
    if combined.contains("watch") || combined.contains("filesystem") {
        push_unique("watch background work, queue depth, and repeated invalidation behavior");
    }
    if combined.contains("ui")
        || combined.contains("frontend")
        || combined.contains("layout")
        || combined.contains("render")
        || combined.contains("browser")
    {
        push_unique("watch rendering smoothness, layout stability, and unnecessary re-renders");
    }
    watchpoints
}

fn infer_risk_level(
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
) -> &'static str {
    let combined = format!(
        "{} {} {}",
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    if [
        "blocker",
        "circular",
        "cycle",
        "destructive",
        "breaking",
        "high risk",
        "error",
        "failing",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
    {
        "high"
    } else if top_findings.len() >= 3
        || ["risk", "impact", "coupling", "dead code", "stale"]
            .iter()
            .any(|needle| combined.contains(needle))
    {
        "medium"
    } else {
        "low"
    }
}
