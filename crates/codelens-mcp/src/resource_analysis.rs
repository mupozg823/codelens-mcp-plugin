use crate::AppState;
use crate::state::AnalysisArtifact;
use serde_json::{Value, json};

pub(crate) fn analysis_resource_entries(state: &AppState) -> Vec<Value> {
    state
        .list_analysis_summaries()
        .into_iter()
        .map(|artifact| {
            json!({
                "uri": format!("codelens://analysis/{}/summary", artifact.id),
                "name": format!("Analysis: {}", artifact.tool_name),
                "description": format!("{} ({})", artifact.summary, artifact.surface),
                "mimeType": "application/json"
            })
        })
        .collect()
}

pub(crate) fn analysis_summary_payload(artifact: &AnalysisArtifact) -> Value {
    let verifier_checks = if artifact.verifier_checks.is_empty() {
        vec![
            json!({
                "check": "diagnostic_verifier",
                "status": artifact.readiness.diagnostics_ready,
                "summary": "Refresh diagnostics evidence before trusting a reused artifact.",
                "evidence_section": null,
            }),
            json!({
                "check": "reference_verifier",
                "status": artifact.readiness.reference_safety,
                "summary": "Refresh reference evidence before mutating reused analysis targets.",
                "evidence_section": null,
            }),
            json!({
                "check": "test_readiness_verifier",
                "status": artifact.readiness.test_readiness,
                "summary": "Refresh test-readiness evidence before relying on a reused artifact.",
                "evidence_section": null,
            }),
            json!({
                "check": "mutation_readiness_verifier",
                "status": artifact.readiness.mutation_ready,
                "summary": if artifact.blockers.is_empty() {
                    "Reused artifact needs fresh verifier evidence before mutation."
                } else {
                    "Blockers remain on the reused artifact; refresh evidence before mutation."
                },
                "evidence_section": null,
            }),
        ]
    } else {
        artifact
            .verifier_checks
            .iter()
            .map(|check| {
                json!({
                    "check": check.check,
                    "status": check.status,
                    "summary": check.summary,
                    "evidence_section": check.evidence_section,
                })
            })
            .collect::<Vec<_>>()
    };
    let quality_focus = infer_summary_quality_focus(
        &artifact.tool_name,
        &artifact.summary,
        &artifact.top_findings,
    );
    let recommended_checks = infer_summary_recommended_checks(
        &artifact.tool_name,
        &artifact.summary,
        &artifact.top_findings,
        &artifact.next_actions,
        &artifact.available_sections,
    );
    let performance_watchpoints = infer_summary_performance_watchpoints(
        &artifact.summary,
        &artifact.top_findings,
        &artifact.next_actions,
    );
    let mut payload = json!({
        "analysis_id": artifact.id,
        "tool_name": artifact.tool_name,
        "surface": artifact.surface,
        "summary": artifact.summary,
        "top_findings": artifact.top_findings,
        "risk_level": artifact.risk_level,
        "confidence": artifact.confidence,
        "next_actions": artifact.next_actions,
        "blockers": artifact.blockers,
        "blocker_count": artifact.blockers.len(),
        "readiness": artifact.readiness,
        "verifier_checks": verifier_checks,
        "quality_focus": quality_focus,
        "recommended_checks": recommended_checks,
        "performance_watchpoints": performance_watchpoints,
        "available_sections": artifact.available_sections,
        "created_at_ms": artifact.created_at_ms,
    });
    if artifact.surface == "ci-audit" {
        payload["schema_version"] = json!("codelens-ci-audit-v1");
        payload["report_kind"] = json!(artifact.tool_name);
        payload["profile"] = json!("ci-audit");
        payload["machine_summary"] = json!({
            "finding_count": artifact.top_findings.len(),
            "next_action_count": artifact.next_actions.len(),
            "section_count": artifact.available_sections.len(),
            "blocker_count": artifact.blockers.len(),
            "verifier_check_count": payload["verifier_checks"].as_array().map(|v| v.len()).unwrap_or(0),
            "ready_check_count": payload["verifier_checks"].as_array().map(|checks| checks.iter().filter(|check| check.get("status").and_then(|value| value.as_str()) == Some("ready")).count()).unwrap_or(0),
            "blocked_check_count": payload["verifier_checks"].as_array().map(|checks| checks.iter().filter(|check| check.get("status").and_then(|value| value.as_str()) == Some("blocked")).count()).unwrap_or(0),
            "quality_focus_count": payload["quality_focus"].as_array().map(|v| v.len()).unwrap_or(0),
            "recommended_check_count": payload["recommended_checks"].as_array().map(|v| v.len()).unwrap_or(0),
            "performance_watchpoint_count": payload["performance_watchpoints"].as_array().map(|v| v.len()).unwrap_or(0),
        });
        payload["evidence_handles"] = json!(
            artifact
                .available_sections
                .iter()
                .map(|section| json!({
                    "section": section,
                    "uri": format!("codelens://analysis/{}/{section}", artifact.id),
                }))
                .collect::<Vec<_>>()
        );
    }
    payload
}

fn infer_summary_quality_focus(
    tool_name: &str,
    summary: &str,
    top_findings: &[String],
) -> Vec<String> {
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
        "explore_codebase"
            | "trace_request_path"
            | "review_architecture"
            | "plan_safe_refactor"
            | "audit_security_context"
            | "analyze_change_impact"
            | "cleanup_duplicate_logic"
            | "onboard_project"
            | "analyze_change_request"
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

fn infer_summary_recommended_checks(
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

fn infer_summary_performance_watchpoints(
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
