use crate::analysis_handles::{analysis_section_handles, analysis_summary_resource};
use crate::state::{AnalysisReadiness, AnalysisVerifierCheck};
use serde_json::{Value, json};

use super::report_verifier::{VERIFIER_BLOCKED, VERIFIER_READY};

#[allow(clippy::too_many_arguments)]
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
                pass_condition: super::report_verifier::default_pass_condition(
                    "diagnostic_verifier",
                ),
            },
            AnalysisVerifierCheck {
                check: "reference_verifier".to_owned(),
                status: readiness.reference_safety.clone(),
                summary: "Refresh reference evidence before mutating reused analysis targets."
                    .to_owned(),
                evidence_section: None,
                pass_condition: super::report_verifier::default_pass_condition(
                    "reference_verifier",
                ),
            },
            AnalysisVerifierCheck {
                check: "test_readiness_verifier".to_owned(),
                status: readiness.test_readiness.clone(),
                summary: "Refresh test-readiness evidence before relying on a reused artifact."
                    .to_owned(),
                evidence_section: None,
                pass_condition: super::report_verifier::default_pass_condition(
                    "test_readiness_verifier",
                ),
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
                pass_condition: super::report_verifier::default_pass_condition(
                    "mutation_readiness_verifier",
                ),
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
    let summary_resource = analysis_summary_resource(analysis_id);
    let section_handles = analysis_section_handles(analysis_id, available_sections);
    // Harness-facing action chain: every section handle is promoted to
    // a `suggested_next_tools` entry so the orchestrator has a single
    // uniform place to look for "what do I call next" across every
    // analysis report, rather than picking section_handles apart by
    // hand. The tool+args shape matches how other CodeLens responses
    // already emit `suggested_next_tools`, so the harness can drive
    // the chain without a report-specific adapter.
    let suggested_next_tools: Vec<Value> = available_sections
        .iter()
        .map(|section| {
            json!({
                "tool": "ReadMcpResourceTool",
                "arguments": {
                    "uri": format!("codelens://analysis/{analysis_id}/{section}")
                },
                "rationale": format!("Expand `{section}` section of analysis {analysis_id}"),
            })
        })
        .collect();
    // Phase O8a — `session_continuation_hint` fires when the verifier
    // surfaces 3+ blockers so the caller knows to persist-and-resume
    // instead of driving the work further inline. Threshold matches the
    // doom-loop burst threshold so both signals align.
    let session_continuation_hint = blockers.len() >= 3;
    let mut payload = json!({
        "analysis_id": analysis_id,
        "summary": summary,
        "top_findings": top_findings,
        "risk_level": risk_level,
        "confidence": confidence,
        "next_actions": next_actions,
        "blockers": blockers,
        "blocker_count": blockers.len(),
        "session_continuation_hint": session_continuation_hint,
        "readiness": readiness,
        "verifier_checks": normalized_verifier_checks,
        "quality_focus": quality_focus,
        "recommended_checks": recommended_checks,
        "performance_watchpoints": performance_watchpoints,
        "available_sections": available_sections,
        "summary_resource": summary_resource,
        "section_handles": section_handles,
        "suggested_next_tools": suggested_next_tools,
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
        payload["evidence_handles"] = payload["section_handles"].clone();
    }
    trim_preview_first_handle_payload(tool_name, ci_audit, &mut payload);
    payload
}

/// Threshold for the size-gated preview-first trim on high-payload handle
/// reports. Below this, the verbose arrays are cheap enough to leave inline
/// (avoids forcing an extra `get_analysis_section` round-trip for small
/// responses).
const PREVIEW_FIRST_TRIM_MIN_CHARS: usize = 4000; // ≈ 1000 tokens

fn trim_preview_first_handle_payload(tool_name: &str, ci_audit: bool, payload: &mut Value) {
    if ci_audit {
        return;
    }

    let always_trim = matches!(tool_name, "refactor_safety_report");
    let size_gated = matches!(
        tool_name,
        "impact_report"
            | "module_boundary_report"
            | "semantic_code_review"
            | "analyze_change_request"
    );
    if !always_trim && !size_gated {
        return;
    }

    if size_gated && !always_trim {
        let approx_chars = payload.to_string().len();
        if approx_chars < PREVIEW_FIRST_TRIM_MIN_CHARS {
            return;
        }
    }

    let Some(obj) = payload.as_object_mut() else {
        return;
    };

    // Verbose reasoning arrays — already mirrored inside the stored artifact
    // and reachable through `section_handles`. Drop them from the inline
    // payload so the response stays preview-first.
    obj.remove("verifier_checks");
    obj.remove("quality_focus");
    obj.remove("recommended_checks");
    obj.remove("performance_watchpoints");

    // `refactor_safety_report` historically also drops `top_findings` (its
    // signal lives entirely in readiness + section handles). The new
    // size-gated origins keep `top_findings` because the 1-3 line preview
    // is the cheapest first-call signal for callers.
    if always_trim {
        obj.remove("top_findings");
    }
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

#[cfg(test)]
mod preview_first_trim_tests {
    use super::*;
    use serde_json::json;

    fn make_payload(extra_filler_chars: usize) -> Value {
        json!({
            "analysis_id": "analysis-test",
            "summary": "x".repeat(extra_filler_chars),
            "top_findings": ["finding-A", "finding-B"],
            "verifier_checks": [{"check": "diagnostic_verifier", "status": "ready"}],
            "quality_focus": ["correctness"],
            "recommended_checks": ["run targeted tests"],
            "performance_watchpoints": ["watch latency"],
            "readiness": {"mutation_ready": "ready"},
        })
    }

    #[test]
    fn refactor_safety_report_always_trims_top_findings() {
        let mut payload = make_payload(0);
        trim_preview_first_handle_payload("refactor_safety_report", false, &mut payload);
        let obj = payload.as_object().unwrap();
        assert!(!obj.contains_key("top_findings"));
        assert!(!obj.contains_key("verifier_checks"));
        assert!(!obj.contains_key("recommended_checks"));
        assert!(obj.contains_key("readiness"));
    }

    #[test]
    fn impact_report_below_threshold_keeps_verbose_arrays() {
        let mut payload = make_payload(0);
        trim_preview_first_handle_payload("impact_report", false, &mut payload);
        let obj = payload.as_object().unwrap();
        assert!(obj.contains_key("verifier_checks"));
        assert!(obj.contains_key("recommended_checks"));
        assert!(obj.contains_key("top_findings"));
    }

    #[test]
    fn impact_report_above_threshold_trims_verbose_but_keeps_top_findings() {
        let mut payload = make_payload(PREVIEW_FIRST_TRIM_MIN_CHARS + 100);
        trim_preview_first_handle_payload("impact_report", false, &mut payload);
        let obj = payload.as_object().unwrap();
        assert!(!obj.contains_key("verifier_checks"));
        assert!(!obj.contains_key("quality_focus"));
        assert!(!obj.contains_key("recommended_checks"));
        assert!(!obj.contains_key("performance_watchpoints"));
        assert!(
            obj.contains_key("top_findings"),
            "size-gated trim must keep top_findings"
        );
        assert!(obj.contains_key("readiness"));
    }

    #[test]
    fn unknown_tool_is_never_trimmed() {
        let mut payload = make_payload(PREVIEW_FIRST_TRIM_MIN_CHARS + 100);
        trim_preview_first_handle_payload("explore_codebase", false, &mut payload);
        let obj = payload.as_object().unwrap();
        assert!(obj.contains_key("verifier_checks"));
        assert!(obj.contains_key("top_findings"));
    }

    // ── Phase O8a — session continuation hint ─────────────────────
    //
    // `docs/plans/PLAN_opus47-alignment.md` Tier C flips a
    // `session_continuation_hint` boolean on the workflow handle
    // payload when the verifier surfaces "many" blockers so the
    // caller knows to persist-and-resume instead of trying to drive
    // the work further inline. The threshold is 3+ blockers, chosen
    // to match the `doom_loop_counter` burst threshold so both
    // signals fire in the same regime.

    #[test]
    fn session_continuation_hint_flips_on_many_blockers() {
        let readiness = AnalysisReadiness::default();

        // No blockers → hint false.
        let calm = build_handle_payload(
            "impact_report",
            "analysis-calm",
            "summary",
            &[],
            "low",
            0.9,
            &[],
            &[],
            &readiness,
            &[],
            &[],
            false,
            false,
        );
        assert_eq!(
            calm["session_continuation_hint"],
            json!(false),
            "no blockers must not request a session reset"
        );
        assert_eq!(calm["blocker_count"], json!(0));

        // 3 blockers → hint true (threshold).
        let many = build_handle_payload(
            "impact_report",
            "analysis-many",
            "summary",
            &[],
            "high",
            0.8,
            &[],
            &["a".to_owned(), "b".to_owned(), "c".to_owned()],
            &readiness,
            &[],
            &[],
            false,
            false,
        );
        assert_eq!(many["blocker_count"], json!(3));
        assert_eq!(
            many["session_continuation_hint"],
            json!(true),
            "3+ blockers must flip the session-continuation hint"
        );
    }

    #[test]
    fn ci_audit_disables_trim() {
        let mut payload = make_payload(PREVIEW_FIRST_TRIM_MIN_CHARS + 100);
        trim_preview_first_handle_payload("refactor_safety_report", true, &mut payload);
        let obj = payload.as_object().unwrap();
        assert!(obj.contains_key("verifier_checks"));
        assert!(obj.contains_key("top_findings"));
    }

    #[test]
    fn module_boundary_and_semantic_review_and_change_request_are_size_gated() {
        for tool in [
            "module_boundary_report",
            "semantic_code_review",
            "analyze_change_request",
        ] {
            let mut small = make_payload(0);
            trim_preview_first_handle_payload(tool, false, &mut small);
            assert!(
                small.as_object().unwrap().contains_key("verifier_checks"),
                "{tool} below threshold must keep verifier_checks"
            );

            let mut large = make_payload(PREVIEW_FIRST_TRIM_MIN_CHARS + 100);
            trim_preview_first_handle_payload(tool, false, &mut large);
            assert!(
                !large.as_object().unwrap().contains_key("verifier_checks"),
                "{tool} above threshold must drop verifier_checks"
            );
            assert!(
                large.as_object().unwrap().contains_key("top_findings"),
                "{tool} above threshold must keep top_findings"
            );
        }
    }
}
