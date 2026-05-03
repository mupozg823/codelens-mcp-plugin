//! Offline evaluation lanes — session-audit aggregation.
//!
//! One lane shipped today: `eval_session_audit`. The three other
//! lanes originally proposed (tool_selection, argument_correctness,
//! retrieval_quality) were rejected after objective evaluation:
//! retrieval_quality is redundant with `embedding-quality.py --check`,
//! argument_correctness is already surfaced by the per-session audit
//! checks themselves, and tool_selection has no ground-truth dataset
//! yet so synthetic scoring would be self-grading. See ADR-0005 §5
//! "Offline eval lanes" and the session notes dated 2026-04-18.

use crate::AppState;
use crate::protocol::BackendKind;
use crate::runtime_types::{AnalysisReadiness, AnalysisVerifierCheck};
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{ToolProfile, ToolSurface};
use crate::tool_runtime::{ToolResult, success_meta};
use crate::tools::report_payload::{build_handle_payload, infer_risk_level};
use crate::tools::report_verifier::{VERIFIER_CAUTION, VERIFIER_READY};
use crate::tools::session::{audit_builder_session, audit_planner_session};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
struct AuditStats {
    pass: u32,
    warn: u32,
    fail: u32,
}

impl AuditStats {
    fn record(&mut self, status: &str) {
        match status {
            "pass" => self.pass += 1,
            "warn" => self.warn += 1,
            "fail" => self.fail += 1,
            _ => {}
        }
    }

    fn applicable(&self) -> u32 {
        self.pass + self.warn + self.fail
    }
}

fn pass_rate(stats: &AuditStats) -> Value {
    let denom = stats.applicable();
    if denom == 0 {
        Value::Null
    } else {
        json!(stats.pass as f64 / denom as f64)
    }
}

fn pass_rate_label(stats: &AuditStats) -> String {
    let denom = stats.applicable();
    if denom == 0 {
        "n/a".to_owned()
    } else {
        format!("{:.3}", stats.pass as f64 / denom as f64)
    }
}

fn collect_failed(audit: &Value, failed_checks: &mut HashMap<String, usize>) {
    if let Some(arr) = audit["findings"].as_array() {
        for finding in arr {
            if let Some(code) = finding["code"].as_str() {
                *failed_checks.entry(code.to_owned()).or_default() += 1;
            }
        }
    }
}

fn status_counts_json(stats: &AuditStats) -> Value {
    json!({
        "pass": stats.pass,
        "warn": stats.warn,
        "fail": stats.fail,
    })
}

fn finding_codes(audit: &Value) -> Vec<String> {
    audit["findings"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|finding| finding["code"].as_str())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn audit_row(role: &str, audit: &Value) -> Value {
    json!({
        "session_id": audit["session_summary"]["session_id"],
        "role": role,
        "status": audit["status"],
        "score": audit["score"],
        "surface": audit["session_summary"]["current_surface"],
        "transport": audit["session_summary"]["transport"],
        "finding_codes": finding_codes(audit),
        "recommended_next_tools": audit["recommended_next_tools"],
        "recent_tools": audit["session_summary"]["recent_tools"],
    })
}

fn recommended_action_for_check(code: &str) -> Option<&'static str> {
    match code {
        "bootstrap_order" => Some("prepare_harness_session"),
        "mutation_gate" => Some("verify_change_readiness"),
        "structure_evidence" | "read_side_evidence" => Some("get_symbols_overview"),
        "diagnostics_before_mutation" | "diagnostics_after_mutation" => {
            Some("get_file_diagnostics")
        }
        "coordination_registration" => Some("register_agent_work"),
        "coordination_claim" => Some("claim_files"),
        "coordination_release" => Some("release_files"),
        "change_evidence" => Some("get_changed_files"),
        "workflow_first" => Some("review_changes"),
        _ => None,
    }
}

fn is_builder_surface(surface: &str) -> bool {
    matches!(surface, "builder-minimal" | "refactor-full")
}

