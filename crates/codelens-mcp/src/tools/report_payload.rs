use crate::state::{AnalysisReadiness, AnalysisVerifierCheck};
use serde_json::{Value, json};

use super::report_verifier::{VERIFIER_BLOCKED, VERIFIER_READY};

pub(crate) fn build_handle_payload(
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
        payload["evidence_handles"] = json!(
            available_sections
                .iter()
                .map(|section| json!({
                    "section": section,
                    "uri": format!("codelens://analysis/{analysis_id}/{section}"),
                }))
                .collect::<Vec<_>>()
        );
    }
    payload
}

pub(crate) fn infer_risk_level(
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
