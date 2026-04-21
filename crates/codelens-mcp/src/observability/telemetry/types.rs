use serde::Serialize;
use std::collections::{BTreeMap, HashMap, VecDeque};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub target_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CallTelemetryHints<'a> {
    pub suggested_next_tools: &'a [String],
    pub delegate_hint_trigger: Option<&'a str>,
    pub delegate_target_tool: Option<&'a str>,
    pub delegate_handoff_id: Option<&'a str>,
    pub handoff_id: Option<&'a str>,
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

#[derive(Debug, Default, Serialize, Clone)]
pub struct SessionMetrics {
    pub total_calls: u64,
    pub success_count: u64,
    pub total_ms: u64,
    pub total_tokens: usize,
    pub error_count: u64,
    pub retry_count: u64,
    pub truncated_response_count: u64,
    pub truncation_followup_count: u64,
    pub truncation_same_tool_retry_count: u64,
    pub truncation_handle_followup_count: u64,
    pub handle_reuse_count: u64,
    pub tools_list_tokens: usize,
    pub analysis_summary_reads: u64,
    pub analysis_section_reads: u64,
    pub analysis_cache_hit_count: u64,
    pub repeated_low_level_chain_count: u64,
    pub composite_guidance_emitted_count: u64,
    pub composite_guidance_followed_count: u64,
    pub composite_guidance_missed_count: u64,
    pub composite_guidance_missed_by_origin: BTreeMap<String, u64>,
    pub quality_contract_emitted_count: u64,
    pub recommended_checks_emitted_count: u64,
    pub recommended_check_followthrough_count: u64,
    pub quality_focus_reuse_count: u64,
    pub performance_watchpoint_emit_count: u64,
    pub verifier_contract_emitted_count: u64,
    pub blocker_emit_count: u64,
    pub verifier_followthrough_count: u64,
    pub coordination_registration_count: u64,
    pub coordination_claim_count: u64,
    pub coordination_release_count: u64,
    pub coordination_overlap_emit_count: u64,
    pub coordination_caution_emit_count: u64,
    pub mutation_preflight_checked_count: u64,
    pub mutation_without_preflight_count: u64,
    pub mutation_preflight_gate_denied_count: u64,
    pub stale_preflight_reject_count: u64,
    pub mutation_with_caution_count: u64,
    pub rename_without_symbol_preflight_count: u64,
    pub deferred_namespace_expansion_count: u64,
    pub deferred_hidden_tool_call_denied_count: u64,
    pub profile_switch_count: u64,
    pub preset_switch_count: u64,
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
    pub pending_truncation_tool: Option<String>,
    #[serde(skip_serializing)]
    pub pending_composite_guidance_from: Option<String>,
    #[serde(skip_serializing)]
    pub pending_quality_contract: bool,
    #[serde(skip_serializing)]
    pub pending_verifier_contract: bool,
    pub timeline: Vec<ToolInvocation>,
}

#[derive(Debug, Default, Clone)]
pub struct SessionTelemetryBucket {
    pub tools: HashMap<String, ToolMetrics>,
    pub surfaces: HashMap<String, SurfaceMetrics>,
    pub session: SessionMetrics,
}

pub const MAX_TIMELINE: usize = 200;
const MAX_LATENCY_SAMPLES: usize = 256;
const SESSION_RATE_LIMIT_WINDOW_MS: u64 = 60_000;

pub fn push_latency_sample(samples: &mut VecDeque<u64>, elapsed_ms: u64) {
    if samples.len() >= MAX_LATENCY_SAMPLES {
        samples.pop_front();
    }
    samples.push_back(elapsed_ms);
}

pub fn trim_rate_limit_window(samples: &mut VecDeque<u64>, now_ms: u64) {
    while let Some(oldest) = samples.front().copied() {
        if now_ms.saturating_sub(oldest) <= SESSION_RATE_LIMIT_WINDOW_MS {
            break;
        }
        samples.pop_front();
    }
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

pub fn is_workflow_tool(name: &str) -> bool {
    matches!(
        name,
        "tools/list"
            | "explore_codebase"
            | "trace_request_path"
            | "review_architecture"
            | "plan_safe_refactor"
            | "cleanup_duplicate_logic"
            | "review_changes"
            | "diagnose_issues"
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
            | "semantic_code_review"
            | "start_analysis_job"
            | "get_analysis_job"
            | "cancel_analysis_job"
    )
}

pub fn is_low_level_tool(name: &str) -> bool {
    !is_workflow_tool(name)
}

pub fn has_low_level_chain(timeline: &[ToolInvocation]) -> bool {
    if timeline.len() < 3 {
        return false;
    }
    let recent = &timeline[timeline.len() - 3..];
    recent.iter().all(|entry| is_low_level_tool(&entry.tool))
}
