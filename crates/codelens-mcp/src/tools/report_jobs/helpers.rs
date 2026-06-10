use super::super::{AppState, ToolResult};
use crate::error::CodeLensError;
use crate::resources::{analysis_section_handles, analysis_summary_resource};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

pub(super) fn run_job_kind(state: &AppState, kind: &str, arguments: &Value) -> ToolResult {
    match kind {
        "impact_report" => super::super::reports::impact_report(state, arguments),
        "dead_code_report" => super::super::reports::dead_code_report(state, arguments),
        "refactor_safety_report" => super::super::reports::refactor_safety_report(state, arguments),
        "module_boundary_report" => super::super::reports::module_boundary_report(state, arguments),
        "safe_rename_report" => super::super::reports::safe_rename_report(state, arguments),
        "diff_aware_references" => super::super::reports::diff_aware_references(state, arguments),
        "semantic_code_review" => super::super::reports::semantic_code_review(state, arguments),
        "orchestrate_change" => super::super::reports::orchestrate_change(state, arguments),
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

pub(super) fn debug_step_delay_ms(arguments: &Value) -> u64 {
    arguments
        .get("debug_step_delay_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        .min(250)
}

pub(super) fn maybe_delay(ms: u64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms));
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
        "orchestrate_change" => vec![
            "orchestration_run".to_owned(),
            "plan".to_owned(),
            "preflight".to_owned(),
            "audit_events".to_owned(),
            "evidence_handles".to_owned(),
        ],
        "analyze_change_request" => vec!["change_request".to_owned()],
        "verify_change_readiness" => vec!["readiness".to_owned()],
        "eval_session_audit" => vec!["audit_pass_rate".to_owned(), "session_rows".to_owned()],
        _ => Vec::new(),
    }
}

pub(super) fn job_handle_fields(analysis_id: Option<&str>, sections: &[String]) -> Value {
    match analysis_id {
        Some(analysis_id) => json!({
            "summary_resource": analysis_summary_resource(analysis_id),
            "section_handles": analysis_section_handles(analysis_id, sections),
        }),
        None => json!({
            "summary_resource": Value::Null,
            "section_handles": Vec::<Value>::new(),
        }),
    }
}

pub(super) fn patch_job_file(
    project_path: &str,
    job_id: &str,
    status: Option<crate::runtime_types::JobLifecycle>,
    progress: Option<u8>,
    current_step: Option<Option<String>>,
    analysis_id: Option<Option<String>>,
    error: Option<Option<String>>,
) {
    let path = Path::new(project_path)
        .join(".codelens")
        .join("analysis-cache")
        .join("jobs")
        .join(format!("{job_id}.json"));
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let Ok(mut job) = serde_json::from_slice::<crate::state::AnalysisJob>(&bytes) else {
        return;
    };
    if let Some(status) = status {
        job.status = status;
    }
    if let Some(progress) = progress {
        job.progress = progress;
    }
    if let Some(current_step) = current_step {
        job.current_step = current_step;
    }
    if let Some(analysis_id) = analysis_id {
        job.analysis_id = analysis_id;
    }
    if let Some(error) = error {
        job.error = error;
    }
    job.updated_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if let Ok(updated) = serde_json::to_vec_pretty(&job) {
        let tmp_path = path.with_extension("json.tmp");
        let _ = std::fs::write(&tmp_path, updated);
        let _ = std::fs::rename(tmp_path, path);
    }
}

pub(super) fn advance_job_progress(
    state: &AppState,
    scope: &str,
    job_id: &str,
    progress: u8,
    current_step: &str,
    delay_ms: u64,
) -> Result<bool, String> {
    if state
        .get_analysis_job_for_scope(scope, job_id)
        .as_ref()
        .map(|job| job.status)
        == Some(crate::runtime_types::JobLifecycle::Cancelled)
    {
        return Ok(false);
    }
    state
        .update_analysis_job(
            scope,
            job_id,
            Some(crate::runtime_types::JobLifecycle::Running),
            Some(progress),
            Some(Some(current_step.to_owned())),
            None,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
    maybe_delay(delay_ms);
    Ok(true)
}
