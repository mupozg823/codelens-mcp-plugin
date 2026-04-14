use crate::AppState;
use crate::tool_defs::canonical_tool_name;
use serde_json::{Map, Value, json};

pub(crate) struct SessionMetricsPayload {
    pub(crate) session: Map<String, Value>,
    pub(crate) derived_kpis: Value,
}

pub(crate) fn build_session_metrics_payload(state: &AppState) -> SessionMetricsPayload {
    let session = state.metrics().session_snapshot();
    let handle_reads = session.analysis_summary_reads + session.analysis_section_reads;
    let watcher_stats = state.watcher_stats();
    let watcher_failure_health = state.watcher_failure_health();
    let coordination = state
        .coordination_counts_for_session(&crate::session_context::SessionRequestContext::default());

    let mut session_json = Map::new();
    session_json.insert("total_calls".to_owned(), json!(session.total_calls));
    session_json.insert("success_count".to_owned(), json!(session.success_count));
    session_json.insert("total_ms".to_owned(), json!(session.total_ms));
    session_json.insert("total_tokens".to_owned(), json!(session.total_tokens));
    session_json.insert("error_count".to_owned(), json!(session.error_count));
    session_json.insert(
        "tools_list_tokens".to_owned(),
        json!(session.tools_list_tokens),
    );
    session_json.insert(
        "analysis_summary_reads".to_owned(),
        json!(session.analysis_summary_reads),
    );
    session_json.insert(
        "analysis_section_reads".to_owned(),
        json!(session.analysis_section_reads),
    );
    session_json.insert(
        "active_http_sessions".to_owned(),
        json!(state.active_session_count()),
    );
    session_json.insert(
        "session_resume_supported".to_owned(),
        json!(state.session_resume_supported()),
    );
    session_json.insert(
        "session_timeout_seconds".to_owned(),
        json!(state.session_timeout_seconds()),
    );
    session_json.insert(
        "active_coordination_agents".to_owned(),
        json!(coordination.active_agents),
    );
    session_json.insert(
        "active_coordination_claims".to_owned(),
        json!(coordination.active_claims),
    );
    session_json.insert("retry_count".to_owned(), json!(session.retry_count));
    session_json.insert(
        "analysis_cache_hit_count".to_owned(),
        json!(session.analysis_cache_hit_count),
    );
    session_json.insert(
        "truncated_response_count".to_owned(),
        json!(session.truncated_response_count),
    );
    session_json.insert(
        "truncation_followup_count".to_owned(),
        json!(session.truncation_followup_count),
    );
    session_json.insert(
        "truncation_same_tool_retry_count".to_owned(),
        json!(session.truncation_same_tool_retry_count),
    );
    session_json.insert(
        "truncation_handle_followup_count".to_owned(),
        json!(session.truncation_handle_followup_count),
    );
    session_json.insert(
        "handle_reuse_count".to_owned(),
        json!(session.handle_reuse_count),
    );
    session_json.insert(
        "repeated_low_level_chain_count".to_owned(),
        json!(session.repeated_low_level_chain_count),
    );
    session_json.insert(
        "composite_guidance_emitted_count".to_owned(),
        json!(session.composite_guidance_emitted_count),
    );
    session_json.insert(
        "composite_guidance_followed_count".to_owned(),
        json!(session.composite_guidance_followed_count),
    );
    session_json.insert(
        "quality_contract_emitted_count".to_owned(),
        json!(session.quality_contract_emitted_count),
    );
    session_json.insert(
        "recommended_checks_emitted_count".to_owned(),
        json!(session.recommended_checks_emitted_count),
    );
    session_json.insert(
        "recommended_check_followthrough_count".to_owned(),
        json!(session.recommended_check_followthrough_count),
    );
    session_json.insert(
        "quality_focus_reuse_count".to_owned(),
        json!(session.quality_focus_reuse_count),
    );
    session_json.insert(
        "performance_watchpoint_emit_count".to_owned(),
        json!(session.performance_watchpoint_emit_count),
    );
    session_json.insert(
        "verifier_contract_emitted_count".to_owned(),
        json!(session.verifier_contract_emitted_count),
    );
    session_json.insert(
        "blocker_emit_count".to_owned(),
        json!(session.blocker_emit_count),
    );
    session_json.insert(
        "verifier_followthrough_count".to_owned(),
        json!(session.verifier_followthrough_count),
    );
    session_json.insert(
        "coordination_registration_count".to_owned(),
        json!(session.coordination_registration_count),
    );
    session_json.insert(
        "coordination_claim_count".to_owned(),
        json!(session.coordination_claim_count),
    );
    session_json.insert(
        "coordination_release_count".to_owned(),
        json!(session.coordination_release_count),
    );
    session_json.insert(
        "coordination_overlap_emit_count".to_owned(),
        json!(session.coordination_overlap_emit_count),
    );
    session_json.insert(
        "coordination_caution_emit_count".to_owned(),
        json!(session.coordination_caution_emit_count),
    );
    session_json.insert(
        "mutation_preflight_checked_count".to_owned(),
        json!(session.mutation_preflight_checked_count),
    );
    session_json.insert(
        "mutation_without_preflight_count".to_owned(),
        json!(session.mutation_without_preflight_count),
    );
    session_json.insert(
        "mutation_preflight_gate_denied_count".to_owned(),
        json!(session.mutation_preflight_gate_denied_count),
    );
    session_json.insert(
        "stale_preflight_reject_count".to_owned(),
        json!(session.stale_preflight_reject_count),
    );
    session_json.insert(
        "mutation_with_caution_count".to_owned(),
        json!(session.mutation_with_caution_count),
    );
    session_json.insert(
        "rename_without_symbol_preflight_count".to_owned(),
        json!(session.rename_without_symbol_preflight_count),
    );
    session_json.insert(
        "deferred_namespace_expansion_count".to_owned(),
        json!(session.deferred_namespace_expansion_count),
    );
    session_json.insert(
        "deferred_hidden_tool_call_denied_count".to_owned(),
        json!(session.deferred_hidden_tool_call_denied_count),
    );
    session_json.insert("composite_calls".to_owned(), json!(session.composite_calls));
    session_json.insert("low_level_calls".to_owned(), json!(session.low_level_calls));
    session_json.insert(
        "stdio_session_count".to_owned(),
        json!(session.stdio_session_count),
    );
    session_json.insert(
        "http_session_count".to_owned(),
        json!(session.http_session_count),
    );
    session_json.insert(
        "analysis_jobs_enqueued".to_owned(),
        json!(session.analysis_jobs_enqueued),
    );
    session_json.insert(
        "analysis_jobs_started".to_owned(),
        json!(session.analysis_jobs_started),
    );
    session_json.insert(
        "analysis_jobs_completed".to_owned(),
        json!(session.analysis_jobs_completed),
    );
    session_json.insert(
        "analysis_jobs_failed".to_owned(),
        json!(session.analysis_jobs_failed),
    );
    session_json.insert(
        "analysis_jobs_cancelled".to_owned(),
        json!(session.analysis_jobs_cancelled),
    );
    session_json.insert(
        "analysis_queue_depth".to_owned(),
        json!(session.analysis_queue_depth),
    );
    session_json.insert(
        "analysis_queue_max_depth".to_owned(),
        json!(session.analysis_queue_max_depth),
    );
    session_json.insert(
        "analysis_queue_weighted_depth".to_owned(),
        json!(session.analysis_queue_weighted_depth),
    );
    session_json.insert(
        "analysis_queue_max_weighted_depth".to_owned(),
        json!(session.analysis_queue_max_weighted_depth),
    );
    session_json.insert(
        "analysis_queue_priority_promotions".to_owned(),
        json!(session.analysis_queue_priority_promotions),
    );
    session_json.insert(
        "active_analysis_workers".to_owned(),
        json!(session.active_analysis_workers),
    );
    session_json.insert(
        "peak_active_analysis_workers".to_owned(),
        json!(session.peak_active_analysis_workers),
    );
    session_json.insert(
        "analysis_worker_limit".to_owned(),
        json!(session.analysis_worker_limit),
    );
    session_json.insert(
        "analysis_cost_budget".to_owned(),
        json!(session.analysis_cost_budget),
    );
    session_json.insert(
        "analysis_transport_mode".to_owned(),
        json!(session.analysis_transport_mode.clone()),
    );
    session_json.insert(
        "daemon_mode".to_owned(),
        json!(state.daemon_mode().as_str()),
    );
    session_json.insert(
        "watcher_running".to_owned(),
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.running)
                .unwrap_or(false)
        ),
    );
    session_json.insert(
        "watcher_events_processed".to_owned(),
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.events_processed)
                .unwrap_or(0)
        ),
    );
    session_json.insert(
        "watcher_files_reindexed".to_owned(),
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.files_reindexed)
                .unwrap_or(0)
        ),
    );
    session_json.insert(
        "watcher_lock_contention_batches".to_owned(),
        json!(
            watcher_stats
                .as_ref()
                .map(|stats| stats.lock_contention_batches)
                .unwrap_or(0)
        ),
    );
    session_json.insert(
        "watcher_index_failures".to_owned(),
        json!(watcher_failure_health.recent_failures),
    );
    session_json.insert(
        "watcher_index_failures_total".to_owned(),
        json!(watcher_failure_health.total_failures),
    );
    session_json.insert(
        "watcher_stale_index_failures".to_owned(),
        json!(watcher_failure_health.stale_failures),
    );
    session_json.insert(
        "watcher_persistent_index_failures".to_owned(),
        json!(watcher_failure_health.persistent_failures),
    );
    session_json.insert(
        "watcher_pruned_missing_failures".to_owned(),
        json!(watcher_failure_health.pruned_missing_failures),
    );
    session_json.insert(
        "watcher_recent_failure_window_seconds".to_owned(),
        json!(watcher_failure_health.recent_window_seconds),
    );
    session_json.insert(
        "avg_ms_per_call".to_owned(),
        json!(if session.total_calls > 0 {
            session.total_ms / session.total_calls
        } else {
            0
        }),
    );
    session_json.insert(
        "avg_tool_output_tokens".to_owned(),
        json!(if session.total_calls > 0 {
            session.total_tokens / session.total_calls as usize
        } else {
            0
        }),
    );
    session_json.insert(
        "p95_tool_latency_ms".to_owned(),
        json!(crate::telemetry::percentile_95(&session.latency_samples)),
    );
    session_json.insert("timeline_length".to_owned(), json!(session.timeline.len()));

    let derived_kpis = json!({
        "composite_ratio": if session.total_calls > 0 {
            session.composite_calls as f64 / session.total_calls as f64
        } else { 0.0 },
        "surface_token_efficiency": if session.success_count > 0 {
            session.total_tokens as f64 / session.success_count as f64
        } else { 0.0 },
        "low_level_chain_reduction": if session.low_level_calls > 0 {
            1.0 - (session.repeated_low_level_chain_count as f64 / session.low_level_calls as f64)
        } else { 1.0 },
        "handle_reuse_rate": if handle_reads > 0 {
            session.handle_reuse_count as f64 / handle_reads as f64
        } else { 0.0 },
        "analysis_cache_hit_rate": if session.composite_calls > 0 {
            session.analysis_cache_hit_count as f64 / session.composite_calls as f64
        } else { 0.0 },
        "quality_contract_present_rate": if session.composite_calls > 0 {
            session.quality_contract_emitted_count as f64 / session.composite_calls as f64
        } else { 0.0 },
        "recommended_check_followthrough_rate": if session.quality_contract_emitted_count > 0 {
            session.recommended_check_followthrough_count as f64 / session.quality_contract_emitted_count as f64
        } else { 0.0 },
        "quality_focus_reuse_rate": if session.handle_reuse_count > 0 {
            session.quality_focus_reuse_count as f64 / session.handle_reuse_count as f64
        } else { 0.0 },
        "performance_watchpoint_emit_rate": if session.quality_contract_emitted_count > 0 {
            session.performance_watchpoint_emit_count as f64 / session.quality_contract_emitted_count as f64
        } else { 0.0 },
        "verifier_contract_present_rate": if session.composite_calls > 0 {
            session.verifier_contract_emitted_count as f64 / session.composite_calls as f64
        } else { 0.0 },
        "blocker_emit_rate": if session.verifier_contract_emitted_count > 0 {
            session.blocker_emit_count as f64 / session.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "verifier_followthrough_rate": if session.verifier_contract_emitted_count > 0 {
            session.verifier_followthrough_count as f64 / session.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "coordination_overlap_rate": if session.verifier_contract_emitted_count > 0 {
            session.coordination_overlap_emit_count as f64 / session.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "coordination_caution_rate": if session.verifier_contract_emitted_count > 0 {
            session.coordination_caution_emit_count as f64 / session.verifier_contract_emitted_count as f64
        } else { 0.0 },
        "coordination_release_ratio": if session.coordination_claim_count > 0 {
            session.coordination_release_count as f64 / session.coordination_claim_count as f64
        } else { 0.0 },
        "mutation_preflight_gate_deny_rate": if session.mutation_preflight_checked_count > 0 {
            session.mutation_preflight_gate_denied_count as f64
                / session.mutation_preflight_checked_count as f64
        } else { 0.0 },
        "deferred_hidden_tool_call_deny_rate": if session.deferred_namespace_expansion_count > 0 {
            session.deferred_hidden_tool_call_denied_count as f64
                / session.deferred_namespace_expansion_count as f64
        } else { 0.0 },
        "truncation_followup_rate": if session.truncated_response_count > 0 {
            session.truncation_followup_count as f64 / session.truncated_response_count as f64
        } else { 0.0 },
        "composite_guidance_followthrough_rate": if session.composite_guidance_emitted_count > 0 {
            session.composite_guidance_followed_count as f64 / session.composite_guidance_emitted_count as f64
        } else { 0.0 },
        "analysis_job_success_rate": if session.analysis_jobs_started > 0 {
            session.analysis_jobs_completed as f64 / session.analysis_jobs_started as f64
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
        match canonical_tool_name(&entry.tool) {
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
            | "review_changes"
            | "semantic_code_review"
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
