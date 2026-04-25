use serde_json::Value;

use crate::analysis_queue::{analysis_job_cost_units, AnalysisJobRequest, AnalysisWorkerQueue};
use crate::error::CodeLensError;
use crate::runtime_types::{
    AnalysisArtifact, AnalysisJob, AnalysisReadiness, AnalysisSummary, AnalysisVerifierCheck,
    JobLifecycle,
};

use super::AppState;

impl AppState {
    // ── Job Store delegations ────────────────────────────────────────────

    pub(crate) fn enqueue_analysis_job(
        &self,
        job_id: String,
        kind: String,
        arguments: Value,
        profile_hint: Option<String>,
    ) -> Result<(), CodeLensError> {
        let (depth, weighted_depth, priority_promoted) = self
            .analysis_queue
            .get_or_init(|| AnalysisWorkerQueue::new(self))
            .enqueue(AnalysisJobRequest {
                job_id,
                cost_units: analysis_job_cost_units(&kind),
                kind,
                arguments,
                profile_hint,
            })?;
        self.metrics
            .record_analysis_job_enqueued(depth, weighted_depth, priority_promoted);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store_analysis_job(
        &self,
        scope: &str,
        kind: &str,
        profile_hint: Option<String>,
        estimated_sections: Vec<String>,
        status: JobLifecycle,
        progress: u8,
        current_step: Option<String>,
        analysis_id: Option<String>,
        error: Option<String>,
    ) -> Result<AnalysisJob, CodeLensError> {
        self.job_store.store(
            kind,
            profile_hint,
            estimated_sections,
            status,
            progress,
            current_step,
            analysis_id,
            error,
            scope.to_owned(),
        )
    }

    // Test-only delegate: matches the param list of store_analysis_job so
    // refactoring to a struct would cascade into the production caller path.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store_analysis_job_for_current_scope(
        &self,
        kind: &str,
        profile_hint: Option<String>,
        estimated_sections: Vec<String>,
        status: JobLifecycle,
        progress: u8,
        current_step: Option<String>,
        analysis_id: Option<String>,
        error: Option<String>,
    ) -> Result<AnalysisJob, CodeLensError> {
        self.store_analysis_job(
            &self.current_project_scope(),
            kind,
            profile_hint,
            estimated_sections,
            status,
            progress,
            current_step,
            analysis_id,
            error,
        )
    }

    pub(crate) fn list_analysis_jobs_for_scope(
        &self,
        scope: &str,
        status_filter: Option<&str>,
    ) -> Vec<AnalysisJob> {
        self.job_store.list(status_filter, Some(scope))
    }

    pub(crate) fn get_analysis_job_for_scope(
        &self,
        scope: &str,
        job_id: &str,
    ) -> Option<AnalysisJob> {
        let job = self.job_store.get(job_id, Some(scope))?;
        // Cross-concern: warm artifact cache when job references an analysis
        if let Some(analysis_id) = job.analysis_id.as_deref() {
            let _ = self.get_analysis_for_scope(scope, analysis_id);
        }
        Some(job)
    }

    #[cfg(test)]
    pub(crate) fn get_analysis_job(&self, job_id: &str) -> Option<AnalysisJob> {
        self.get_analysis_job_for_scope(&self.current_project_scope(), job_id)
    }

