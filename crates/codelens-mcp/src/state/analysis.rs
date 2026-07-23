use serde_json::Value;

use crate::analysis_queue::{AnalysisJobRequest, JobService, analysis_job_cost_units};
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
        project_scope: String,
        job_id: String,
        kind: String,
        arguments: Value,
        profile_hint: Option<String>,
    ) -> Result<(), CodeLensError> {
        let job_id_for_failure = job_id.clone();
        let scope_for_failure = project_scope.clone();
        let queued = self
            .job_service
            .get_or_init(|| JobService::new(self))
            .enqueue(AnalysisJobRequest {
                job_id,
                project_scope,
                cost_units: analysis_job_cost_units(&kind),
                kind,
                arguments,
                profile_hint,
            });
        match queued {
            Ok((depth, weighted_depth, priority_promoted)) => {
                self.metrics
                    .record_analysis_job_enqueued(depth, weighted_depth, priority_promoted);
                Ok(())
            }
            Err(error) => {
                let _ = self.update_analysis_job(
                    &scope_for_failure,
                    &job_id_for_failure,
                    Some(JobLifecycle::Error),
                    Some(100),
                    Some(Some("enqueue failed".to_owned())),
                    None,
                    None,
                    Some(Some(error.to_string())),
                );
                Err(error)
            }
        }
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
        let analysis_dir = self.analysis_dir();
        let artifact = self.artifact_store.store_in_dir(
            &analysis_dir,
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

        // Phase 1: semantic artifact memory — embed summary + findings
        #[cfg(feature = "semantic")]
        {
            if let Ok(guard) = self.embedding.read()
                && let Some(engine) = guard.as_ref()
            {
                let text = format!("{} {}", artifact.summary, artifact.top_findings.join(" "));
                if let Ok(embedding) = engine.embed_text(&text) {
                    let chunk = codelens_engine::embedding_store::ArtifactEmbeddingChunk {
                        analysis_id: artifact.id.clone(),
                        tool_name: artifact.tool_name.clone(),
                        surface: artifact.surface.clone(),
                        project_scope: artifact.project_scope.clone(),
                        summary: artifact.summary.clone(),
                        top_findings: artifact.top_findings.clone(),
                        risk_level: artifact.risk_level.clone(),
                        embedding,
                    };
                    let _ = engine.store_artifact_embeddings(&[chunk]);
                }
            }
        }

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

    /// Tiered cache lookup — returns artifact + hit tier for metrics.
    pub(crate) fn find_reusable_analysis_tiered(
        &self,
        scope: &str,
        tool_name: &str,
        cache_key: &str,
    ) -> Option<(AnalysisArtifact, crate::runtime_types::CacheHitTier)> {
        let analysis_dir = self.analysis_dir();
        self.artifact_store.find_reusable_tiered_in_dir(
            &analysis_dir,
            tool_name,
            cache_key,
            self.surface().as_label(),
            Some(scope),
        )
    }

    /// Tiered cache lookup for current scope.
    pub(crate) fn find_reusable_analysis_tiered_for_current_scope(
        &self,
        tool_name: &str,
        cache_key: &str,
    ) -> Option<(AnalysisArtifact, crate::runtime_types::CacheHitTier)> {
        self.find_reusable_analysis_tiered(&self.current_project_scope(), tool_name, cache_key)
    }

    pub(crate) fn get_analysis_for_scope(
        &self,
        scope: &str,
        analysis_id: &str,
    ) -> Option<AnalysisArtifact> {
        let analysis_dir = self.analysis_dir();
        self.artifact_store
            .get_in_dir(&analysis_dir, analysis_id, scope)
    }

    pub(crate) fn get_analysis(&self, analysis_id: &str) -> Option<AnalysisArtifact> {
        self.get_analysis_for_scope(&self.current_project_scope(), analysis_id)
    }

    pub(crate) fn list_analysis_summaries_for_scope(&self, scope: &str) -> Vec<AnalysisSummary> {
        let analysis_dir = self.analysis_dir();
        self.artifact_store
            .list_summaries_in_dir(&analysis_dir, Some(scope))
    }

    pub(crate) fn list_analysis_summaries(&self) -> Vec<AnalysisSummary> {
        self.list_analysis_summaries_for_scope(&self.current_project_scope())
    }

    pub(crate) fn get_analysis_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.get_analysis_section_for_scope(&self.current_project_scope(), analysis_id, section)
    }

    pub(crate) fn get_analysis_section_for_scope(
        &self,
        scope: &str,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        let analysis_dir = self.analysis_dir();
        self.artifact_store
            .get_section_in_dir(&analysis_dir, analysis_id, section, scope)
    }

    pub(crate) fn peek_analysis_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.get_analysis_section(analysis_id, section)
    }

    pub(crate) fn upsert_analysis_section_for_scope(
        &self,
        scope: &str,
        analysis_id: &str,
        section: &str,
        value: &serde_json::Value,
    ) -> Result<(), CodeLensError> {
        self.get_analysis_for_scope(scope, analysis_id)
            .ok_or_else(|| {
                CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`"))
            })?;
        let analysis_dir = self.analysis_dir();
        self.artifact_store
            .upsert_section_in_dir(&analysis_dir, analysis_id, section, value, scope)
    }

    #[cfg(test)]
    pub(crate) fn set_analysis_created_at_for_test(
        &self,
        analysis_id: &str,
        created_at_ms: u64,
    ) -> Result<(), CodeLensError> {
        let analysis_dir = self.analysis_dir();
        self.artifact_store
            .set_created_at_for_test_in_dir(&analysis_dir, analysis_id, created_at_ms)
            .map_err(|e| CodeLensError::Internal(e.into()))
    }
}
