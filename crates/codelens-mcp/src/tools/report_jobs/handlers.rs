use super::super::{AppState, ToolResult, required_string, success_meta};
use super::helpers::{estimated_sections_for_kind, job_handle_fields};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::resources::analysis_summary_resource;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn start_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let kind = required_string(arguments, "kind")?.to_owned();
    let scope = state.project_scope_for_arguments(arguments);
    let profile_hint = arguments
        .get("profile_hint")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let estimated_sections = estimated_sections_for_kind(&kind);
    let job = state.store_analysis_job(
        &scope,
        &kind,
        profile_hint.clone(),
        estimated_sections.clone(),
        crate::runtime_types::JobLifecycle::Queued,
        0,
        Some("queued".to_owned()),
        None,
        None,
    )?;
    state.enqueue_analysis_job(
        scope,
        job.id.clone(),
        kind,
        arguments.clone(),
        profile_hint.clone(),
    )?;
    let handle_fields = job_handle_fields(job.analysis_id.as_deref(), &estimated_sections);
    Ok((
        json!({
            "job_id": job.id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "analysis_id": job.analysis_id,
            "estimated_sections": estimated_sections,
            "summary_resource": handle_fields["summary_resource"].clone(),
            "section_handles": handle_fields["section_handles"].clone(),
        }),
        success_meta(BackendKind::Hybrid, 0.92),
    ))
}

