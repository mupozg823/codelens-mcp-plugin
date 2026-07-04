use crate::runtime_types::WatcherFailureHealth;
use crate::telemetry::{SessionMetrics, ToolInvocation};
use codelens_engine::WatcherStats;
use serde_json::{Value, json};

pub(super) fn build_derived_kpis(
    session: &SessionMetrics,
    handle_reads: u64,
    watcher_stats: Option<&WatcherStats>,
    watcher_failure_health: &WatcherFailureHealth,
) -> Value {
    json!({
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

#[cfg(test)]
mod tests {
    use super::build_derived_kpis;
    use crate::runtime_types::WatcherFailureHealth;
    use crate::telemetry::{SessionMetrics, ToolInvocation};
    use codelens_engine::WatcherStats;
    use serde_json::json;

    #[test]
    fn computes_rates_and_infers_refactoring_session() {
        let session = SessionMetrics {
            core: crate::telemetry::CoreMetrics {
                total_calls: 4,
                success_count: 2,
                total_tokens: 1000,
                ..Default::default()
            },
            call_type: crate::telemetry::CallTypeMetrics {
                composite_calls: 2,
                low_level_calls: 2,
            },
            truncation: crate::telemetry::TruncationMetrics {
                handle_reuse_count: 1,
                ..Default::default()
            },
            jobs: crate::telemetry::AnalysisJobMetrics {
                analysis_jobs_started: 4,
                analysis_jobs_completed: 3,
                ..Default::default()
            },
            timeline: vec![
                invocation("plan_safe_refactor"),
                invocation("safe_rename_report"),
                invocation("rename_symbol"),
                invocation("replace_symbol_body"),
            ],
            ..Default::default()
        };
        let watcher_stats = WatcherStats {
            running: true,
            events_processed: 10,
            files_reindexed: 7,
            lock_contention_batches: 2,
            index_failures: None,
        };
        let watcher_failure_health = WatcherFailureHealth {
            recent_failures: 1,
            total_failures: 4,
            ..Default::default()
        };

        let kpis = build_derived_kpis(&session, 2, Some(&watcher_stats), &watcher_failure_health);

        assert_eq!(kpis["composite_ratio"], json!(0.5));
        assert_eq!(kpis["surface_token_efficiency"], json!(500.0));
        assert_eq!(kpis["handle_reuse_rate"], json!(0.5));
        assert_eq!(kpis["analysis_job_success_rate"], json!(0.75));
        assert_eq!(kpis["watcher_lock_contention_rate"], json!(0.2));
        assert_eq!(kpis["watcher_recent_failure_share"], json!(0.25));
        assert_eq!(kpis["inferred_session_type"], json!("refactoring"));
    }

    fn invocation(tool: &str) -> ToolInvocation {
        ToolInvocation {
            tool: tool.to_owned(),
            surface: "builder-minimal".to_owned(),
            elapsed_ms: 1,
            tokens: 1,
            success: true,
            truncated: false,
            phase: None,
            target_paths: Vec::new(),
        }
    }
}
