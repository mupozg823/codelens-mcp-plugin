use super::report_utils::{extract_handle_fields, stable_cache_key, strings_from_array};
use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

fn run_job_kind(state: &AppState, kind: &str, arguments: &Value) -> ToolResult {
    match kind {
        "impact_report" => super::reports::impact_report(state, arguments),
        "dead_code_report" => super::reports::dead_code_report(state, arguments),
        "refactor_safety_report" => super::reports::refactor_safety_report(state, arguments),
        "module_boundary_report" => super::reports::module_boundary_report(state, arguments),
        "safe_rename_report" => super::reports::safe_rename_report(state, arguments),
        "diff_aware_references" => super::reports::diff_aware_references(state, arguments),
        "semantic_code_review" => super::reports::semantic_code_review(state, arguments),
        "analyze_change_request" => super::reports::analyze_change_request(state, arguments),
        "verify_change_readiness" => super::reports::verify_change_readiness(state, arguments),
        _ => Err(CodeLensError::Validation(format!(
            "unsupported analysis job kind `{kind}`"
        ))),
    }
}

fn debug_step_delay_ms(arguments: &Value) -> u64 {
    arguments
        .get("debug_step_delay_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        .min(250)
}

fn maybe_delay(ms: u64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

fn estimated_sections_for_kind(kind: &str) -> Vec<String> {
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
        _ => Vec::new(),
    }
}

fn patch_job_file(
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

fn advance_job_progress(
    state: &AppState,
    job_id: &str,
    progress: u8,
    current_step: &str,
    delay_ms: u64,
) -> Result<bool, String> {
    if state
        .get_analysis_job(job_id)
        .as_ref()
        .map(|job| job.status)
        == Some(crate::runtime_types::JobLifecycle::Cancelled)
    {
        return Ok(false);
    }
    state
        .update_analysis_job(
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

fn run_job_kind_with_progress(
    state: &AppState,
    job_id: &str,
    kind: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let delay_ms = debug_step_delay_ms(arguments);
    match kind {
        "impact_report" => run_impact_report_job(state, job_id, arguments, delay_ms),
        "dead_code_report" => run_dead_code_report_job(state, job_id, arguments, delay_ms),
        "refactor_safety_report" => {
            run_refactor_safety_report_job(state, job_id, arguments, delay_ms)
        }
        "module_boundary_report" => run_simple_report_job(state, job_id, kind, arguments, delay_ms),
        "safe_rename_report" => run_simple_report_job(state, job_id, kind, arguments, delay_ms),
        "diff_aware_references" => run_simple_report_job(state, job_id, kind, arguments, delay_ms),
        "semantic_code_review" => run_simple_report_job(state, job_id, kind, arguments, delay_ms),
        "analyze_change_request" => run_simple_report_job(state, job_id, kind, arguments, delay_ms),
        "verify_change_readiness" => {
            run_simple_report_job(state, job_id, kind, arguments, delay_ms)
        }
        _ => run_job_kind(state, kind, arguments)
            .map(|(payload, _meta)| payload)
            .map_err(|error| error.to_string()),
    }
}

/// Generic progress wrapper for report kinds that don't need step-level progress tracking.
fn run_simple_report_job(
    state: &AppState,
    job_id: &str,
    kind: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    if !advance_job_progress(state, job_id, 30, &format!("starting {kind}"), delay_ms)? {
        return Ok(json!({}));
    }
    let result = run_job_kind(state, kind, arguments)
        .map(|(payload, _meta)| payload)
        .map_err(|error| error.to_string())?;
    if !advance_job_progress(state, job_id, 90, &format!("finalizing {kind}"), delay_ms)? {
        return Ok(json!({}));
    }
    Ok(result)
}

fn run_dead_code_report_job(
    state: &AppState,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20);
    if !advance_job_progress(state, job_id, 20, "scanning dead code candidates", delay_ms)? {
        return Ok(json!({}));
    }
    let dead_code =
        super::graph::find_dead_code_v2_tool(state, &json!({"max_results": max_results}))
            .map(|output| output.0)
            .map_err(|error| error.to_string())?;
    if !advance_job_progress(state, job_id, 70, "filtering scoped dead code", delay_ms)? {
        return Ok(json!({}));
    }
    let candidates = dead_code
        .get("dead_code")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.to_string().contains(scope))
        .take(10)
        .collect::<Vec<_>>();
    let top_findings = strings_from_array(Some(&candidates), "file", 3);
    let mut sections = BTreeMap::new();
    sections.insert(
        "candidates".to_owned(),
        json!({"scope": scope, "dead_code": candidates}),
    );
    sections.insert("raw_dead_code".to_owned(), dead_code);
    if !advance_job_progress(state, job_id, 90, "writing dead code analysis", delay_ms)? {
        return Ok(json!({}));
    }
    super::report_contract::make_handle_response(
        state,
        "dead_code_report",
        stable_cache_key("dead_code_report", arguments, &["scope", "max_results"]),
        format!("Bounded dead-code audit for scope `{scope}`."),
        top_findings,
        0.84,
        vec!["Validate runtime entry points before deleting candidates".to_owned()],
        sections,
        if scope == "." {
            Vec::new()
        } else {
            vec![scope.to_owned()]
        },
        None,
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

fn run_impact_report_job(
    state: &AppState,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    if !advance_job_progress(state, job_id, 20, "collecting changed files", delay_ms)? {
        return Ok(json!({}));
    }
    let changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let target_files = if !changed_files.is_empty() {
        changed_files
    } else if let Some(path) = arguments.get("path").and_then(|value| value.as_str()) {
        vec![path.to_owned()]
    } else {
        let changed =
            super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))
                .map(|out| out.0)
                .unwrap_or_else(|_| json!({"files": [], "count": 0}));
        strings_from_array(
            changed.get("files").and_then(|value| value.as_array()),
            "file",
            8,
        )
    };
    if !advance_job_progress(state, job_id, 45, "measuring impact surface", delay_ms)? {
        return Ok(json!({}));
    }
    let mut impact_rows = Vec::new();
    let mut top_findings = Vec::new();
    let total = target_files.iter().take(5).count().max(1);
    for (idx, path) in target_files.iter().take(5).enumerate() {
        let impact = super::graph::get_impact_analysis(
            state,
            &json!({"file_path": path, "max_depth": 2}),
        )
        .map(|output| output.0)
        .unwrap_or_else(
            |_| json!({"file_path": path, "total_affected_files": 0, "direct_importers": []}),
        );
        let affected = impact
            .get("total_affected_files")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let change_kind = codelens_core::git::classify_change_kind(&state.project(), path);
        let kind_label = if change_kind == "additive" {
            " (additive)"
        } else {
            ""
        };
        top_findings.push(format!("{path}: {affected} affected file(s){kind_label}"));
        impact_rows.push(json!({
            "path": path,
            "affected_files": affected,
            "change_kind": change_kind,
            "direct_importers": impact.get("direct_importers").cloned().unwrap_or(json!([])),
            "blast_radius": impact.get("blast_radius").cloned().unwrap_or(json!([])),
        }));
        let loop_progress = 45 + (((idx + 1) * 35) / total) as u8;
        if !advance_job_progress(
            state,
            job_id,
            loop_progress.min(80),
            &format!("analyzed impact for {path}"),
            delay_ms,
        )? {
            return Ok(json!({}));
        }
    }
    let mut sections = BTreeMap::new();
    sections.insert(
        "impact_rows".to_owned(),
        json!({"files": target_files, "impacts": impact_rows}),
    );
    if !advance_job_progress(state, job_id, 90, "writing impact analysis", delay_ms)? {
        return Ok(json!({}));
    }
    super::report_contract::make_handle_response(
        state,
        "impact_report",
        stable_cache_key("impact_report", arguments, &["path", "changed_files"]),
        "Diff-aware impact report with bounded blast radius and importer evidence.".to_owned(),
        top_findings,
        0.88,
        vec!["Expand only the highest-impact file before deeper review".to_owned()],
        sections,
        target_files,
        None,
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

fn run_refactor_safety_report_job(
    state: &AppState,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    let path = arguments
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or(".");
    let task = arguments.get("task").and_then(|value| value.as_str());
    let symbol = arguments.get("symbol").and_then(|value| value.as_str());
    if !advance_job_progress(state, job_id, 20, "analyzing module boundaries", delay_ms)? {
        return Ok(json!({}));
    }
    let boundary = super::reports::module_boundary_report(state, &json!({"path": path}))
        .map(|output| output.0)
        .map_err(|error| error.to_string())?;
    if !advance_job_progress(state, job_id, 40, "summarizing symbol impact", delay_ms)? {
        return Ok(json!({}));
    }
    let symbol_impact = if let Some(symbol) = symbol {
        super::reports::summarize_symbol_impact(
            state,
            &json!({"symbol": symbol, "file_path": arguments.get("file_path").and_then(|v| v.as_str())}),
        )
        .map(|output| output.0)
        .unwrap_or_else(|error| json!({"symbol": symbol, "error": error.to_string()}))
    } else {
        json!({"skipped": true, "reason": "no symbol provided"})
    };
    if !advance_job_progress(state, job_id, 60, "ranking refactor context", delay_ms)? {
        return Ok(json!({}));
    }
    let change_request = task
        .map(|task| {
            super::reports::analyze_change_request(state, &json!({"task": task}))
                .map(|output| output.0)
        })
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| json!({"skipped": true, "reason": "no task provided"}));
    if !advance_job_progress(state, job_id, 80, "collecting related tests", delay_ms)? {
        return Ok(json!({}));
    }
    let tests = super::filesystem::find_tests(state, &json!({"path": path, "max_results": 10}))
        .map(|output| output.0)
        .unwrap_or_else(|_| json!({"tests": []}));

    let mut top_findings = Vec::new();
    if let Some(symbol) = symbol {
        top_findings.push(format!(
            "Validate symbol-level callers before refactoring `{symbol}`."
        ));
    }
    if let Some(task) = task {
        top_findings.push(format!("Keep the refactor aligned with `{task}`."));
    }
    top_findings.push(format!(
        "Check tests around `{path}` before applying broad edits."
    ));

    let mut sections = BTreeMap::new();
    sections.insert("module_boundary".to_owned(), boundary);
    sections.insert("symbol_impact".to_owned(), symbol_impact);
    sections.insert("change_request".to_owned(), change_request);
    sections.insert("related_tests".to_owned(), tests);
    super::report_contract::make_handle_response(
        state,
        "refactor_safety_report",
        stable_cache_key(
            "refactor_safety_report",
            arguments,
            &["task", "symbol", "path", "file_path"],
        ),
        format!("Preview-first refactor safety report for `{path}`."),
        top_findings,
        0.9,
        vec!["Use safe_rename_report or focused edits only after checking blockers".to_owned()],
        sections,
        vec![arguments
            .get("file_path")
            .and_then(|value| value.as_str())
            .unwrap_or(path)
            .to_owned()],
        symbol.map(ToOwned::to_owned),
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

pub(crate) fn run_analysis_job_from_queue(
    worker_state: &AppState,
    job_id: String,
    kind: String,
    arguments: Value,
) -> crate::runtime_types::JobLifecycle {
    use crate::runtime_types::JobLifecycle;
    let project_path = worker_state
        .project()
        .as_path()
        .to_string_lossy()
        .to_string();
    if worker_state
        .get_analysis_job(&job_id)
        .as_ref()
        .map(|job| job.status)
        == Some(crate::runtime_types::JobLifecycle::Cancelled)
    {
        return JobLifecycle::Cancelled;
    }
    patch_job_file(
        &project_path,
        &job_id,
        Some(crate::runtime_types::JobLifecycle::Running),
        Some(5),
        Some(Some("worker started".to_owned())),
        None,
        None,
    );
    let worker = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<JobLifecycle, String> {
            if worker_state
                .get_analysis_job(&job_id)
                .as_ref()
                .map(|job| job.status)
                == Some(JobLifecycle::Cancelled)
            {
                return Ok(JobLifecycle::Cancelled);
            }
            // Retry transient failures up to 2 times with backoff
            let mut last_err = None;
            let mut result = None;
            for attempt in 0..3 {
                match run_job_kind_with_progress(worker_state, &job_id, &kind, &arguments) {
                    Ok(payload) => {
                        result = Some(payload);
                        break;
                    }
                    Err(e) if attempt < 2 => {
                        last_err = Some(e);
                        std::thread::sleep(std::time::Duration::from_millis(
                            100 * (attempt as u64 + 1),
                        ));
                        continue;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        break;
                    }
                }
            }
            match result.map(Ok).unwrap_or_else(|| Err(last_err.unwrap())) {
                Ok(payload) if payload.is_object() => {
                    let (analysis_id, estimated_sections) = extract_handle_fields(&payload);
                    let current = worker_state.get_analysis_job(&job_id);
                    if current.as_ref().map(|job| job.status) == Some(JobLifecycle::Cancelled) {
                        return Ok(JobLifecycle::Cancelled);
                    }
                    worker_state
                        .update_analysis_job(
                            &job_id,
                            Some(JobLifecycle::Completed),
                            Some(100),
                            Some(Some("completed".to_owned())),
                            Some(estimated_sections),
                            Some(analysis_id),
                            Some(None),
                        )
                        .map_err(|error| error.to_string())?;
                    Ok(JobLifecycle::Completed)
                }
                Ok(_) => Ok(JobLifecycle::Error),
                Err(error) => {
                    worker_state
                        .update_analysis_job(
                            &job_id,
                            Some(JobLifecycle::Error),
                            Some(100),
                            Some(Some("failed".to_owned())),
                            None,
                            Some(None),
                            Some(Some(error.to_string())),
                        )
                        .map_err(|error| error.to_string())?;
                    Ok(JobLifecycle::Error)
                }
            }
        },
    ));
    match worker {
        Err(panic) => {
            let message = if let Some(text) = panic.downcast_ref::<&str>() {
                (*text).to_owned()
            } else if let Some(text) = panic.downcast_ref::<String>() {
                text.clone()
            } else {
                "analysis worker panicked".to_owned()
            };
            patch_job_file(
                &project_path,
                &job_id,
                Some(JobLifecycle::Error),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(message)),
            );
            JobLifecycle::Error
        }
        Ok(Err(message)) => {
            patch_job_file(
                &project_path,
                &job_id,
                Some(JobLifecycle::Error),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(message)),
            );
            JobLifecycle::Error
        }
        Ok(Ok(status)) => status,
    }
}

pub fn start_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let kind = required_string(arguments, "kind")?.to_owned();
    let profile_hint = arguments
        .get("profile_hint")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let estimated_sections = estimated_sections_for_kind(&kind);
    let job = state.store_analysis_job(
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
        job.id.clone(),
        kind,
        arguments.clone(),
        profile_hint.clone(),
    )?;
    Ok((
        json!({
            "job_id": job.id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "analysis_id": job.analysis_id,
            "estimated_sections": estimated_sections,
        }),
        success_meta(BackendKind::Hybrid, 0.92),
    ))
}

