//! Per-tool usage telemetry: call counts, latency, and error tracking.

use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

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
    /// Harness phase at invocation time (plan/build/review/eval).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Normalized target paths associated with the invocation, when available.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub target_paths: Vec<String>,
}

/// Safe, non-PII telemetry hints derived from the tool response.
///
/// These fields intentionally exclude tool arguments and payload excerpts.
/// They only record public tool names and synthetic delegate metadata so the
/// append-only JSONL log can support routing analysis without leaking user
/// query text.
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

/// Session-level aggregate metrics across all tool calls.
#[derive(Debug, Default, Serialize, Clone)]
pub struct SessionMetrics {
    // ── Core ─────────────────────────────────────────────────────────────
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

    // ── Token usage ──────────────────────────────────────────────────────
    pub tools_list_tokens: usize,

    // ── Analysis context reads ───────────────────────────────────────────
    pub analysis_summary_reads: u64,
    pub analysis_section_reads: u64,
    pub analysis_cache_hit_count: u64,

    // ── Workflow guidance ────────────────────────────────────────────────
    pub repeated_low_level_chain_count: u64,
    pub composite_guidance_emitted_count: u64,
    pub composite_guidance_followed_count: u64,
    pub composite_guidance_missed_count: u64,
    pub composite_guidance_missed_by_origin: BTreeMap<String, u64>,

    // ── Quality contract / verifier ──────────────────────────────────────
    pub quality_contract_emitted_count: u64,
    pub recommended_checks_emitted_count: u64,
    pub recommended_check_followthrough_count: u64,
    pub quality_focus_reuse_count: u64,
    pub performance_watchpoint_emit_count: u64,
    pub verifier_contract_emitted_count: u64,
    pub blocker_emit_count: u64,
    pub verifier_followthrough_count: u64,

    // ── Coordination ─────────────────────────────────────────────────────
    pub coordination_registration_count: u64,
    pub coordination_claim_count: u64,
    pub coordination_release_count: u64,
    pub coordination_overlap_emit_count: u64,
    pub coordination_caution_emit_count: u64,

    // ── Mutation gate / preflight ────────────────────────────────────────
    pub mutation_preflight_checked_count: u64,
    pub mutation_without_preflight_count: u64,
    pub mutation_preflight_gate_denied_count: u64,
    pub stale_preflight_reject_count: u64,
    pub mutation_with_caution_count: u64,
    pub rename_without_symbol_preflight_count: u64,

    // ── Namespace / surface tier ─────────────────────────────────────────
    pub deferred_namespace_expansion_count: u64,
    pub deferred_hidden_tool_call_denied_count: u64,
    pub profile_switch_count: u64,
    pub preset_switch_count: u64,

    // ── Call-type classification ─────────────────────────────────────────
    pub composite_calls: u64,
    pub low_level_calls: u64,

    // ── Transport ────────────────────────────────────────────────────────
    pub stdio_session_count: u64,
    pub http_session_count: u64,

    // ── Analysis job system ──────────────────────────────────────────────
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

    // ── Internal state (excluded from serialization) ─────────────────────
    #[serde(skip_serializing)]
    pub latency_samples: VecDeque<u64>,
    #[serde(skip_serializing)]
    pending_truncation_tool: Option<String>,
    #[serde(skip_serializing)]
    pending_composite_guidance_from: Option<String>,
    #[serde(skip_serializing)]
    pending_quality_contract: bool,
    #[serde(skip_serializing)]
    pending_verifier_contract: bool,
    /// Ordered tool invocation timeline (capped at 200 entries).
    pub timeline: Vec<ToolInvocation>,
}

mod registry;
mod writer;

pub use registry::ToolMetricsRegistry;
pub(crate) use registry::percentile_95;
#[cfg(test)]
pub(crate) use writer::{PersistedEvent, TelemetryWriter};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod writer_tests;
