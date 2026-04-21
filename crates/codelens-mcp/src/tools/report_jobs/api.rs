use super::catalog::estimated_sections_for_kind;
use super::handles::job_handle_fields;
use super::{
    AppState, BTreeMap, BackendKind, CodeLensError, ToolResult, Value, analysis_summary_resource,
    json, required_string, success_meta,
};

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
        arguments.clone(),
        profile_hint.clone(),
        estimated_sections.clone(),
        crate::runtime_types::JobLifecycle::Queued,
        0,
        Some("queued".to_owned()),
        None,
        None,
    )?;
    state.enqueue_analysis_job(
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
    let content =
        state
            .get_analysis_section(analysis_id, section)
            .map_err(|error| match error {
                CodeLensError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                    CodeLensError::NotFound(format!(
                        "analysis `{analysis_id}` has no section `{section}`"
                    ))
                }
                other => other,
            })?;
    Ok((
        json!({
            "analysis_id": analysis_id,
            "section": section,
            "content": content,
            "tool_name": artifact.tool_name,
            "surface": artifact.surface,
        }),
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
    let retry_arguments = match original.arguments.as_object() {
        Some(map) if !map.is_empty() => original.arguments.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("kind".to_owned(), json!(kind));
            if let Some(profile_hint) = profile_hint.as_deref() {
                fallback.insert("profile_hint".to_owned(), json!(profile_hint));
            }
            Value::Object(fallback)
        }
    };
    let job = state.store_analysis_job(
        &scope,
        &kind,
        retry_arguments.clone(),
        profile_hint.clone(),
        estimated_sections.clone(),
        crate::runtime_types::JobLifecycle::Queued,
        0,
        Some("queued".to_owned()),
        None,
        None,
    )?;
    state.enqueue_analysis_job(job.id.clone(), kind, retry_arguments, profile_hint)?;
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
