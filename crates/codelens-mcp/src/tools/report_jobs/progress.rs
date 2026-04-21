use super::{AppState, Duration, Path, Value};

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
