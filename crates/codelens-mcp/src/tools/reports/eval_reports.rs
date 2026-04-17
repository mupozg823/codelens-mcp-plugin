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

use crate::tool_runtime::ToolResult;
use crate::tools::report_contract::make_handle_response;
use crate::tools::session::builder_audit::build_builder_session_audit;
use crate::tools::session::planner_audit::build_planner_session_audit;
use crate::AppState;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
struct AuditStats {
    pass: u32,
    warn: u32,
    fail: u32,
    not_applicable: u32,
}

impl AuditStats {
    fn record(&mut self, status: &str) {
        match status {
            "pass" => self.pass += 1,
            "warn" => self.warn += 1,
            "fail" => self.fail += 1,
            "not_applicable" => self.not_applicable += 1,
            _ => {}
        }
    }

    fn applicable(&self) -> u32 {
        self.pass + self.warn + self.fail
    }

    fn pass_rate(&self) -> f64 {
        let denom = self.applicable();
        if denom == 0 {
            0.0
        } else {
            self.pass as f64 / denom as f64
        }
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

fn stats_to_json(stats: &AuditStats) -> Value {
    json!({
        "pass": stats.pass,
        "warn": stats.warn,
        "fail": stats.fail,
        "not_applicable": stats.not_applicable,
        "applicable_count": stats.applicable(),
        "pass_rate": stats.pass_rate(),
    })
}

pub fn eval_session_audit(state: &AppState, arguments: &Value) -> ToolResult {
    let session_ids = state.metrics().tracked_session_ids();
    let session_count = session_ids.len();

    let mut builder = AuditStats::default();
    let mut planner = AuditStats::default();
    let mut failed_checks: HashMap<String, usize> = HashMap::new();

    for sid in &session_ids {
        let args = json!({ "session_id": sid });
        if let Ok(audit) = build_builder_session_audit(state, &args) {
            let status = audit["status"].as_str().unwrap_or("");
            builder.record(status);
            if matches!(status, "warn" | "fail") {
                collect_failed(&audit, &mut failed_checks);
            }
        }
        if let Ok(audit) = build_planner_session_audit(state, &args) {
            let status = audit["status"].as_str().unwrap_or("");
            planner.record(status);
            if matches!(status, "warn" | "fail") {
                collect_failed(&audit, &mut failed_checks);
            }
        }
    }

    let mut top_failed: Vec<(String, usize)> = failed_checks.into_iter().collect();
    top_failed.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_failed_json: Vec<Value> = top_failed
        .iter()
        .take(10)
        .map(|(code, count)| json!({ "code": code, "count": count }))
        .collect();

    let mut sections: BTreeMap<String, Value> = BTreeMap::new();
    sections.insert(
        "audit_pass_rate".to_owned(),
        json!({
            "session_count": session_count,
            "builder": stats_to_json(&builder),
            "planner": stats_to_json(&planner),
            "top_failed_checks": top_failed_json,
        }),
    );

    let summary = format!(
        "Aggregated audit signal over {} session(s): builder pass={}/warn={}/fail={}, planner pass={}/warn={}/fail={}",
        session_count, builder.pass, builder.warn, builder.fail, planner.pass, planner.warn, planner.fail,
    );
    let top_findings = vec![
        format!(
            "builder pass_rate={:.3} over {} applicable session(s)",
            builder.pass_rate(),
            builder.applicable()
        ),
        format!(
            "planner pass_rate={:.3} over {} applicable session(s)",
            planner.pass_rate(),
            planner.applicable()
        ),
    ];
    let next_actions = if session_count == 0 {
        vec![
            "run audit_builder_session or audit_planner_session on a live session first".to_owned(),
        ]
    } else {
        vec![
            "drill into top_failed_checks via audit_builder_session({session_id}) for the flagged sessions".to_owned(),
        ]
    };

    make_handle_response(
        state,
        "eval_session_audit",
        None,
        summary,
        top_findings,
        0.92,
        next_actions,
        sections,
        Vec::new(),
        None,
        Some(arguments),
    )
}
