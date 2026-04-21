use super::{SessionMetrics, ToolMetricsRegistry};

impl ToolMetricsRegistry {
    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_analysis_read(&self, is_section: bool) {
        self.record_analysis_read_for_session(is_section, None);
    }

    pub fn record_analysis_read_for_session(
        &self,
        is_section: bool,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.handle_reuse_count += 1;
            session.quality_focus_reuse_count += 1;
            if session.pending_composite_guidance_from.take().is_some() {
                session.composite_guidance_followed_count += 1;
            }
            if session.pending_truncation_tool.take().is_some() {
                session.truncation_followup_count += 1;
                session.truncation_handle_followup_count += 1;
            }
            if session.pending_quality_contract {
                session.recommended_check_followthrough_count += 1;
                session.pending_quality_contract = false;
            }
            if session.pending_verifier_contract {
                session.verifier_followthrough_count += 1;
                session.pending_verifier_contract = false;
            }
            if is_section {
                session.analysis_section_reads += 1;
            } else {
                session.analysis_summary_reads += 1;
            }
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_analysis_cache_hit(&self) {
        self.record_analysis_cache_hit_for_session(None);
    }

    pub fn record_analysis_cache_hit_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.analysis_cache_hit_count += 1;
            session.handle_reuse_count += 1;
            session.quality_focus_reuse_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_quality_contract_emitted(
        &self,
        quality_focus_count: usize,
        recommended_checks_count: usize,
        performance_watchpoint_count: usize,
    ) {
        self.record_quality_contract_emitted_for_session(
            quality_focus_count,
            recommended_checks_count,
            performance_watchpoint_count,
            None,
        );
    }

    pub fn record_quality_contract_emitted_for_session(
        &self,
        quality_focus_count: usize,
        recommended_checks_count: usize,
        performance_watchpoint_count: usize,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.quality_contract_emitted_count += 1;
            session.recommended_checks_emitted_count += recommended_checks_count as u64;
            session.performance_watchpoint_emit_count += performance_watchpoint_count as u64;
            session.pending_quality_contract = recommended_checks_count > 0;
            if quality_focus_count == 0 {
                session.pending_quality_contract = false;
            }
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_verifier_contract_emitted(
        &self,
        blocker_count: usize,
        verifier_check_count: usize,
    ) {
        self.record_verifier_contract_emitted_for_session(
            blocker_count,
            verifier_check_count,
            None,
        );
    }

    pub fn record_verifier_contract_emitted_for_session(
        &self,
        blocker_count: usize,
        verifier_check_count: usize,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.verifier_contract_emitted_count += 1;
            if blocker_count > 0 {
                session.blocker_emit_count += 1;
            }
            session.pending_verifier_contract = verifier_check_count > 0;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_coordination_overlap_emitted(&self, caution_only: bool) {
        self.record_coordination_overlap_emitted_for_session(caution_only, None);
    }

    pub fn record_coordination_overlap_emitted_for_session(
        &self,
        caution_only: bool,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.coordination_overlap_emit_count += 1;
            if caution_only {
                session.coordination_caution_emit_count += 1;
            }
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_coordination_registration(&self) {
        self.record_coordination_registration_for_session(None);
    }

    pub fn record_coordination_registration_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.coordination_registration_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_coordination_claim(&self) {
        self.record_coordination_claim_for_session(None);
    }

    pub fn record_coordination_claim_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.coordination_claim_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_coordination_release(&self) {
        self.record_coordination_release_for_session(None);
    }

    pub fn record_coordination_release_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.coordination_release_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_mutation_without_preflight(&self) {
        self.record_mutation_without_preflight_for_session(None);
    }

    pub fn record_mutation_without_preflight_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.mutation_without_preflight_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_mutation_preflight_checked(&self) {
        self.record_mutation_preflight_checked_for_session(None);
    }

    pub fn record_mutation_preflight_checked_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.mutation_preflight_checked_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_mutation_preflight_gate_denied(&self, stale: bool) {
        self.record_mutation_preflight_gate_denied_for_session(stale, None);
    }

    pub fn record_mutation_preflight_gate_denied_for_session(
        &self,
        stale: bool,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.mutation_preflight_gate_denied_count += 1;
            if stale {
                session.stale_preflight_reject_count += 1;
            }
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_mutation_with_caution(&self) {
        self.record_mutation_with_caution_for_session(None);
    }

    pub fn record_mutation_with_caution_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.mutation_with_caution_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_rename_without_symbol_preflight(&self) {
        self.record_rename_without_symbol_preflight_for_session(None);
    }

    pub fn record_rename_without_symbol_preflight_for_session(
        &self,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.rename_without_symbol_preflight_count += 1;
        });
    }

    pub fn record_deferred_namespace_expansion(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.deferred_namespace_expansion_count += 1;
    }

    pub fn record_deferred_hidden_tool_call_denied(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.deferred_hidden_tool_call_denied_count += 1;
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_composite_guidance_emitted(&self, origin_tool: &str) {
        self.record_composite_guidance_emitted_for_session(origin_tool, None);
    }

    pub fn record_composite_guidance_emitted_for_session(
        &self,
        origin_tool: &str,
        logical_session_id: Option<&str>,
    ) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.composite_guidance_emitted_count += 1;
            session.pending_composite_guidance_from = Some(origin_tool.to_owned());
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_profile_switch(&self) {
        self.record_profile_switch_for_session(None);
    }

    pub fn record_profile_switch_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.profile_switch_count += 1;
        });
    }

    #[allow(dead_code)] // compatibility wrapper; session-aware callers use *_for_session
    pub fn record_preset_switch(&self) {
        self.record_preset_switch_for_session(None);
    }

    pub fn record_preset_switch_for_session(&self, logical_session_id: Option<&str>) {
        self.mutate_session_metrics(logical_session_id, |session| {
            session.preset_switch_count += 1;
        });
    }

    pub fn record_transport_session(&self, transport: &str) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match transport {
            "http" => session.http_session_count += 1,
            _ => session.stdio_session_count += 1,
        }
    }

