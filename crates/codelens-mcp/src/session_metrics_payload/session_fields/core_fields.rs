use super::{SessionFieldInputs, put};
use serde_json::{Map, Value, json};

pub(super) fn put_core_fields(
    session_json: &mut Map<String, Value>,
    inputs: &SessionFieldInputs<'_>,
) {
    let session = inputs.session;
    put(session_json, "total_calls", json!(session.core.total_calls));
    put(
        session_json,
        "success_count",
        json!(session.core.success_count),
    );
    put(session_json, "total_ms", json!(session.core.total_ms));
    put(
        session_json,
        "total_tokens",
        json!(session.core.total_tokens),
    );
    put(session_json, "error_count", json!(session.core.error_count));
    put(
        session_json,
        "tools_list_tokens",
        json!(session.token.tools_list_tokens),
    );
    put(
        session_json,
        "analysis_summary_reads",
        json!(session.context.analysis_summary_reads),
    );
    put(
        session_json,
        "analysis_section_reads",
        json!(session.context.analysis_section_reads),
    );
    put(
        session_json,
        "active_http_sessions",
        json!(inputs.active_http_sessions),
    );
    put(
        session_json,
        "session_resume_supported",
        json!(inputs.session_resume_supported),
    );
    put(
        session_json,
        "session_timeout_seconds",
        json!(inputs.session_timeout_seconds),
    );
    put(
        session_json,
        "active_coordination_agents",
        json!(inputs.coordination.active_agents),
    );
    put(
        session_json,
        "active_coordination_claims",
        json!(inputs.coordination.active_claims),
    );
    put(
        session_json,
        "coordination_lock_acquire_count",
        json!(inputs.coordination_lock.acquire_count),
    );
    put(
        session_json,
        "coordination_lock_wait_total_micros",
        json!(inputs.coordination_lock.wait_total_micros),
    );
    put(
        session_json,
        "coordination_lock_wait_max_micros",
        json!(inputs.coordination_lock.wait_max_micros),
    );
    put(
        session_json,
        "coordination_lock_avg_wait_micros",
        json!(inputs.coordination_lock.avg_wait_micros()),
    );
    put(session_json, "retry_count", json!(session.core.retry_count));
    put(
        session_json,
        "analysis_cache_hit_count",
        json!(session.context.analysis_cache_hit_count),
    );
    put(
        session_json,
        "analysis_cache_hit_exact_count",
        json!(session.context.analysis_cache_hit_exact_count),
    );
    put(
        session_json,
        "analysis_cache_hit_warm_count",
        json!(session.context.analysis_cache_hit_warm_count),
    );
    put(
        session_json,
        "analysis_cache_hit_cold_count",
        json!(session.context.analysis_cache_hit_cold_count),
    );
}
