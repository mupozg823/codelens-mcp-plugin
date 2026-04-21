use super::{AppState, ToolResult, Value};
use crate::error::CodeLensError;

pub(super) fn run_job_kind(state: &AppState, kind: &str, arguments: &Value) -> ToolResult {
    match kind {
        "impact_report" => super::super::reports::impact_report(state, arguments),
        "dead_code_report" => super::super::reports::dead_code_report(state, arguments),
        "refactor_safety_report" => super::super::reports::refactor_safety_report(state, arguments),
        "module_boundary_report" => super::super::reports::module_boundary_report(state, arguments),
        "safe_rename_report" => super::super::reports::safe_rename_report(state, arguments),
        "diff_aware_references" => super::super::reports::diff_aware_references(state, arguments),
        "semantic_code_review" => super::super::reports::semantic_code_review(state, arguments),
        "analyze_change_request" => super::super::reports::analyze_change_request(state, arguments),
        "verify_change_readiness" => {
            super::super::reports::verify_change_readiness(state, arguments)
        }
        "eval_session_audit" => super::super::reports::eval_session_audit(state, arguments),
        _ => Err(CodeLensError::Validation(format!(
            "unsupported analysis job kind `{kind}`"
        ))),
    }
}

pub(super) fn estimated_sections_for_kind(kind: &str) -> Vec<String> {
    match kind {
        "impact_report" => vec!["impact_rows".to_owned()],
        "dead_code_report" => vec!["candidates".to_owned(), "raw_dead_code".to_owned()],
        "refactor_safety_report" => vec![
            "module_boundary".to_owned(),
            "symbol_impact".to_owned(),
            "change_request".to_owned(),
            "related_tests".to_owned(),
        ],
        "module_boundary_report" => vec!["boundary".to_owned()],
        "safe_rename_report" => vec!["rename_safety".to_owned()],
        "diff_aware_references" => vec!["references".to_owned()],
        "semantic_code_review" => vec!["review_items".to_owned(), "semantic_status".to_owned()],
        "analyze_change_request" => vec!["change_request".to_owned()],
        "verify_change_readiness" => vec!["readiness".to_owned()],
        "eval_session_audit" => vec!["audit_pass_rate".to_owned(), "session_rows".to_owned()],
        _ => Vec::new(),
    }
}
