use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::CodeLensError;
use crate::runtime_types::{AnalysisJob, JobLifecycle};

pub(crate) const MAX_ANALYSIS_JOBS: usize = 128;
const TTL_MS: u64 = 24 * 60 * 60 * 1000;

pub(crate) struct AnalysisJobStore {
    jobs_dir: Mutex<PathBuf>,
    seq: std::sync::atomic::AtomicU64,
    order: Mutex<VecDeque<String>>,
    jobs: Mutex<HashMap<String, AnalysisJob>>,
}

impl AnalysisJobStore {
    pub fn new(jobs_dir: PathBuf) -> Self {
        Self {
            jobs_dir: Mutex::new(jobs_dir),
            seq: std::sync::atomic::AtomicU64::new(0),
            order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_JOBS)),
            jobs: Mutex::new(HashMap::new()),
        }
    }

    fn jobs_dir(&self) -> PathBuf {
        self.jobs_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_jobs_dir(&self, dir: PathBuf) {
        *self.jobs_dir.lock().unwrap_or_else(|p| p.into_inner()) = dir;
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn expired(updated_at_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(updated_at_ms) > TTL_MS
    }

    fn job_path(&self, job_id: &str) -> PathBuf {
        self.jobs_dir().join(format!("{job_id}.json"))
    }

    fn write_to_disk(&self, job: &AnalysisJob) -> Result<(), CodeLensError> {
        let dir = self.jobs_dir();
        fs::create_dir_all(&dir)?;
        let bytes =
            serde_json::to_vec_pretty(job).map_err(|e| CodeLensError::Internal(e.into()))?;
        let path = dir.join(format!("{}.json", job.id));
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn remove_from_disk(&self, job_id: &str) {
        let _ = fs::remove_file(self.job_path(job_id));
    }

    pub fn cleanup_stale_files(&self, now_ms: u64, project_scope: Option<&str>) {
        let entries = match fs::read_dir(self.jobs_dir()) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let job = fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<AnalysisJob>(&b).ok());
            match job {
                Some(job)
                    if Self::expired(job.updated_at_ms, now_ms)
                        || !matches_scope(job.project_scope.as_deref(), project_scope) =>
                {
                    let _ = fs::remove_file(&path);
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
                _ => {}
            }
        }
    }

    fn prune(&self, now_ms: u64, project_scope: Option<&str>) {
        self.cleanup_stale_files(now_ms, project_scope);

        let expired = {
            let jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
            jobs.iter()
                .filter(|(_, job)| Self::expired(job.updated_at_ms, now_ms))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        let mut evicted = expired;
        {
            let mut order = self.order.lock().unwrap_or_else(|p| p.into_inner());
            if !evicted.is_empty() {
                order.retain(|id| !evicted.contains(id));
            }
            while order.len() > MAX_ANALYSIS_JOBS {
                if let Some(oldest) = order.pop_front() {
                    evicted.push(oldest);
                }
            }
        }

        if evicted.is_empty() {
            return;
        }
        evicted.sort();
        evicted.dedup();
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        for id in &evicted {
            jobs.remove(id);
        }
        drop(jobs);
        for id in evicted {
            self.remove_from_disk(&id);
        }
    }

    pub fn clear(&self) {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner()).clear();
        self.order.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    pub fn store(
        &self,
        kind: &str,
        profile_hint: Option<String>,
        estimated_sections: Vec<String>,
        status: JobLifecycle,
        progress: u8,
        current_step: Option<String>,
        analysis_id: Option<String>,
        error: Option<String>,
        project_scope: String,
    ) -> Result<AnalysisJob, CodeLensError> {
        let now_ms = Self::now_ms();
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("job-{now_ms}-{seq}");
        let job = AnalysisJob {
            id: id.clone(),
            kind: kind.to_owned(),
            project_scope: Some(project_scope),
            status,
            progress,
            current_step,
            profile_hint,
            estimated_sections,
            analysis_id,
            error,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        self.write_to_disk(&job)?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), job.clone());
        self.order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(id);
        self.prune(now_ms, job.project_scope.as_deref());
        Ok(job)
    }

    /// Get a job by ID. Returns the job without warming the artifact cache —
    /// the caller (AppState) handles that cross-concern.
    pub fn get(&self, job_id: &str, project_scope: Option<&str>) -> Option<AnalysisJob> {
        self.prune(Self::now_ms(), project_scope);
        let path = self.job_path(job_id);
        let job = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<AnalysisJob>(&b).ok())
            .filter(|j| matches_scope(j.project_scope.as_deref(), project_scope))
            .or_else(|| {
                self.jobs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(job_id)
                    .cloned()
                    .filter(|j| matches_scope(j.project_scope.as_deref(), project_scope))
            })?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.to_owned(), job.clone());
        Some(job)
    }

    pub fn cancel(
        &self,
        job_id: &str,
        project_scope: Option<&str>,
    ) -> Result<AnalysisJob, CodeLensError> {
        self.prune(Self::now_ms(), project_scope);
        let mut job = self
            .get(job_id, project_scope)
            .ok_or_else(|| CodeLensError::NotFound(format!("Unknown job `{job_id}`")))?;
        if job.status != JobLifecycle::Completed {
            let previous = job.status;
            job.status = JobLifecycle::Cancelled;
            job.progress = 0;
            job.current_step = Some("cancelled".to_owned());
            job.updated_at_ms = Self::now_ms();
            // Return previous status so caller can record metrics
            let _ = previous;
        }
        self.write_to_disk(&job)?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.to_owned(), job.clone());
        Ok(job)
    }

    /// List jobs, optionally filtered by status string (e.g. "queued", "running",
    /// "completed", "cancelled", "error"). Returns jobs ordered newest-first.
    pub fn list(
        &self,
        status_filter: Option<&str>,
        project_scope: Option<&str>,
    ) -> Vec<AnalysisJob> {
        self.prune(Self::now_ms(), project_scope);
        let jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        let order = self.order.lock().unwrap_or_else(|p| p.into_inner());
        order
            .iter()
            .rev()
            .filter_map(|id| jobs.get(id))
            .filter(|job| matches_scope(job.project_scope.as_deref(), project_scope))
            .filter(|job| {
                status_filter
                    .map(|f| job.status.as_str() == f)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    pub fn update(
        &self,
        job_id: &str,
        status: Option<JobLifecycle>,
        progress: Option<u8>,
        current_step: Option<Option<String>>,
        estimated_sections: Option<Vec<String>>,
        analysis_id: Option<Option<String>>,
        error: Option<Option<String>>,
        project_scope: Option<&str>,
    ) -> Result<AnalysisJob, CodeLensError> {
        let path = self.job_path(job_id);
        let mut job = self
            .get(job_id, project_scope)
            .ok_or_else(|| CodeLensError::NotFound(format!("Unknown job `{job_id}`")))?;
        if let Some(s) = status {
            job.status = s;
        }
        if let Some(p) = progress {
            job.progress = p;
        }
        if let Some(cs) = current_step {
            job.current_step = cs;
        }
        if let Some(es) = estimated_sections {
            job.estimated_sections = es;
        }
        if let Some(aid) = analysis_id {
            job.analysis_id = aid;
        }
        if let Some(e) = error {
            job.error = e;
        }
        job.updated_at_ms = Self::now_ms();
        self.write_to_disk(&job)?;
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.to_owned(), job.clone());
        if !path.exists() {
            self.order
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push_back(job_id.to_owned());
        }
        Ok(job)
    }
}

fn matches_scope(job_scope: Option<&str>, current_scope: Option<&str>) -> bool {
    match (job_scope, current_scope) {
        (Some(j), Some(c)) => j == c,
        (None, _) => true,
        (_, None) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_stale_files_ignores_inflight_tmp_files() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-job-store-test-{}-{}",
            std::process::id(),
            AnalysisJobStore::now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        let store = AnalysisJobStore::new(dir.clone());

        let tmp_path = dir.join("job-1.json.tmp");
        fs::write(&tmp_path, br#"{"inflight":true}"#).unwrap();

        store.cleanup_stale_files(AnalysisJobStore::now_ms(), None);

        assert!(tmp_path.exists(), "cleanup should not remove inflight tmp files");
        let _ = fs::remove_file(tmp_path);
        let _ = fs::remove_dir_all(dir);
    }
}
