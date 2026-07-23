use crate::runtime_types::WatcherFailureHealth;
use crate::telemetry::{SessionMetrics, ToolInvocation};
use codelens_engine::WatcherStats;
use serde_json::{Value, json};

pub(crate) const SESSION_EVIDENCE_KPI_SCHEMA_ID: &str = "codelens-session-evidence-kpis";

pub(super) fn build_derived_kpis(
    session: &SessionMetrics,
    handle_reads: u64,
    watcher_stats: Option<&WatcherStats>,
    watcher_failure_health: &WatcherFailureHealth,
) -> Value {
    let suggestion_resolved_count =
        session.guidance.suggestion_accepted_count + session.guidance.suggestion_diverted_count;
    let suggestion_total_count =
        suggestion_resolved_count + session.guidance.suggestion_unresolved_count;
    json!({
        "schema_version": SESSION_EVIDENCE_KPI_SCHEMA_ID,
        "composite_ratio": ratio_u64(session.call_type.composite_calls, session.core.total_calls),
        "surface_token_efficiency": ratio_usize(session.core.total_tokens, session.core.success_count as usize),
        "low_level_chain_reduction": if session.call_type.low_level_calls > 0 {
            1.0 - ratio_u64(
                session.guidance.repeated_low_level_chain_count,
                session.call_type.low_level_calls,
            )
        } else {
            1.0
        },
        "handle_reuse_rate": ratio_u64(session.truncation.handle_reuse_count, handle_reads),
        "analysis_cache_hit_rate": ratio_u64(
            session.context.analysis_cache_hit_count,
            session.call_type.composite_calls,
        ),
        "quality_contract_present_rate": ratio_u64(
            session.guidance.quality_contract_emitted_count,
            session.call_type.composite_calls,
        ),
        "recommended_check_followthrough_rate": ratio_u64(
            session.guidance.recommended_check_followthrough_count,
            session.guidance.quality_contract_emitted_count,
        ),
        "quality_focus_reuse_rate": ratio_u64(
            session.guidance.quality_focus_reuse_count,
            session.truncation.handle_reuse_count,
        ),
        "performance_watchpoint_emit_rate": ratio_u64(
            session.guidance.performance_watchpoint_emit_count,
            session.guidance.quality_contract_emitted_count,
        ),
        "verifier_contract_present_rate": ratio_u64(
            session.guidance.verifier_contract_emitted_count,
            session.call_type.composite_calls,
        ),
        "blocker_emit_rate": ratio_u64(
            session.guidance.blocker_emit_count,
            session.guidance.verifier_contract_emitted_count,
        ),
        "verifier_followthrough_rate": ratio_u64(
            session.guidance.verifier_followthrough_count,
            session.guidance.verifier_contract_emitted_count,
        ),
        "coordination_overlap_rate": ratio_u64(
            session.coordination.coordination_overlap_emit_count,
            session.guidance.verifier_contract_emitted_count,
        ),
        "coordination_caution_rate": ratio_u64(
            session.coordination.coordination_caution_emit_count,
            session.guidance.verifier_contract_emitted_count,
        ),
        "coordination_release_ratio": ratio_u64(
            session.coordination.coordination_release_count,
            session.coordination.coordination_claim_count,
        ),
        "mutation_preflight_gate_deny_rate": ratio_u64(
            session.mutation.mutation_preflight_gate_denied_count,
            session.mutation.mutation_preflight_checked_count,
        ),
        "deferred_hidden_tool_call_deny_rate": ratio_u64(
            session.namespace.deferred_hidden_tool_call_denied_count,
            session.namespace.deferred_namespace_expansion_count,
        ),
        "truncation_followup_rate": ratio_u64(
            session.truncation.truncation_followup_count,
            session.truncation.truncated_response_count,
        ),
        "composite_guidance_followthrough_rate": ratio_u64(
            session.guidance.composite_guidance_followed_count,
            session.guidance.composite_guidance_emitted_count,
        ),
        "composite_guidance_miss_rate": ratio_u64(
            session.guidance.composite_guidance_missed_count,
            session.guidance.composite_guidance_emitted_count,
        ),
        "suggestion_acceptance_rate": ratio_u64(
            session.guidance.suggestion_accepted_count,
            suggestion_resolved_count,
        ),
        "suggestion_resolution_rate": ratio_u64(
            suggestion_resolved_count,
            suggestion_total_count,
        ),
        "suggestion_successful_outcome_rate": ratio_u64(
            session.guidance.suggestion_outcome_success_count,
            session.guidance.suggestion_accepted_count,
        ),
        "suggestion_value_rate": ratio_u64(
            session.guidance.suggestion_outcome_success_count,
            suggestion_resolved_count,
        ),
        "analysis_job_success_rate": ratio_u64(
            session.jobs.analysis_jobs_completed,
            session.jobs.analysis_jobs_started,
        ),
        "watcher_lock_contention_rate": watcher_lock_contention_rate(watcher_stats),
        "watcher_recent_failure_share": ratio_usize(
            watcher_failure_health.recent_failures,
            watcher_failure_health.total_failures,
        ),
        "inferred_session_type": infer_session_type(&session.timeline),
    })
}

fn ratio_u64(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn ratio_usize(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn watcher_lock_contention_rate(watcher_stats: Option<&WatcherStats>) -> f64 {
    match watcher_stats {
        Some(stats) if stats.events_processed > 0 => {
            stats.lock_contention_batches as f64 / stats.events_processed as f64
        }
        _ => 0.0,
    }
}

fn infer_session_type(timeline: &[ToolInvocation]) -> &'static str {
    let mut mutation_count = 0u32;
    let mut review_count = 0u32;
    let mut exploration_count = 0u32;
    let mut refactor_count = 0u32;

    for entry in timeline {
        let operation = entry.resolved_target.as_deref().unwrap_or(&entry.tool);
        match operation {
            "rename_symbol"
            | "replace_symbol_body"
            | "replace_content"
            | "replace_lines"
            | "delete_lines"
            | "insert_content"
            | "insert_at_line"
            | "create_text_file"
            | "add_import"
            | "refactor_extract_function"
            | "refactor_inline_function"
            | "refactor_move_to_file"
            | "refactor_change_signature" => mutation_count += 1,

            "get_changed_files"
            | "get_impact_analysis"
            | "diff_aware_references"
            | "review_architecture"
            | "analyze_change_impact"
            | "audit_security_context"
            | "cleanup_duplicate_logic"
            | "impact_report"
            | "verify_change_readiness" => review_count += 1,

            "explore_codebase"
            | "trace_request_path"
            | "onboard_project"
            | "get_project_structure"
            | "get_symbols_overview"
            | "get_current_config"
            | "activate_project" => exploration_count += 1,

            "plan_safe_refactor"
            | "safe_rename_report"
            | "refactor_safety_report"
            | "unresolved_reference_check"
            | "find_scoped_references" => refactor_count += 1,

            _ => {}
        }
    }

    if refactor_count >= 2 || (mutation_count >= 1 && refactor_count >= 1) {
        "refactoring"
    } else if review_count >= 2 {
        "code_review"
    } else if mutation_count >= 2 {
        "code_modification"
    } else if exploration_count >= 2 {
        "onboarding"
    } else if timeline.len() < 5 {
        "brief"
    } else {
        "mixed"
    }
}

#[cfg(test)]
#[path = "derived_kpis_tests.rs"]
mod tests;