pub fn get_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let job = state
        .get_analysis_job(job_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown job_id `{job_id}`")))?;
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
            "error": job.error,
            "updated_at_ms": job.updated_at_ms,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn cancel_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let job = state.cancel_analysis_job(job_id)?;
    Ok((
        json!({
            "job_id": job.id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "analysis_id": job.analysis_id,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn get_analysis_section(state: &AppState, arguments: &Value) -> ToolResult {
    let analysis_id = required_string(arguments, "analysis_id")?;
    let section = required_string(arguments, "section")?;
    let artifact = state
        .get_analysis(analysis_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`")))?;
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
    let status_filter = arguments
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let jobs = state.list_analysis_jobs(status_filter.as_deref());
    let items = jobs
        .iter()
        .map(|job| {
            json!({
                "job_id": job.id,
                "kind": job.kind,
                "status": job.status,
                "progress": job.progress,
                "current_step": job.current_step,
                "analysis_id": job.analysis_id,
                "error": job.error,
                "updated_at_ms": job.updated_at_ms,
            })
        })
        .collect::<Vec<_>>();
    Ok((
        json!({
            "jobs": items,
            "count": items.len(),
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn list_analysis_artifacts(state: &AppState, _arguments: &Value) -> ToolResult {
    let summaries = state.list_analysis_summaries();
    let items = summaries
        .iter()
        .map(|s| {
            json!({
                "analysis_id": s.id,
                "tool_name": s.tool_name,
                "summary": s.summary,
                "created_at_ms": s.created_at_ms,
                "surface": s.surface,
            })
        })
        .collect::<Vec<_>>();
    Ok((
        json!({
            "artifacts": items,
            "count": items.len(),
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn retry_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let original = state
        .get_analysis_job(job_id)
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