    pub(crate) fn cancel_analysis_job_for_scope(
        &self,
        scope: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, CodeLensError> {
        let job = self.job_store.cancel(job_id, Some(scope))?;
        if job.status == JobLifecycle::Cancelled {
            self.metrics.record_analysis_job_cancelled(0, 0);
        }
        Ok(job)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_analysis_job(
        &self,
        scope: &str,
        job_id: &str,
        status: Option<JobLifecycle>,
        progress: Option<u8>,
        current_step: Option<Option<String>>,
        estimated_sections: Option<Vec<String>>,
        analysis_id: Option<Option<String>>,
        error: Option<Option<String>>,
    ) -> Result<AnalysisJob, CodeLensError> {
        self.job_store.update(
            job_id,
            status,
            progress,
            current_step,
            estimated_sections,
            analysis_id,
            error,
            Some(scope),
        )
    }

    // ── Artifact Store delegations ────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store_analysis(
        &self,
        scope: &str,
        tool_name: &str,
        cache_key: Option<String>,
        summary: String,
        top_findings: Vec<String>,
        risk_level: String,
        confidence: f64,
        next_actions: Vec<String>,
        blockers: Vec<String>,
        readiness: AnalysisReadiness,
        verifier_checks: Vec<AnalysisVerifierCheck>,
        sections: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<AnalysisArtifact, CodeLensError> {
        let artifact = self.artifact_store.store(
            tool_name,
            self.surface().as_label(),
            scope.to_owned(),
            cache_key,
            summary,
            top_findings,
            risk_level,
            confidence,
            next_actions,
            blockers,
            readiness,
            verifier_checks,
            sections,
        )?;
        // Cross-phase context: track recent analysis IDs so subsequent
        // tool calls can reference prior analysis results.
        self.push_recent_analysis_id(artifact.id.clone());
        Ok(artifact)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store_analysis_for_current_scope(
        &self,
        tool_name: &str,
        cache_key: Option<String>,
        summary: String,
        top_findings: Vec<String>,
        risk_level: String,
        confidence: f64,
        next_actions: Vec<String>,
        blockers: Vec<String>,
        readiness: AnalysisReadiness,
        verifier_checks: Vec<AnalysisVerifierCheck>,
        sections: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<AnalysisArtifact, CodeLensError> {
        self.store_analysis(
            &self.current_project_scope(),
            tool_name,
            cache_key,
            summary,
            top_findings,
            risk_level,
            confidence,
            next_actions,
            blockers,
            readiness,
            verifier_checks,
            sections,
        )
    }

    pub(crate) fn find_reusable_analysis(
        &self,
        scope: &str,
        tool_name: &str,
        cache_key: &str,
    ) -> Option<AnalysisArtifact> {
        self.artifact_store.find_reusable(
            tool_name,
            cache_key,
            self.surface().as_label(),
            Some(scope),
        )
    }

    pub(crate) fn find_reusable_analysis_for_current_scope(
        &self,
        tool_name: &str,
        cache_key: &str,
    ) -> Option<AnalysisArtifact> {
        self.find_reusable_analysis(&self.current_project_scope(), tool_name, cache_key)
    }

    pub(crate) fn get_analysis_for_scope(
        &self,
        scope: &str,
        analysis_id: &str,
    ) -> Option<AnalysisArtifact> {
        self.artifact_store.get(analysis_id, Some(scope))
    }

    pub(crate) fn get_analysis(&self, analysis_id: &str) -> Option<AnalysisArtifact> {
        self.get_analysis_for_scope(&self.current_project_scope(), analysis_id)
    }

    pub(crate) fn list_analysis_summaries_for_scope(&self, scope: &str) -> Vec<AnalysisSummary> {
        self.artifact_store.list_summaries(Some(scope))
    }

    pub(crate) fn list_analysis_summaries(&self) -> Vec<AnalysisSummary> {
        self.list_analysis_summaries_for_scope(&self.current_project_scope())
    }

    pub(crate) fn get_analysis_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.artifact_store.get_section(analysis_id, section)
    }

    pub(crate) fn peek_analysis_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.artifact_store.get_section(analysis_id, section)
    }

    #[cfg(test)]
    pub(crate) fn set_analysis_created_at_for_test(
        &self,
        analysis_id: &str,
        created_at_ms: u64,
    ) -> Result<(), CodeLensError> {
        self.artifact_store
            .set_created_at_for_test(analysis_id, created_at_ms)
            .map_err(|e| CodeLensError::Internal(e.into()))
    }
}
