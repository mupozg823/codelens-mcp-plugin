use crate::AppState;
use serde_json::{Map, Value};

mod derived_kpis;
mod session_fields;
mod token_bill;

use derived_kpis::build_derived_kpis;
use session_fields::{SessionFieldInputs, build_session_fields};
use token_bill::build_token_bill_payload;

pub(crate) struct SessionMetricsPayload {
    pub(crate) session: Map<String, Value>,
    pub(crate) derived_kpis: Value,
    pub(crate) token_bill: Value,
}

pub(crate) fn build_session_metrics_payload(
    state: &AppState,
    logical_session_id: Option<&str>,
    coordination_scope: Option<&str>,
) -> Result<SessionMetricsPayload, crate::error::CodeLensError> {
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
        })?;
    let coordination_lock = state.coordination_lock_stats();

    let session_json = build_session_fields(SessionFieldInputs {
        session: &session,
        active_http_sessions: state.active_session_count(),
        session_resume_supported: state.session_resume_supported(),
        session_timeout_seconds: state.session_timeout_seconds(),
        coordination: &coordination,
        coordination_lock: &coordination_lock,
        daemon_mode: state.daemon_mode().as_str(),
        watcher_stats: watcher_stats.as_ref(),
        watcher_failure_health: &watcher_failure_health,
    });

    let derived_kpis = build_derived_kpis(
        &session,
        handle_reads,
        watcher_stats.as_ref(),
        &watcher_failure_health,
    );
    let token_bill = build_token_bill_payload(&session);

    Ok(SessionMetricsPayload {
        session: session_json,
        derived_kpis,
        token_bill,
    })
}