fn is_planner_surface(surface: &str) -> bool {
    matches!(surface, "planner-readonly" | "reviewer-graph")
}

fn choose_role_audit(
    state: &AppState,
    session_id: &str,
) -> Result<Option<(&'static str, Value)>, crate::error::CodeLensError> {
    let arguments = json!({"session_id": session_id, "detail": "compact"});
    let builder = audit_builder_session(state, &arguments)?.0;
    let planner = audit_planner_session(state, &arguments)?.0;
    let current_surface = builder["session_summary"]["current_surface"]
        .as_str()
        .or_else(|| planner["session_summary"]["current_surface"].as_str())
        .unwrap_or("");

    if is_builder_surface(current_surface) && builder["status"].as_str() != Some("not_applicable") {
        return Ok(Some(("builder", builder)));
    }
    if is_planner_surface(current_surface) && planner["status"].as_str() != Some("not_applicable") {
        return Ok(Some(("planner", planner)));
    }
    if builder["status"].as_str() != Some("not_applicable") {
        return Ok(Some(("builder", builder)));
    }
    if planner["status"].as_str() != Some("not_applicable") {
        return Ok(Some(("planner", planner)));
    }

    Ok(None)
}

fn coverage_check(
    check: &str,
    status: &'static str,
    summary: impl Into<String>,
) -> AnalysisVerifierCheck {
    AnalysisVerifierCheck {
        check: check.to_owned(),
        status: status.to_owned(),
        summary: summary.into(),
        evidence_section: Some("audit_pass_rate".to_owned()),
    }
}

