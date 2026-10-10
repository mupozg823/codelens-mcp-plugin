use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;

use crate::AppState;
use crate::error::CodeLensError;

pub(crate) const MAX_PENDING_ANALYSIS_REQUESTS: usize = 32;
pub(crate) const STDIO_ANALYSIS_WORKER_COUNT: usize = 1;
pub(crate) const HTTP_ANALYSIS_WORKER_COUNT: usize = 2;
pub(crate) const STDIO_ANALYSIS_COST_BUDGET: usize = 2;
pub(crate) const HTTP_ANALYSIS_COST_BUDGET: usize = 3;

pub(crate) fn analysis_job_cost_units(kind: &str) -> usize {
    match kind {
        "impact_report" => 1,
        "orchestrate_change" => 1,
        "refactor_safety_report" => 2,
        "dead_code_report" => 3,
        // Whole-repo sweeps. The daemon log puts their slow-path averages at
        // 229 s and 202 s, above every other report, so they hold a worker for
        // far longer than a targeted analysis.
        "explore_codebase" => 4,
        "review_architecture" => 4,
        "index_embeddings" => 4,
        _ => 2,
    }
}

/// Whether a worker may start a job of `cost_units` now. A job priced above the
/// whole budget runs alone once the pool is idle; otherwise it would wait for
/// capacity that never exists.
fn admits(
    active_jobs: usize,
    allowed_parallelism: usize,
    active_cost_units: usize,
    cost_units: usize,
    cost_budget: usize,
) -> bool {
    active_jobs < allowed_parallelism
        && (active_cost_units + cost_units <= cost_budget || active_jobs == 0)
}

pub(crate) struct AnalysisJobRequest {
    pub(crate) job_id: String,
    pub(crate) project_scope: String,
    pub(crate) kind: String,
    pub(crate) arguments: Value,
    pub(crate) profile_hint: Option<String>,
    pub(crate) cost_units: usize,
}

struct AnalysisQueueState {
    pending: VecDeque<AnalysisJobRequest>,
    pending_cost_units: usize,
    active_jobs: usize,
    active_cost_units: usize,
}

pub(crate) struct JobService {
    inner: Arc<(Mutex<AnalysisQueueState>, Condvar)>,
}

impl JobService {
    pub(crate) fn new(state: &AppState) -> Self {
        let inner = Arc::new((
            Mutex::new(AnalysisQueueState {
                pending: VecDeque::new(),
                pending_cost_units: 0,
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
                                admits(
                                    guard.active_jobs,
                                    allowed_parallelism,
                                    guard.active_cost_units,
                                    request.cost_units,
                                    cost_budget,
                                )
                            });
                            if let Some(index) = next_index {
                                let request = guard.pending.remove(index);
                                guard.active_jobs += 1;
                                if let Some(request) = request.as_ref() {
                                    guard.pending_cost_units =
                                        guard.pending_cost_units.saturating_sub(request.cost_units);
                                    guard.active_cost_units += request.cost_units;
                                }
                                let remaining_depth = guard.pending.len();
                                let remaining_cost_units =
                                    guard.pending_cost_units + guard.active_cost_units;
                                break request.map(|request| {
                                    (request, remaining_depth, remaining_cost_units)
                                });
                            }
                            guard = condvar.wait(guard).unwrap_or_else(|p| p.into_inner());
                        }
                    };
                    if let Some((request, remaining_depth, remaining_cost_units)) = request {
                        let scope = request.project_scope;
                        if worker_state
                            .get_analysis_job_for_scope(&scope, &request.job_id)
                            .as_ref()
                            .map(|job| job.status)
                            == Some(crate::runtime_types::JobLifecycle::Cancelled)
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
                        let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
                            &worker_state,
                            request.job_id,
                            scope,
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
                                guard.pending_cost_units + guard.active_cost_units;
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
        let cost = request.cost_units;
        guard.pending.insert(insert_at, request);
        guard.pending_cost_units += cost;
        let depth = guard.pending.len();
        let weighted_depth = guard.pending_cost_units + guard.active_cost_units;
        condvar.notify_all();
        Ok((depth, weighted_depth, priority_promoted))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HTTP_ANALYSIS_COST_BUDGET, HTTP_ANALYSIS_WORKER_COUNT, STDIO_ANALYSIS_COST_BUDGET,
        STDIO_ANALYSIS_WORKER_COUNT, admits, analysis_job_cost_units,
    };

    #[test]
    fn every_job_kind_can_start_on_an_idle_queue() {
        // A job priced above the budget used to wait for capacity that can never
        // exist: `index_embeddings` (4) sat `queued` forever on the HTTP daemon
        // (budget 3), and so did the async `explore_codebase` and
        // `review_architecture`; on stdio (budget 2) `dead_code_report` too.
        for kind in [
            "impact_report",
            "orchestrate_change",
            "refactor_safety_report",
            "dead_code_report",
            "explore_codebase",
            "review_architecture",
            "index_embeddings",
            "any_other_report",
        ] {
            for (workers, budget) in [
                (HTTP_ANALYSIS_WORKER_COUNT, HTTP_ANALYSIS_COST_BUDGET),
                (STDIO_ANALYSIS_WORKER_COUNT, STDIO_ANALYSIS_COST_BUDGET),
            ] {
                assert!(
                    admits(0, workers, 0, analysis_job_cost_units(kind), budget),
                    "{kind} (cost {}) can never start under budget {budget}",
                    analysis_job_cost_units(kind)
                );
            }
        }
    }

    #[test]
    fn an_oversized_job_waits_until_it_can_run_alone() {
        let cost = analysis_job_cost_units("index_embeddings");
        assert!(cost > HTTP_ANALYSIS_COST_BUDGET);
        assert!(!admits(
            1,
            HTTP_ANALYSIS_WORKER_COUNT,
            1,
            cost,
            HTTP_ANALYSIS_COST_BUDGET
        ));
    }

    #[test]
    fn jobs_within_the_budget_still_share_the_pool() {
        let budget = HTTP_ANALYSIS_COST_BUDGET;
        assert!(admits(1, HTTP_ANALYSIS_WORKER_COUNT, 1, 2, budget));
        assert!(!admits(1, HTTP_ANALYSIS_WORKER_COUNT, 2, 2, budget));
        assert!(!admits(
            HTTP_ANALYSIS_WORKER_COUNT,
            HTTP_ANALYSIS_WORKER_COUNT,
            0,
            1,
            budget
        ));
    }

    #[test]
    fn whole_repo_sweeps_reserve_more_capacity_than_targeted_reports() {
        // `explore_codebase` and `review_architecture` average 229 s and 202 s in
        // the daemon's slow-execution log — far longer than any targeted report —
        // so they must not be priced like one, or a pair of them starves the pool.
        assert_eq!(analysis_job_cost_units("explore_codebase"), 4);
        assert_eq!(analysis_job_cost_units("review_architecture"), 4);
        assert!(
            analysis_job_cost_units("explore_codebase") > analysis_job_cost_units("impact_report")
        );
        assert!(
            analysis_job_cost_units("review_architecture")
                > analysis_job_cost_units("dead_code_report")
        );
    }
}