    pub fn record_analysis_worker_pool(
        &self,
        worker_limit: usize,
        cost_budget: usize,
        transport: &str,
    ) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.analysis_worker_limit = worker_limit as u64;
        session.analysis_cost_budget = cost_budget as u64;
        session.analysis_transport_mode = transport.to_owned();
    }

    pub fn record_analysis_job_enqueued(
        &self,
        queue_depth: usize,
        weighted_depth: usize,
        priority_promoted: bool,
    ) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.analysis_jobs_enqueued += 1;
        session.analysis_queue_depth = queue_depth as u64;
        session.analysis_queue_max_depth = session.analysis_queue_max_depth.max(queue_depth as u64);
        session.analysis_queue_weighted_depth = weighted_depth as u64;
        session.analysis_queue_max_weighted_depth = session
            .analysis_queue_max_weighted_depth
            .max(weighted_depth as u64);
        if priority_promoted {
            session.analysis_queue_priority_promotions += 1;
        }
    }

    pub fn record_analysis_job_started(&self, queue_depth: usize, weighted_depth: usize) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.analysis_jobs_started += 1;
        session.analysis_queue_depth = queue_depth as u64;
        session.analysis_queue_weighted_depth = weighted_depth as u64;
        session.analysis_queue_max_weighted_depth = session
            .analysis_queue_max_weighted_depth
            .max(weighted_depth as u64);
        session.active_analysis_workers += 1;
        session.peak_active_analysis_workers = session
            .peak_active_analysis_workers
            .max(session.active_analysis_workers);
    }

    pub fn record_analysis_job_finished(
        &self,
        status: crate::runtime_types::JobLifecycle,
        queue_depth: usize,
        weighted_depth: usize,
    ) {
        use crate::runtime_types::JobLifecycle;

        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match status {
            JobLifecycle::Completed => session.analysis_jobs_completed += 1,
            JobLifecycle::Cancelled => session.analysis_jobs_cancelled += 1,
            _ => session.analysis_jobs_failed += 1,
        }
        session.analysis_queue_depth = queue_depth as u64;
        session.analysis_queue_max_depth = session.analysis_queue_max_depth.max(queue_depth as u64);
        session.analysis_queue_weighted_depth = weighted_depth as u64;
        session.analysis_queue_max_weighted_depth = session
            .analysis_queue_max_weighted_depth
            .max(weighted_depth as u64);
        session.active_analysis_workers = session.active_analysis_workers.saturating_sub(1);
    }

    pub fn record_analysis_job_cancelled(&self, queue_depth: usize, weighted_depth: usize) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.analysis_jobs_cancelled += 1;
        session.analysis_queue_depth = queue_depth as u64;
        session.analysis_queue_max_depth = session.analysis_queue_max_depth.max(queue_depth as u64);
        session.analysis_queue_weighted_depth = weighted_depth as u64;
        session.analysis_queue_max_weighted_depth = session
            .analysis_queue_max_weighted_depth
            .max(weighted_depth as u64);
    }

    /// Clear all recorded metrics.
    #[allow(dead_code)] // used in tests
    pub fn reset(&self) {
        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.clear();

        let mut surfaces = self
            .surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        surfaces.clear();

        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *session = SessionMetrics::default();

        let mut session_windows = self
            .session_windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session_windows.clear();

        let mut session_buckets = self
            .session_buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session_buckets.clear();
    }

    pub(super) fn mutate_session_metrics<F>(&self, logical_session_id: Option<&str>, mut f: F)
    where
        F: FnMut(&mut SessionMetrics),
    {
        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&mut session);
        }
        if let Some(session_id) = logical_session_id {
            let mut buckets = self
                .session_buckets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let bucket = buckets.entry(session_id.to_owned()).or_default();
            f(&mut bucket.session);
        }
    }
}