pub fn eval_session_audit(state: &AppState, arguments: &Value) -> ToolResult {
    let mut session_ids = state.metrics().tracked_session_ids();
    session_ids.sort();
    session_ids.dedup();
    let tracked_session_count = session_ids.len();

    let mut builder = AuditStats::default();
    let mut planner = AuditStats::default();
    let mut failed_checks: HashMap<String, usize> = HashMap::new();
    let mut session_rows = Vec::new();
    let mut skipped_session_count = 0usize;

    for session_id in &session_ids {
        let Some((role, audit)) = choose_role_audit(state, session_id)? else {
            skipped_session_count += 1;
            continue;
        };

        let status = audit["status"].as_str().unwrap_or("");
        match role {
            "builder" => builder.record(status),
            "planner" => planner.record(status),
            _ => {}
        }
        if matches!(status, "warn" | "fail") {
            collect_failed(&audit, &mut failed_checks);
        }
        session_rows.push(audit_row(role, &audit));
    }

    let mut top_failed: Vec<(String, usize)> = failed_checks.into_iter().collect();
    top_failed.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_failed_json: Vec<Value> = top_failed
        .iter()
        .take(8)
        .map(|(code, count)| json!({ "code": code, "count": count }))
        .collect();

    let applicable_session_count = session_rows.len();
    let mut sections: BTreeMap<String, Value> = BTreeMap::new();
    sections.insert(
        "audit_pass_rate".to_owned(),
        json!({
            "tracked_session_count": tracked_session_count,
            "session_count": applicable_session_count,
            "skipped_session_count": skipped_session_count,
            "builder_session_count": builder.applicable(),
            "builder_pass_rate": pass_rate(&builder),
            "builder_status_counts": status_counts_json(&builder),
            "planner_session_count": planner.applicable(),
            "planner_pass_rate": pass_rate(&planner),
            "planner_status_counts": status_counts_json(&planner),
            "top_failed_checks": top_failed_json,
        }),
    );
    sections.insert(
        "session_rows".to_owned(),
        json!({
            "count": session_rows.len(),
            "sessions": session_rows,
        }),
    );

    let summary = format!(
        "Aggregated session audit signal over {} applicable session(s) from {} tracked runtime session(s).",
        applicable_session_count, tracked_session_count,
    );
    let mut top_findings = vec![
        format!(
            "Builder pass rate: {} across {} applicable session(s).",
            pass_rate_label(&builder),
            builder.applicable(),
        ),
        format!(
            "Planner pass rate: {} across {} applicable session(s).",
            pass_rate_label(&planner),
            planner.applicable(),
        ),
    ];
    if let Some((code, count)) = top_failed.first() {
        top_findings.push(format!(
            "Most frequent non-pass check: `{}` in {} session(s).",
            code, count
        ));
    }

    let mut next_actions = if applicable_session_count == 0 {
        vec![
            "Run audit_builder_session or audit_planner_session on at least one live session before promoting this aggregate lane to CI.".to_owned(),
        ]
    } else {
        vec!["Inspect `session_rows` and drill into flagged sessions with the matching per-session audit tool.".to_owned()]
    };
    for code in top_failed
        .iter()
        .take(3)
        .map(|(code, _count)| code.as_str())
    {
        if let Some(tool) = recommended_action_for_check(code) {
            let action =
                format!("Re-run `{tool}` on the failing sessions before tightening the gate.");
            if !next_actions.iter().any(|existing| existing == &action) {
                next_actions.push(action);
            }
        }
    }

    let readiness = if applicable_session_count == 0 {
        AnalysisReadiness {
            diagnostics_ready: VERIFIER_CAUTION.to_owned(),
            reference_safety: VERIFIER_CAUTION.to_owned(),
            test_readiness: VERIFIER_CAUTION.to_owned(),
            mutation_ready: VERIFIER_CAUTION.to_owned(),
        }
    } else {
        AnalysisReadiness {
            diagnostics_ready: VERIFIER_READY.to_owned(),
            reference_safety: VERIFIER_READY.to_owned(),
            test_readiness: VERIFIER_READY.to_owned(),
            mutation_ready: VERIFIER_READY.to_owned(),
        }
    };
    let verifier_checks = vec![
        coverage_check(
            "session_audit_coverage",
            if applicable_session_count == 0 {
                VERIFIER_CAUTION
            } else {
                VERIFIER_READY
            },
            if applicable_session_count == 0 {
                "No applicable builder/planner session audits were available in the current runtime."
            } else {
                "Aggregate includes at least one applicable builder/planner session audit."
            },
        ),
        coverage_check(
            "builder_lane_coverage",
            if builder.applicable() == 0 {
                VERIFIER_CAUTION
            } else {
                VERIFIER_READY
            },
            format!(
                "Builder coverage: {} applicable session(s).",
                builder.applicable()
            ),
        ),
        coverage_check(
            "planner_lane_coverage",
            if planner.applicable() == 0 {
                VERIFIER_CAUTION
            } else {
                VERIFIER_READY
            },
            format!(
                "Planner coverage: {} applicable session(s).",
                planner.applicable()
            ),
        ),
    ];

    let logical_session_id = SessionRequestContext::from_json(arguments).session_id;
    let logical_session_id = Some(logical_session_id.as_str());
    let confidence = 0.96;
    let risk_level = infer_risk_level(&summary, &top_findings, &next_actions).to_owned();
    let artifact = state.store_analysis_for_current_scope(
        "eval_session_audit",
        None,
        summary.clone(),
        top_findings.clone(),
        risk_level,
        confidence,
        next_actions.clone(),
        Vec::new(),
        readiness.clone(),
        verifier_checks.clone(),
        sections,
    )?;
    let ci_audit = matches!(*state.surface(), ToolSurface::Profile(ToolProfile::CiAudit));
    let mut payload = build_handle_payload(
        "eval_session_audit",
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
        &[],
        false,
        ci_audit,
    );
    state.metrics().record_quality_contract_emitted_for_session(
        payload["quality_focus"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0),
        payload["recommended_checks"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0),
        payload["performance_watchpoints"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0),
        logical_session_id,
    );
    state
        .metrics()
        .record_verifier_contract_emitted_for_session(
            payload["blockers"]
                .as_array()
                .map(|items| items.len())
                .unwrap_or(0),
            payload["verifier_checks"]
                .as_array()
                .map(|items| items.len())
                .unwrap_or(0),
            logical_session_id,
        );
    let prior_ids = state.recent_analysis_ids();
    if prior_ids.len() > 1 {
        let prior = prior_ids
            .iter()
            .filter(|id| id.as_str() != artifact.id)
            .cloned()
            .collect::<Vec<_>>();
        if !prior.is_empty() {
            payload["prior_analyses"] = json!(prior);
        }
    }

    Ok((payload, success_meta(BackendKind::Hybrid, confidence)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_stats_records_and_counts() {
        let mut s = AuditStats::default();
        s.record("pass");
        s.record("warn");
        s.record("fail");
        s.record("unknown");
        assert_eq!(s.pass, 1);
        assert_eq!(s.warn, 1);
        assert_eq!(s.fail, 1);
        assert_eq!(s.applicable(), 3);
    }

    #[test]
    fn pass_rate_zero_denom_returns_null() {
        let s = AuditStats::default();
        assert!(pass_rate(&s).is_null());
        assert_eq!(pass_rate_label(&s), "n/a");
    }

    #[test]
    fn pass_rate_computes_correctly() {
        let mut s = AuditStats::default();
        s.record("pass");
        s.record("pass");
        s.record("warn");
        assert_eq!(pass_rate(&s), json!(2.0 / 3.0));
        assert_eq!(pass_rate_label(&s), "0.667");
    }

    #[test]
    fn status_counts_json_matches() {
        let mut s = AuditStats::default();
        s.record("pass");
        s.record("fail");
        assert_eq!(
            status_counts_json(&s),
            json!({"pass": 1, "warn": 0, "fail": 1})
        );
    }

    #[test]
    fn finding_codes_extracts_and_skips_missing() {
        let audit = json!({"findings": [{"code": "A"}, {"other": true}, {"code": "B"}]});
        assert_eq!(finding_codes(&audit), vec!["A", "B"]);
    }

    #[test]
    fn finding_codes_empty_when_no_findings() {
        let audit = json!({});
        assert!(finding_codes(&audit).is_empty());
    }

    #[test]
    fn collect_failed_aggregates_codes() {
        let audit = json!({"findings": [{"code": "X"}, {"code": "Y"}, {"code": "X"}]});
        let mut map = HashMap::new();
        collect_failed(&audit, &mut map);
        assert_eq!(map["X"], 2);
        assert_eq!(map["Y"], 1);
    }

    #[test]
    fn audit_row_shape() {
        let audit = json!({
            "session_summary": {"session_id": "s1", "current_surface": "builder-minimal", "transport": "http", "recent_tools": ["t1"]},
            "status": "pass",
            "score": 95,
            "recommended_next_tools": ["t2"],
        });
        let row = audit_row("builder", &audit);
        assert_eq!(row["session_id"], "s1");
        assert_eq!(row["role"], "builder");
        assert_eq!(row["status"], "pass");
        assert_eq!(row["finding_codes"], json!([]));
    }

    #[test]
    fn recommended_action_mapping() {
        assert_eq!(
            recommended_action_for_check("bootstrap_order"),
            Some("prepare_harness_session")
        );
        assert_eq!(
            recommended_action_for_check("mutation_gate"),
            Some("verify_change_readiness")
        );
        assert_eq!(
            recommended_action_for_check("structure_evidence"),
            Some("get_symbols_overview")
        );
        assert_eq!(recommended_action_for_check("unknown"), None);
    }

    #[test]
    fn surface_classification() {
        assert!(is_builder_surface("builder-minimal"));
        assert!(is_builder_surface("refactor-full"));
        assert!(!is_builder_surface("planner-readonly"));
        assert!(is_planner_surface("planner-readonly"));
        assert!(is_planner_surface("reviewer-graph"));
        assert!(!is_planner_surface("builder-minimal"));
    }

    #[test]
    fn coverage_check_builds() {
        let c = coverage_check("my_check", "ready", "ok");
        assert_eq!(c.check, "my_check");
        assert_eq!(c.status, "ready");
        assert_eq!(c.summary, "ok");
        assert_eq!(c.evidence_section, Some("audit_pass_rate".to_owned()));
    }
}
