use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;

pub fn register_agent_work(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let agent = state.register_agent_work_for_arguments(arguments)?;
    state.metrics().record_coordination_registration();
    Ok((
        json!({
            "status": "registered",
            "agent": agent,
        }),
        success_meta(BackendKind::Session, 0.93),
    ))
}

pub fn list_active_agents(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let agents = state.list_active_agents_for_arguments(arguments);
    Ok((
        json!({
            "agents": agents,
            "count": agents.len(),
        }),
        success_meta(BackendKind::Session, 0.94),
    ))
}

pub fn claim_files(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let claim = state.claim_files_for_arguments(arguments)?;
    state.metrics().record_coordination_claim();
    let claimed_paths = claim.paths.clone();
    let session_id = claim.session_id.clone();
    Ok((
        json!({
            "status": "claimed",
            "session_id": session_id,
            "claimed_paths": claimed_paths,
            "claim": claim,
        }),
        success_meta(BackendKind::Session, 0.92),
    ))
}

pub fn release_files(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let (session_id, released_paths, remaining_claim) =
        state.release_files_for_arguments(arguments)?;
    state.metrics().record_coordination_release();
    let remaining_claim_count = usize::from(remaining_claim.is_some());
    Ok((
        json!({
            "status": "released",
            "session_id": session_id,
            "released_paths": released_paths,
            "remaining_claim": remaining_claim,
            "remaining_claim_count": remaining_claim_count,
        }),
        success_meta(BackendKind::Session, 0.92),
    ))
}
