//! Per-tool usage telemetry: call counts, latency, and error tracking.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metrics for a single tool.
#[derive(Debug, Default, Serialize, Clone)]
pub struct ToolMetrics {
    pub call_count: u64,
    pub success_count: u64,
    pub total_ms: u64,
    pub max_ms: u64,
    pub total_tokens: usize,
    pub error_count: u64,
    #[serde(skip_serializing)]
    pub latency_samples: VecDeque<u64>,
    /// Last invocation timestamp (unix epoch milliseconds).
    pub last_called_at: u64,
}

/// A single tool invocation in the session timeline.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInvocation {
    pub tool: String,
    pub surface: String,
    pub elapsed_ms: u64,
    pub tokens: usize,
    pub success: bool,
    pub truncated: bool,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct SurfaceMetrics {
    pub call_count: u64,
    pub success_count: u64,
    pub total_ms: u64,
    pub total_tokens: usize,
    pub error_count: u64,
    #[serde(skip_serializing)]
    pub latency_samples: VecDeque<u64>,
    pub last_called_at: u64,
}

/// Session-level aggregate metrics across all tool calls.
#[derive(Debug, Default, Serialize, Clone)]
pub struct SessionMetrics {
    pub total_calls: u64,
    pub success_count: u64,
    pub total_ms: u64,
    pub total_tokens: usize,
    pub error_count: u64,
    pub tools_list_tokens: usize,
    pub analysis_summary_reads: u64,
    pub analysis_section_reads: u64,
    pub retry_count: u64,
    pub analysis_cache_hit_count: u64,
    pub truncated_response_count: u64,
    pub truncation_followup_count: u64,
    pub truncation_same_tool_retry_count: u64,
    pub truncation_handle_followup_count: u64,
    pub handle_reuse_count: u64,
    pub repeated_low_level_chain_count: u64,
    pub composite_guidance_emitted_count: u64,
    pub composite_guidance_followed_count: u64,
    pub quality_contract_emitted_count: u64,
    pub recommended_checks_emitted_count: u64,
    pub recommended_check_followthrough_count: u64,
    pub quality_focus_reuse_count: u64,
    pub performance_watchpoint_emit_count: u64,
    pub verifier_contract_emitted_count: u64,
    pub blocker_emit_count: u64,
    pub verifier_followthrough_count: u64,
    pub mutation_preflight_checked_count: u64,
    pub mutation_without_preflight_count: u64,
    pub mutation_preflight_gate_denied_count: u64,
    pub stale_preflight_reject_count: u64,
    pub mutation_with_caution_count: u64,
    pub rename_without_symbol_preflight_count: u64,
    pub deferred_namespace_expansion_count: u64,
    pub deferred_hidden_tool_call_denied_count: u64,
    pub composite_calls: u64,
    pub low_level_calls: u64,
    pub stdio_session_count: u64,
    pub http_session_count: u64,
    pub analysis_jobs_enqueued: u64,
    pub analysis_jobs_started: u64,
    pub analysis_jobs_completed: u64,
    pub analysis_jobs_failed: u64,
    pub analysis_jobs_cancelled: u64,
    pub analysis_queue_depth: u64,
    pub analysis_queue_max_depth: u64,
    pub analysis_queue_weighted_depth: u64,
    pub analysis_queue_max_weighted_depth: u64,
    pub analysis_queue_priority_promotions: u64,
    pub active_analysis_workers: u64,
    pub peak_active_analysis_workers: u64,
    pub analysis_worker_limit: u64,
    pub analysis_cost_budget: u64,
    pub analysis_transport_mode: String,
    #[serde(skip_serializing)]
    pub latency_samples: VecDeque<u64>,
    #[serde(skip_serializing)]
    pending_truncation_tool: Option<String>,
    #[serde(skip_serializing)]
    pending_composite_guidance: bool,
    #[serde(skip_serializing)]
    pending_quality_contract: bool,
    #[serde(skip_serializing)]
    pending_verifier_contract: bool,
    /// Ordered tool invocation timeline (capped at 200 entries).
    pub timeline: Vec<ToolInvocation>,
}

const MAX_TIMELINE: usize = 200;
const MAX_LATENCY_SAMPLES: usize = 256;

fn push_latency_sample(samples: &mut VecDeque<u64>, elapsed_ms: u64) {
    if samples.len() >= MAX_LATENCY_SAMPLES {
        samples.pop_front();
    }
    samples.push_back(elapsed_ms);
}

pub(crate) fn percentile_95(samples: &VecDeque<u64>) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut values = samples.iter().copied().collect::<Vec<_>>();
    values.sort_unstable();
    let index = ((values.len() - 1) * 95) / 100;
    values[index]
}

