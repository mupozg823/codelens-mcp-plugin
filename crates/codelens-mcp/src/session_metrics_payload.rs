use crate::AppState;
use serde_json::{Map, Value, json};

pub(crate) struct SessionMetricsPayload {
    pub(crate) session: Map<String, Value>,
    pub(crate) derived_kpis: Value,
}

fn put(m: &mut Map<String, Value>, k: &str, v: Value) {
    m.insert(k.to_owned(), v);
}

pub(crate) fn build_session_metrics_payload(
    state: &AppState,
    logical_session_id: Option<&str>,
    coordination_scope: Option<&str>,
) -> SessionMetricsPayload {
    let session = logical_session_id
        .map(|session_id| state.metrics().session_snapshot_for(session_id))
        .unwrap_or_else(|| state.metrics().session_snapshot());
    let handle_reads =
        session.context.analysis_summary_reads + session.context.analysis_section_reads;
    let watcher_stats = state.watcher_stats();
    let watcher_failure_health = state.watcher_failure_health();
    let coordination = coordination_scope
        .map(|scope| state.coordination_counts_for_scope(scope))
        .unwrap_or_else(|| {
            state.coordination_counts_for_session(
                &crate::session_context::SessionRequestContext::default(),
            )
        });
    let coordination_lock = state.coordination_lock_stats();

    let mut session_json = Map::new();
    put(
        &mut session_json,
        "total_calls",
        json!(session.core.total_calls),
    );
    put(
        &mut session_json,
        "success_count",
        json!(session.core.success_count),
    );
    put(&mut session_json, "total_ms", json!(session.core.total_ms));
    put(
        &mut session_json,
        "total_tokens",
        json!(session.core.total_tokens),
    );
    put(
        &mut session_json,
        "error_count",
        json!(session.core.error_count),
    );
    put(
        &mut session_json,
        "tools_list_tokens",
        json!(session.token.tools_list_tokens),
    );
    put(
        &mut session_json,
        "analysis_summary_reads",
        json!(session.context.analysis_summary_reads),
    );
    put(
        &mut session_json,
        "analysis_section_reads",
        json!(session.context.analysis_section_reads),
    );
    put(
        &mut session_json,
        "active_http_sessions",
        json!(state.active_session_count()),
    );
    put(
        &mut session_json,
        "session_resume_supported",
        json!(state.session_resume_supported()),
    );
    put(
        &mut session_json,
        "session_timeout_seconds",
        json!(state.session_timeout_seconds()),
    );
    put(
        &mut session_json,
        "active_coordination_agents",
        json!(coordination.active_agents),
    );
    put(
        &mut session_json,
        "active_coordination_claims",
        json!(coordination.active_claims),
    );
    put(
        &mut session_json,
        "coordination_lock_acquire_count",
        json!(coordination_lock.acquire_count),
    );
    put(
        &mut session_json,
        "coordination_lock_wait_total_micros",
        json!(coordination_lock.wait_total_micros),
    );
    put(
        &mut session_json,
        "coordination_lock_wait_max_micros",
        json!(coordination_lock.wait_max_micros),
    );
    put(
        &mut session_json,
        "coordination_lock_avg_wait_micros",
        json!(coordination_lock.avg_wait_micros()),
    );
    put(
        &mut session_json,
        "retry_count",
        json!(session.core.retry_count),
    );
    put(
        &mut session_json,
        "analysis_cache_hit_count",
        json!(session.context.analysis_cache_hit_count),
    );
    put(
        &mut session_json,
        "analysis_cache_hit_exact_count",
        json!(session.context.analysis_cache_hit_exact_count),
    );
    put(
        &mut session_json,
        "analysis_cache_hit_warm_count",
        json!(session.context.analysis_cache_hit_warm_count),
    );
    put(
        &mut session_json,
        "analysis_cache_hit_cold_count",
        json!(session.context.analysis_cache_hit_cold_count),
    );
    put(
        &mut session_json,
        "truncated_response_count",
        json!(session.truncation.truncated_response_count),
    );
    put(
        &mut session_json,
        "truncation_followup_count",
        json!(session.truncation.truncation_followup_count),
    );
    put(
        &mut session_json,
        "truncation_same_tool_retry_count",
        json!(session.truncation.truncation_same_tool_retry_count),
    );
    put(
        &mut session_json,
        "truncation_handle_followup_count",
        json!(session.truncation.truncation_handle_followup_count),
    );
    put(
        &mut session_json,
        "handle_reuse_count",
        json!(session.truncation.handle_reuse_count),
    );
    put(
        &mut session_json,
        "repeated_low_level_chain_count",
        json!(session.guidance.repeated_low_level_chain_count),
    );
    put(
        &mut session_json,
        "composite_guidance_emitted_count",
        json!(session.guidance.composite_guidance_emitted_count),
    );
    put(
        &mut session_json,
        "composite_guidance_followed_count",
        json!(session.guidance.composite_guidance_followed_count),
    );
    put(
        &mut session_json,
        "composite_guidance_missed_count",
        json!(session.guidance.composite_guidance_missed_count),
    );
    put(
        &mut session_json,
        "composite_guidance_missed_by_origin",
        json!(session.guidance.composite_guidance_missed_by_origin),
    );
    put(
        &mut session_json,
        "quality_contract_emitted_count",
        json!(session.guidance.quality_contract_emitted_count),
    );
    put(
        &mut session_json,
        "recommended_checks_emitted_count",
        json!(session.guidance.recommended_checks_emitted_count),
    );
    put(
        &mut session_json,
        "recommended_check_followthrough_count",
        json!(session.guidance.recommended_check_followthrough_count),
    );
    put(
        &mut session_json,
        "quality_focus_reuse_count",
        json!(session.guidance.quality_focus_reuse_count),
    );
    put(
        &mut session_json,
        "performance_watchpoint_emit_count",
        json!(session.guidance.performance_watchpoint_emit_count),
    );
    put(
        &mut session_json,
        "verifier_contract_emitted_count",
        json!(session.guidance.verifier_contract_emitted_count),
    );
    put(
        &mut session_json,
        "blocker_emit_count",
        json!(session.guidance.blocker_emit_count),
    );
    put(
        &mut session_json,
        "verifier_followthrough_count",
        json!(session.guidance.verifier_followthrough_count),
    );
    put(
        &mut session_json,
        "coordination_registration_count",
        json!(session.coordination.coordination_registration_count),
    );
    put(
        &mut session_json,
        "coordination_claim_count",
        json!(session.coordination.coordination_claim_count),
    );
    put(
        &mut session_json,
        "coordination_release_count",
        json!(session.coordination.coordination_release_count),
    );
    put(
        &mut session_json,
        "coordination_overlap_emit_count",
        json!(session.coordination.coordination_overlap_emit_count),
    );
    put(
        &mut session_json,
        "coordination_caution_emit_count",
        json!(session.coordination.coordination_caution_emit_count),
    );
    put(
        &mut session_json,
        "mutation_preflight_checked_count",
        json!(session.mutation.mutation_preflight_checked_count),
    );
    put(
        &mut session_json,
        "mutation_without_preflight_count",
        json!(session.mutation.mutation_without_preflight_count),
    );
    put(
        &mut session_json,
        "mutation_preflight_gate_denied_count",
        json!(session.mutation.mutation_preflight_gate_denied_count),
    );
    put(
        &mut session_json,
        "stale_preflight_reject_count",
        json!(session.mutation.stale_preflight_reject_count),
    );
    put(
        &mut session_json,
        "mutation_with_caution_count",
        json!(session.mutation.mutation_with_caution_count),
    );
    put(
        &mut session_json,
        "rename_without_symbol_preflight_count",
        json!(session.mutation.rename_without_symbol_preflight_count),
    );
    put(
        &mut session_json,
        "deferred_namespace_expansion_count",
        json!(session.namespace.deferred_namespace_expansion_count),
    );
    put(
        &mut session_json,
        "deferred_hidden_tool_call_denied_count",
        json!(session.namespace.deferred_hidden_tool_call_denied_count),
    );
    put(
        &mut session_json,
        "profile_switch_count",
        json!(session.namespace.profile_switch_count),
    );
    put(
        &mut session_json,
        "preset_switch_count",
        json!(session.namespace.preset_switch_count),
    );
    put(
        &mut session_json,
        "composite_calls",
        json!(session.call_type.composite_calls),
    );
    put(
        &mut session_json,
        "low_level_calls",
        json!(session.call_type.low_level_calls),
    );
    put(
        &mut session_json,
        "stdio_session_count",
        json!(session.transport.stdio_session_count),
    );
    put(
        &mut session_json,
        "http_session_count",
        json!(session.transport.http_session_count),
    );
    put(
        &mut session_json,
        "analysis_jobs_enqueued",
        json!(session.jobs.analysis_jobs_enqueued),
    );
    put(
        &mut session_json,
        "analysis_jobs_started",
        json!(session.jobs.analysis_jobs_started),
    );
    put(
        &mut session_json,
        "analysis_jobs_completed",
        json!(session.jobs.analysis_jobs_completed),
    );
    put(
        &mut session_json,
        "analysis_jobs_failed",
        json!(session.jobs.analysis_jobs_failed),
    );
    put(
        &mut session_json,
        "analysis_jobs_cancelled",
        json!(session.jobs.analysis_jobs_cancelled),
    );
    put(
        &mut session_json,
        "analysis_queue_depth",
        json!(session.jobs.analysis_queue_depth),
    );
    put(
        &mut session_json,
        "analysis_queue_max_depth",
        json!(session.jobs.analysis_queue_max_depth),
    );
    put(
        &mut session_json,
        "analysis_queue_weighted_depth",
        json!(session.jobs.analysis_queue_weighted_depth),
    );
    put(
        &mut session_json,
        "analysis_queue_max_weighted_depth",
        json!(session.jobs.analysis_queue_max_weighted_depth),
    );
    put(
        &mut session_json,
        "analysis_queue_priority_promotions",
        json!(session.jobs.analysis_queue_priority_promotions),
    );
    put(
        &mut session_json,
        "active_analysis_workers",
        json!(session.jobs.active_analysis_workers),
    );
    put(
        &mut session_json,
        "peak_active_analysis_workers",
        json!(session.jobs.peak_active_analysis_workers),
    );
    put(
        &mut session_json,
        "analysis_worker_limit",
        json!(session.jobs.analysis_worker_limit),
    );
    put(
        &mut session_json,
        "analysis_cost_budget",
        json!(session.jobs.analysis_cost_budget),
    );
    put(
        &mut session_json,
        "analysis_transport_mode",
        json!(session.jobs.analysis_transport_mode.clone()),
    );
    put(
        &mut session_json,
        "daemon_mode",
        json!(state.daemon_mode().as_str()),
    );
    put(
        &mut session_json,
        "watcher_running",
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.running)
                .unwrap_or(false)
        ),
    );
    put(
        &mut session_json,
        "watcher_events_processed",
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.events_processed)
                .unwrap_or(0)
        ),
    );
    put(
        &mut session_json,
        "watcher_files_reindexed",
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.files_reindexed)
                .unwrap_or(0)
        ),
    );
    put(
        &mut session_json,
        "watcher_lock_contention_batches",
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.lock_contention_batches)
                .unwrap_or(0)
        ),
    );
    put(
        &mut session_json,
        "watcher_index_failures",
        json!(watcher_failure_health.recent_failures),
    );
    put(
        &mut session_json,
        "watcher_index_failures_total",
        json!(watcher_failure_health.total_failures),
    );
    put(
        &mut session_json,
        "watcher_stale_index_failures",
        json!(watcher_failure_health.stale_failures),
    );
    put(
        &mut session_json,
        "watcher_persistent_index_failures",
        json!(watcher_failure_health.persistent_failures),
    );
    put(
        &mut session_json,
        "watcher_pruned_missing_failures",
        json!(watcher_failure_health.pruned_missing_failures),
    );
    put(
        &mut session_json,
        "watcher_recent_failure_window_seconds",
        json!(watcher_failure_health.recent_window_seconds),
    );
    put(
        &mut session_json,
        "avg_ms_per_call",
        json!(
            session
                .core
                .total_ms
                .checked_div(session.core.total_calls)
                .unwrap_or(0)
        ),
    );
    put(
        &mut session_json,
        "avg_tool_output_tokens",
        json!(if session.core.total_calls > 0 {
            session.core.total_tokens / session.core.total_calls as usize
        } else {
            0
        }),
    );
    put(
        &mut session_json,
        "p95_tool_latency_ms",
        json!(crate::telemetry::percentile_95(
            &session.core.latency_samples
        )),
    );
    put(
        &mut session_json,
        "timeline_length",
        json!(session.timeline.len()),
    );

    let derived_kpis = json!({
        "composite_ratio": if session.core.total_calls > 0 {
            session.call_type.composite_calls as f64 / session.core.total_calls as f64
        } else { 0.0 },
        "surface_token_efficiency": if session.core.success_count > 0 {
            session.core.total_tokens as f64 / session.core.success_count as f64
        } else { 0.0 },
        "low_level_chain_reduction": if session.call_type.low_level_calls > 0 {
            1.0 - (session.guidance.repeated_low_level_chain_count as f64 / session.call_type.low_level_calls as f64)
        } else { 1.0 },
        "handle_reuse_rate": if handle_reads > 0 {
            session.truncation.handle_reuse_count as f64 / handle_reads as f64
        } else { 0.0 },
        "analysis_cache_hit_rate": if session.call_type.composite_calls > 0 {
            session.context.analysis_cache_hit_count as f64 / session.call_type.composite_calls as f64
        } else { 0.0 },
        "quality_contract_present_rate": if session.call_type.composite_calls > 0 {
            session.guidance.quality_contract_emitted_count as f64 / session.call_type.composite_calls as f64
        } else { 0.0 },
        "recommended_check_followthrough_rate": if session.guidance.quality_contract_emitted_count > 0 {
            session.guidance.recommended_check_followthrough_count as f64 / session.guidance.quality_contract_emitted_count as f64
        } else { 0.0 },
        "quality_focus_reuse_rate": if session.truncation.handle_reuse_count > 0 {
            session.guidance.quality_focus_reuse_count as f64 / session.truncation.handle_reuse_count as f64
        } else { 0.0 },
        "performance_watchpoint_emit_rate": if session.guidance.quality_contract_emitted_count > 0 {
            session.guidance.performance_watchpoint_emit_count as f64 / session.guidance.quality_contract_emitted_count as f64
        } else { 0.0 },
        "verifier_contract_present_rate": if session.call_type.composite_calls > 0 {
            session.guidance.verifier_contract_emitted_count as f64 / session.call_type.composite_calls as f64
        } else { 0.0 },
        "blocker_emit_rate": if session.guidance.verifier_contract_emitted_count > 0 {
            session.guidance.blocker_emit_count as f64 / session.guidance.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "verifier_followthrough_rate": if session.guidance.verifier_contract_emitted_count > 0 {
            session.guidance.verifier_followthrough_count as f64 / session.guidance.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "coordination_overlap_rate": if session.guidance.verifier_contract_emitted_count > 0 {
            session.coordination.coordination_overlap_emit_count as f64 / session.guidance.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "coordination_caution_rate": if session.guidance.verifier_contract_emitted_count > 0 {
            session.coordination.coordination_caution_emit_count as f64 / session.guidance.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "coordination_release_ratio": if session.coordination.coordination_claim_count > 0 {
            session.coordination.coordination_release_count as f64 / session.coordination.coordination_claim_count as f64
        } else { 0.0 },
        "mutation_preflight_gate_deny_rate": if session.mutation.mutation_preflight_checked_count > 0 {
            session.mutation.mutation_preflight_gate_denied_count as f64
                / session.mutation.mutation_preflight_checked_count as f64
        } else { 0.0 },
        "deferred_hidden_tool_call_deny_rate": if session.namespace.deferred_namespace_expansion_count > 0 {
            session.namespace.deferred_hidden_tool_call_denied_count as f64
                / session.namespace.deferred_namespace_expansion_count as f64
        } else { 0.0 },
        "truncation_followup_rate": if session.truncation.truncated_response_count > 0 {
            session.truncation.truncation_followup_count as f64 / session.truncation.truncated_response_count as f64
        } else { 0.0 },
        "composite_guidance_followthrough_rate": if session.guidance.composite_guidance_emitted_count > 0 {
            session.guidance.composite_guidance_followed_count as f64 / session.guidance.composite_guidance_emitted_count as f64
        } else { 0.0 },
        "composite_guidance_miss_rate": if session.guidance.composite_guidance_emitted_count > 0 {
            session.guidance.composite_guidance_missed_count as f64 / session.guidance.composite_guidance_emitted_count as f64
        } else { 0.0 },
        "analysis_job_success_rate": if session.jobs.analysis_jobs_started > 0 {
            session.jobs.analysis_jobs_completed as f64 / session.jobs.analysis_jobs_started as f64
        } else { 0.0 },
        "watcher_lock_contention_rate": if watcher_stats
            .as_ref()
            .map(|stats| stats.events_processed)
            .unwrap_or(0)
            > 0
        {
            watcher_stats
                .as_ref()
                .map(|stats| stats.lock_contention_batches as f64 / stats.events_processed as f64)
                .unwrap_or(0.0)
        } else { 0.0 },
        "watcher_recent_failure_share": if watcher_failure_health.total_failures > 0 {
            watcher_failure_health.recent_failures as f64
                / watcher_failure_health.total_failures as f64
        } else { 0.0 }
    });

    // Infer session type from tool usage patterns
    let session_type = infer_session_type(&session.timeline);
    let mut kpis = derived_kpis.as_object().cloned().unwrap_or_default();
    kpis.insert("inferred_session_type".to_owned(), json!(session_type));
    let derived_kpis = Value::Object(kpis);

    SessionMetricsPayload {
        session: session_json,
        derived_kpis,
    }
}

/// Classify the session based on tool call patterns in the timeline.
fn infer_session_type(timeline: &[crate::telemetry::ToolInvocation]) -> &'static str {
    let mut mutation_count = 0u32;
    let mut review_count = 0u32;
    let mut exploration_count = 0u32;
    let mut refactor_count = 0u32;

    for entry in timeline {
        match entry.tool.as_str() {
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
