use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;

use crate::AppState;
use crate::error::CodeLensError;

pub(crate) const MAX_PENDING_ANALYSIS_REQUESTS: usize = 32;
pub(crate) const STDIO_ANALYSIS_WORKER_COUNT: usize = 1;
pub(crate) const HTTP_ANALYSIS_WORKER_COUNT: usize = 2;

pub(crate) fn analysis_job_cost_units(kind: &str) -> usize {
    match kind {
        "impact_report" => 1,
        "refactor_safety_report" => 2,
        "dead_code_report" => 3,
        _ => 2,
    }
}

fn queue_pending_cost_units(pending: &VecDeque<AnalysisJobRequest>) -> usize {
    pending.iter().map(|request| request.cost_units).sum()
}

pub(crate) struct AnalysisJobRequest {
    pub(crate) job_id: String,
    pub(crate) kind: String,
    pub(crate) arguments: Value,
    pub(crate) profile_hint: Option<String>,
    pub(crate) cost_units: usize,
}

struct AnalysisQueueState {
    pending: VecDeque<AnalysisJobRequest>,
    active_jobs: usize,
    active_cost_units: usize,
}

pub(crate) struct AnalysisWorkerQueue {
    inner: Arc<(Mutex<AnalysisQueueState>, Condvar)>,
}

impl AnalysisWorkerQueue {
    pub(crate) fn new(state: &AppState) -> Self {
        let inner = Arc::new((
            Mutex::new(AnalysisQueueState {
                pending: VecDeque::new(),
                active_jobs: 0,
                active_cost_units: 0,
            }),
            Condvar::new(),
        ));
        let worker_limit = state.analysis_worker_limit();
        state.metrics().record_analysis_worker_pool(
            worker_limit,
            state.analysis_cost_budget(),
            state.transport_mode().as_str(),
        );
        for _ in 0..worker_limit {
            let inner_clone = Arc::clone(&inner);
            let worker_state = state.clone_for_worker();
            std::thread::spawn(move || {
                loop {
                    let request = {
                        let (lock, condvar) = &*inner_clone;
                        let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            if guard.pending.is_empty() {
                                guard = condvar.wait(guard).unwrap_or_else(|p| p.into_inner());
                                continue;
                            }
                            let cost_budget = worker_state.analysis_cost_budget();
                            let next_index = guard.pending.iter().position(|request| {
                                let allowed_parallelism = worker_state
                                    .analysis_parallelism_for_profile(
                                        request.profile_hint.as_deref(),
                                    );
                                guard.active_jobs < allowed_parallelism
                                    && guard.active_cost_units + request.cost_units <= cost_budget
                            });
                            if let Some(index) = next_index {
                                let request = guard.pending.remove(index);
                                guard.active_jobs += 1;
                                if let Some(request) = request.as_ref() {
                                    guard.active_cost_units += request.cost_units;
                                }
                                let remaining_depth = guard.pending.len();
                                let remaining_cost_units = queue_pending_cost_units(&guard.pending)
                                    + guard.active_cost_units;
                                break request.map(|request| {
                                    (request, remaining_depth, remaining_cost_units)
                                });
                            }
                            guard = condvar.wait(guard).unwrap_or_else(|p| p.into_inner());
                        }
                    };
                    if let Some((request, remaining_depth, remaining_cost_units)) = request {
                        if worker_state
                            .get_analysis_job(&request.job_id)
                            .as_ref()
                            .map(|job| job.status.as_str())
                            == Some("cancelled")
                        {
                            let (lock, condvar) = &*inner_clone;
                            let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                            guard.active_jobs = guard.active_jobs.saturating_sub(1);
                            guard.active_cost_units =
                                guard.active_cost_units.saturating_sub(request.cost_units);
                            condvar.notify_all();
                            continue;
                        }
                        let request_cost = request.cost_units;
                        worker_state
                            .metrics()
                            .record_analysis_job_started(remaining_depth, remaining_cost_units);
                        let final_status = crate::tools::reports::run_analysis_job_from_queue(
                            &worker_state,
                            request.job_id,
                            request.kind,
                            request.arguments,
                        );
                        let (remaining_depth, remaining_cost_units) = {
                            let (lock, condvar) = &*inner_clone;
                            let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                            guard.active_jobs = guard.active_jobs.saturating_sub(1);
                            guard.active_cost_units =
                                guard.active_cost_units.saturating_sub(request_cost);
                            let remaining_depth = guard.pending.len();
                            let remaining_cost_units =
                                queue_pending_cost_units(&guard.pending) + guard.active_cost_units;
                            condvar.notify_all();
                            (remaining_depth, remaining_cost_units)
                        };
                        worker_state.metrics().record_analysis_job_finished(
                            final_status,
                            remaining_depth,
                            remaining_cost_units,
                        );
                    }
                }
            });
        }
        Self { inner }
    }

    pub(crate) fn enqueue(
        &self,
        request: AnalysisJobRequest,
    ) -> Result<(usize, usize, bool), CodeLensError> {
        let (lock, condvar) = &*self.inner;
        let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        if guard.pending.len() >= MAX_PENDING_ANALYSIS_REQUESTS {
            return Err(CodeLensError::Validation(format!(
                "analysis queue is full (>{MAX_PENDING_ANALYSIS_REQUESTS} pending jobs)"
            )));
        }
        let insert_at = guard
            .pending
            .iter()
            .position(|existing| existing.cost_units > request.cost_units)
            .unwrap_or(guard.pending.len());
        let priority_promoted = insert_at < guard.pending.len();
        guard.pending.insert(insert_at, request);
        let depth = guard.pending.len();
        let weighted_depth = queue_pending_cost_units(&guard.pending) + guard.active_cost_units;
        condvar.notify_all();
        Ok((depth, weighted_depth, priority_promoted))
    }
}