fn is_workflow_tool(name: &str) -> bool {
    matches!(
        name,
        "tools/list"
            | "onboard_project"
            | "analyze_change_request"
            | "verify_change_readiness"
            | "find_minimal_context_for_change"
            | "summarize_symbol_impact"
            | "module_boundary_report"
            | "safe_rename_report"
            | "unresolved_reference_check"
            | "dead_code_report"
            | "impact_report"
            | "refactor_safety_report"
            | "diff_aware_references"
            | "start_analysis_job"
            | "get_analysis_job"
            | "cancel_analysis_job"
    )
}

fn is_low_level_tool(name: &str) -> bool {
    !is_workflow_tool(name)
}

fn has_low_level_chain(timeline: &[ToolInvocation]) -> bool {
    if timeline.len() < 3 {
        return false;
    }
    let recent = &timeline[timeline.len() - 3..];
    recent.iter().all(|entry| is_low_level_tool(&entry.tool))
}

/// Thread-safe registry that accumulates per-tool and session-level metrics.
pub struct ToolMetricsRegistry {
    inner: Mutex<HashMap<String, ToolMetrics>>,
    surfaces: Mutex<HashMap<String, SurfaceMetrics>>,
    session: Mutex<SessionMetrics>,
}

impl ToolMetricsRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            surfaces: Mutex::new(HashMap::new()),
            session: Mutex::new(SessionMetrics::default()),
        }
    }

    /// Record a single tool invocation (per-tool + session).
    #[allow(dead_code)] // used in tests and as convenience wrapper
    pub fn record_call(&self, name: &str, elapsed_ms: u64, success: bool) {
        self.record_call_with_tokens(name, elapsed_ms, success, 0, "unknown", false);
    }

    /// Record a tool invocation with token estimate.
    pub fn record_call_with_tokens(
        &self,
        name: &str,
        elapsed_ms: u64,
        success: bool,
        tokens: usize,
        surface: &str,
        truncated: bool,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Per-tool metrics
        {
            let mut map = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let entry = map.entry(name.to_owned()).or_default();
            entry.call_count += 1;
            if success {
                entry.success_count += 1;
            }
            entry.total_ms += elapsed_ms;
            entry.total_tokens += tokens;
            if elapsed_ms > entry.max_ms {
                entry.max_ms = elapsed_ms;
            }
            push_latency_sample(&mut entry.latency_samples, elapsed_ms);
            if !success {
                entry.error_count += 1;
            }
            entry.last_called_at = now;
        }

        // Session-level metrics
        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let previous = session.timeline.last().cloned();
            session.total_calls += 1;
            if success {
                session.success_count += 1;
            }
            session.total_ms += elapsed_ms;
            session.total_tokens += tokens;
            if name == "tools/list" {
                session.tools_list_tokens += tokens;
            }
            if is_workflow_tool(name) {
                session.composite_calls += 1;
            } else {
                session.low_level_calls += 1;
            }
            if !success {
                session.error_count += 1;
            }
            if name != "get_tool_metrics" && session.pending_composite_guidance {
                if is_workflow_tool(name) {
                    session.composite_guidance_followed_count += 1;
                }
                session.pending_composite_guidance = false;
            }
            if name != "get_tool_metrics"
                && session.pending_quality_contract
                && (name == "get_analysis_section"
                    || name == "get_file_diagnostics"
                    || name == "find_tests"
                    || name == "get_tool_metrics")
            {
                session.recommended_check_followthrough_count += 1;
                session.pending_quality_contract = false;
            }
            if name != "get_tool_metrics"
                && session.pending_verifier_contract
                && (name == "get_analysis_section"
                    || name == "get_file_diagnostics"
                    || name == "find_tests"
                    || name == "safe_rename_report"
                    || name == "verify_change_readiness"
                    || name == "unresolved_reference_check"
                    || crate::tool_defs::is_content_mutation_tool(name))
            {
                session.verifier_followthrough_count += 1;
                session.pending_verifier_contract = false;
            }
            if let Some(prev) = previous {
                if prev.tool == name && !prev.success {
                    session.retry_count += 1;
                }
            }
            if name != "get_tool_metrics" {
                if let Some(prev_tool) = session.pending_truncation_tool.take() {
                    session.truncation_followup_count += 1;
                    if prev_tool == name {
                        session.truncation_same_tool_retry_count += 1;
                    }
                }
            }
            push_latency_sample(&mut session.latency_samples, elapsed_ms);
            if session.timeline.len() < MAX_TIMELINE {
                session.timeline.push(ToolInvocation {
                    tool: name.to_owned(),
                    surface: surface.to_owned(),
                    elapsed_ms,
                    tokens,
                    success,
                    truncated,
                });
            } else {
                session.timeline.remove(0);
                session.timeline.push(ToolInvocation {
                    tool: name.to_owned(),
                    surface: surface.to_owned(),
                    elapsed_ms,
                    tokens,
                    success,
                    truncated,
                });
            }
            if truncated {
                session.truncated_response_count += 1;
                session.pending_truncation_tool = Some(name.to_owned());
            }
            if has_low_level_chain(&session.timeline) {
                session.repeated_low_level_chain_count += 1;
            }
        }

        {
            let mut surfaces = self
                .surfaces
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let entry = surfaces.entry(surface.to_owned()).or_default();
            entry.call_count += 1;
            if success {
                entry.success_count += 1;
            }
            entry.total_ms += elapsed_ms;
            entry.total_tokens += tokens;
            push_latency_sample(&mut entry.latency_samples, elapsed_ms);
            if !success {
                entry.error_count += 1;
            }
            entry.last_called_at = now;
        }
    }

    /// Return a snapshot of all per-tool metrics, sorted by call_count descending.
    pub fn snapshot(&self) -> Vec<(String, ToolMetrics)> {
        let map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut entries: Vec<(String, ToolMetrics)> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        entries.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
        entries
    }

    /// Return a snapshot of session-level metrics.
    pub fn session_snapshot(&self) -> SessionMetrics {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn surface_snapshot(&self) -> Vec<(String, SurfaceMetrics)> {
        let map = self
            .surfaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries: Vec<(String, SurfaceMetrics)> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
        entries
    }

    pub fn record_analysis_read(&self, is_section: bool) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.handle_reuse_count += 1;
        session.quality_focus_reuse_count += 1;
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
    }

    pub fn record_analysis_cache_hit(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.analysis_cache_hit_count += 1;
        session.handle_reuse_count += 1;
        session.quality_focus_reuse_count += 1;
    }

    pub fn record_quality_contract_emitted(
        &self,
        quality_focus_count: usize,
        recommended_checks_count: usize,
        performance_watchpoint_count: usize,
    ) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.quality_contract_emitted_count += 1;
        session.recommended_checks_emitted_count += recommended_checks_count as u64;
        session.performance_watchpoint_emit_count += performance_watchpoint_count as u64;
        session.pending_quality_contract = recommended_checks_count > 0;
        if quality_focus_count == 0 {
            session.pending_quality_contract = false;
        }
    }

    pub fn record_verifier_contract_emitted(
        &self,
        blocker_count: usize,
        verifier_check_count: usize,
    ) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.verifier_contract_emitted_count += 1;
        if blocker_count > 0 {
            session.blocker_emit_count += 1;
        }
        session.pending_verifier_contract = verifier_check_count > 0;
    }

    pub fn record_mutation_without_preflight(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.mutation_without_preflight_count += 1;
    }

    pub fn record_mutation_preflight_checked(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.mutation_preflight_checked_count += 1;
    }

    pub fn record_mutation_preflight_gate_denied(&self, stale: bool) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.mutation_preflight_gate_denied_count += 1;
        if stale {
            session.stale_preflight_reject_count += 1;
        }
    }

    pub fn record_mutation_with_caution(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.mutation_with_caution_count += 1;
    }

    pub fn record_rename_without_symbol_preflight(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.rename_without_symbol_preflight_count += 1;
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

    pub fn record_composite_guidance_emitted(&self) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        session.composite_guidance_emitted_count += 1;
        session.pending_composite_guidance = true;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_snapshot() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("find_symbol", 42, true);
        reg.record_call("find_symbol", 58, true);

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 1);

        let (name, m) = &snap[0];
        assert_eq!(name, "find_symbol");
        assert_eq!(m.call_count, 2);
        assert_eq!(m.total_ms, 100);
        assert_eq!(m.max_ms, 58);
        assert_eq!(m.error_count, 0);
        assert!(m.last_called_at > 0);
    }

    #[test]
    fn multiple_tools_independent() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("find_symbol", 10, true);
        reg.record_call("get_ranked_context", 20, true);

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 2);

        let names: Vec<&str> = snap.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"find_symbol"));
        assert!(names.contains(&"get_ranked_context"));
    }

    #[test]
    fn error_count_tracked() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("bad_tool", 5, false);
        reg.record_call("bad_tool", 3, true);
        reg.record_call("bad_tool", 7, false);

        let snap = reg.snapshot();
        let (_, m) = &snap[0];
        assert_eq!(m.call_count, 3);
        assert_eq!(m.error_count, 2);
    }

    #[test]
    fn reset_clears_all() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("a", 1, true);
        reg.record_call("b", 2, true);
        assert_eq!(reg.snapshot().len(), 2);

        reg.reset();
        assert!(reg.snapshot().is_empty());
    }

    #[test]
    fn session_metrics_accumulate() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call_with_tokens("find_symbol", 15, true, 500, "planner-readonly", false);
        reg.record_call_with_tokens(
            "get_ranked_context",
            42,
            true,
            2000,
            "planner-readonly",
            false,
        );
        reg.record_call_with_tokens("rename_symbol", 8, false, 0, "refactor-full", false);

        let session = reg.session_snapshot();
        assert_eq!(session.total_calls, 3);
        assert_eq!(session.total_ms, 65);
        assert_eq!(session.total_tokens, 2500);
        assert_eq!(session.error_count, 1);
        assert_eq!(session.timeline.len(), 3);
        assert_eq!(session.timeline[0].tool, "find_symbol");
        assert_eq!(session.timeline[0].surface, "planner-readonly");
        assert_eq!(session.timeline[1].tokens, 2000);
        assert!(!session.timeline[2].success);

        let surfaces = reg.surface_snapshot();
        assert_eq!(surfaces.len(), 2);
        assert_eq!(session.low_level_calls, 3);
    }

    #[test]
    fn transport_counts_accumulate() {
        let reg = ToolMetricsRegistry::new();
        reg.record_transport_session("stdio");
        reg.record_transport_session("http");
        reg.record_transport_session("http");

        let session = reg.session_snapshot();
        assert_eq!(session.stdio_session_count, 1);
        assert_eq!(session.http_session_count, 2);
    }

    #[test]
    fn analysis_queue_metrics_accumulate() {
        let reg = ToolMetricsRegistry::new();
        reg.record_analysis_worker_pool(2, 3, "http");
        reg.record_analysis_job_enqueued(2, 4, true);
        reg.record_analysis_job_started(1, 3);
        reg.record_analysis_job_finished(crate::runtime_types::JobLifecycle::Completed, 0, 0);
        reg.record_analysis_job_cancelled(0, 0);

        let session = reg.session_snapshot();
        assert_eq!(session.analysis_jobs_enqueued, 1);
        assert_eq!(session.analysis_jobs_started, 1);
        assert_eq!(session.analysis_jobs_completed, 1);
        assert_eq!(session.analysis_jobs_cancelled, 1);
        assert_eq!(session.analysis_queue_max_depth, 2);
        assert_eq!(session.analysis_queue_max_weighted_depth, 4);
        assert_eq!(session.analysis_queue_priority_promotions, 1);
        assert_eq!(session.analysis_queue_depth, 0);
        assert_eq!(session.active_analysis_workers, 0);
        assert_eq!(session.peak_active_analysis_workers, 1);
        assert_eq!(session.analysis_cost_budget, 3);
    }

    #[test]
    fn session_reset_clears() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call_with_tokens("a", 10, true, 100, "planner-readonly", false);
        assert_eq!(reg.session_snapshot().total_calls, 1);

        reg.reset();
        let session = reg.session_snapshot();
        assert_eq!(session.total_calls, 0);
        assert_eq!(session.total_tokens, 0);
        assert!(session.timeline.is_empty());
    }

    #[test]
    fn truncation_metrics_capture_followup() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call_with_tokens(
            "analyze_change_request",
            20,
            true,
            1200,
            "planner-readonly",
            true,
        );
        reg.record_call_with_tokens(
            "analyze_change_request",
            18,
            true,
            800,
            "planner-readonly",
            false,
        );
        reg.record_call_with_tokens("impact_report", 10, true, 500, "reviewer-graph", true);
        reg.record_analysis_read(true);

        let session = reg.session_snapshot();
        assert_eq!(session.truncated_response_count, 2);
        assert_eq!(session.truncation_followup_count, 2);
        assert_eq!(session.truncation_same_tool_retry_count, 1);
        assert_eq!(session.truncation_handle_followup_count, 1);
    }

    #[test]
    fn snapshot_sorted_by_call_count() {
        let reg = ToolMetricsRegistry::new();
        reg.record_call("low", 1, true);
        reg.record_call("high", 1, true);
        reg.record_call("high", 1, true);
        reg.record_call("high", 1, true);
        reg.record_call("mid", 1, true);
        reg.record_call("mid", 1, true);

        let snap = reg.snapshot();
        let counts: Vec<u64> = snap.iter().map(|(_, m)| m.call_count).collect();
        assert_eq!(counts, vec![3, 2, 1]);
        assert_eq!(snap[0].0, "high");
        assert_eq!(snap[1].0, "mid");
        assert_eq!(snap[2].0, "low");
    }
}