pub fn get_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let scope = state.project_scope_for_arguments(arguments);
    let job = state
        .get_analysis_job_for_scope(&scope, job_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown job_id `{job_id}`")))?;
    let handle_fields = job_handle_fields(job.analysis_id.as_deref(), &job.estimated_sections);
    Ok((
        json!({
            "job_id": job.id,
            "kind": job.kind,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "profile_hint": job.profile_hint,
            "estimated_sections": job.estimated_sections,
            "analysis_id": job.analysis_id,
            "summary_resource": handle_fields["summary_resource"].clone(),
            "section_handles": handle_fields["section_handles"].clone(),
            "error": job.error,
            "updated_at_ms": job.updated_at_ms,
            "heartbeat_at_ms": job.heartbeat_at_ms,
            "deadline_at_ms": job.deadline_at_ms,
            "cancel_requested_at_ms": job.cancel_requested_at_ms,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn cancel_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let scope = state.project_scope_for_arguments(arguments);
    let job = state.cancel_analysis_job_for_scope(&scope, job_id)?;
    let handle_fields = job_handle_fields(job.analysis_id.as_deref(), &job.estimated_sections);
    Ok((
        json!({
            "job_id": job.id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "analysis_id": job.analysis_id,
            "summary_resource": handle_fields["summary_resource"].clone(),
            "section_handles": handle_fields["section_handles"].clone(),
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn get_analysis_section(state: &AppState, arguments: &Value) -> ToolResult {
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let analysis_id = required_string(arguments, "analysis_id")?;
    let section = required_string(arguments, "section")?;
    let scope = state.project_scope_for_arguments(arguments);
    let artifact = state
        .get_analysis_for_scope(&scope, analysis_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`")))?;
    state
        .metrics()
        .record_analysis_read_for_session(true, Some(session.session_id.as_str()));
    let content = state
        .get_analysis_section_for_scope(&scope, analysis_id, section)
        .map_err(|error| match error {
            CodeLensError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                CodeLensError::NotFound(format!(
                    "analysis `{analysis_id}` has no section `{section}`"
                ))
            }
            other => other,
        })?;
    let payload = json!({
        "analysis_id": analysis_id,
        "section": section,
        "content": content,
        "tool_name": artifact.tool_name,
        "surface": artifact.surface,
    });
    Ok((
        payload,
        success_meta(BackendKind::Memory, artifact.confidence),
    ))
}

pub fn list_analysis_jobs(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = state.project_scope_for_arguments(arguments);
    let status_filter = arguments
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let jobs = state.list_analysis_jobs_for_scope(&scope, status_filter.as_deref());
    let mut status_counts = BTreeMap::new();
    for job in &jobs {
        *status_counts
            .entry(job.status.as_str().to_owned())
            .or_insert(0usize) += 1;
    }
    let items = jobs
        .iter()
        .map(|job| {
            let handle_fields =
                job_handle_fields(job.analysis_id.as_deref(), &job.estimated_sections);
            json!({
                "job_id": job.id,
                "kind": job.kind,
                "status": job.status,
                "progress": job.progress,
                "current_step": job.current_step,
                "analysis_id": job.analysis_id,
                "summary_resource": handle_fields["summary_resource"].clone(),
                "section_handles": handle_fields["section_handles"].clone(),
                "error": job.error,
                "updated_at_ms": job.updated_at_ms,
                "heartbeat_at_ms": job.heartbeat_at_ms,
                "deadline_at_ms": job.deadline_at_ms,
                "cancel_requested_at_ms": job.cancel_requested_at_ms,
            })
        })
        .collect::<Vec<_>>();
    Ok((
        json!({
            "jobs": items,
            "count": items.len(),
            "active_count": jobs.iter().filter(|job| matches!(job.status, crate::runtime_types::JobLifecycle::Queued | crate::runtime_types::JobLifecycle::Running)).count(),
            "status_counts": status_counts,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn list_analysis_artifacts(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = state.project_scope_for_arguments(arguments);
    let summaries = state.list_analysis_summaries_for_scope(&scope);
    let mut tool_counts = BTreeMap::new();
    for summary in &summaries {
        *tool_counts
            .entry(summary.tool_name.clone())
            .or_insert(0usize) += 1;
    }
    let latest_created_at_ms = summaries
        .iter()
        .map(|summary| summary.created_at_ms)
        .max()
        .unwrap_or_default();
    let items = summaries
        .iter()
        .map(|s| {
            json!({
                "analysis_id": s.id,
                "tool_name": s.tool_name,
                "summary": s.summary,
                "created_at_ms": s.created_at_ms,
                "surface": s.surface,
                "summary_resource": analysis_summary_resource(&s.id),
            })
        })
        .collect::<Vec<_>>();
    Ok((
        json!({
            "artifacts": items,
            "count": items.len(),
            "latest_created_at_ms": latest_created_at_ms,
            "tool_counts": tool_counts,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

#[allow(dead_code)]
pub fn retry_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let scope = state.project_scope_for_arguments(arguments);
    let original = state
        .get_analysis_job_for_scope(&scope, job_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown job_id `{job_id}`")))?;
    if !matches!(
        original.status,
        crate::runtime_types::JobLifecycle::Error | crate::runtime_types::JobLifecycle::Cancelled
    ) {
        return Err(CodeLensError::Validation(format!(
            "job `{job_id}` has status `{}` — only error or cancelled jobs can be retried",
            original.status
        )));
    }
    let kind = original.kind.clone();
    let profile_hint = original.profile_hint.clone();
    let estimated_sections = estimated_sections_for_kind(&kind);
    let job = state.store_analysis_job(
        &scope,
        &kind,
        profile_hint.clone(),
        estimated_sections.clone(),
        crate::runtime_types::JobLifecycle::Queued,
        0,
        Some("queued".to_owned()),
        None,
        None,
    )?;
    // Re-enqueue with the original arguments embedded in the new job's arguments.
    // Since we don't persist the original call arguments, we reconstruct from job fields.
    let retry_arguments = json!({
        "kind": kind,
        "profile_hint": profile_hint,
    });
    state.enqueue_analysis_job(scope, job.id.clone(), kind, retry_arguments, profile_hint)?;
    Ok((
        json!({
            "job_id": job.id,
            "retried_from": job_id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "estimated_sections": estimated_sections,
        }),
        success_meta(BackendKind::Hybrid, 0.92),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::helpers::{
        debug_step_delay_ms, estimated_sections_for_kind, job_handle_fields,
    };
    use super::*;

    #[test]
    fn debug_step_delay_ms_default_zero() {
        assert_eq!(debug_step_delay_ms(&json!({})), 0);
    }

    #[test]
    fn debug_step_delay_ms_extracts_and_caps() {
        assert_eq!(
            debug_step_delay_ms(&json!({"debug_step_delay_ms": 100})),
            100
        );
        assert_eq!(
            debug_step_delay_ms(&json!({"debug_step_delay_ms": 500})),
            250
        );
    }

    #[test]
    fn estimated_sections_for_known_kinds() {
        assert_eq!(
            estimated_sections_for_kind("impact_report"),
            vec!["impact_rows".to_owned()]
        );
        assert_eq!(
            estimated_sections_for_kind("eval_session_audit"),
            vec!["audit_pass_rate".to_owned(), "session_rows".to_owned()]
        );
    }

    #[test]
    fn estimated_sections_for_unknown_kind_is_empty() {
        assert!(estimated_sections_for_kind("unknown_kind").is_empty());
    }

    #[test]
    fn job_handle_fields_with_analysis_id() {
        let fields = job_handle_fields(Some("aid123"), &["sec1".to_owned(), "sec2".to_owned()]);
        assert_eq!(
            fields["summary_resource"]["uri"],
            "codelens://analysis/aid123/summary"
        );
        let handles = fields["section_handles"].as_array().unwrap();
        assert_eq!(handles.len(), 2);
    }

    #[test]
    fn job_handle_fields_without_analysis_id() {
        let fields = job_handle_fields(None, &["sec1".to_owned()]);
        assert!(fields["summary_resource"].is_null());
        assert_eq!(fields["section_handles"], json!(Vec::<Value>::new()));
    }
}
