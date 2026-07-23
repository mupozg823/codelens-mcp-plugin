use crate::AppState;
use crate::error::CodeLensError;
use crate::tool_defs::{ToolProfile, ToolSurface, preferred_namespaces, preferred_tier_labels};
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::ResourceRequestContext;

pub(crate) fn build_http_session_payload(
    state: &AppState,
    request: &ResourceRequestContext,
) -> Result<Value, CodeLensError> {
    let surface = state.execution_surface(&request.session);
    let runtime_health = collect_runtime_health_snapshot(state, surface);
    let coordination = state.coordination_counts_for_session(&request.session)?;
    // T3: the namespace/tier gates only apply when this session is actually
    // operating in deferred-loading mode. Reporting them as unconditionally
    // `true` contradicted `visible_tools.deferred_loading_active` (which is
    // `request.deferred_loading_active()`) for full-exposure / non-deferred
    // sessions. Tie them to the same signal so the two never disagree; a
    // non-deferred session now reports `false` (no listing requirement).
    // `deferred_loading_supported` (a server capability) and the trust /
    // rename-preflight hooks are orthogonal to deferred loading and stay fixed.
    let deferred_active = request.deferred_loading_active();
    Ok(json!({
        "enabled": state.session_resume_supported(),
        "active_sessions": state.active_session_count(),
        "active_coordination_agents": coordination.active_agents,
        "active_coordination_claims": coordination.active_claims,
        "timeout_seconds": state.session_timeout_seconds(),
        "resume_supported": state.session_resume_supported(),
        "daemon_mode": state.daemon_mode().as_str(),
        "client_profile": request.client_profile.as_str(),
        "client_name": request.session.client_name,
        "active_surface": surface.as_label(),
        "semantic_search_status": runtime_health.semantic_status.status_key(),
        "indexed_files": runtime_health.indexed_files(),
        "supported_files": runtime_health.supported_files(),
        "stale_files": runtime_health.stale_files(),
        "daemon_binary_drift": runtime_health.daemon_binary_drift,
        "health_summary": runtime_health.health_summary,
        "deferred_loading_supported": true,
        "default_deferred_tool_loading": request.client_profile.default_deferred_tool_loading(),
        "default_tools_list_contract_mode": request.client_profile.default_tool_contract_mode(),
        "loaded_namespaces": request.session.loaded_namespaces,
        "loaded_tiers": request.session.loaded_tiers,
        "full_tool_exposure": request.session.full_tool_exposure,
        "deferred_namespace_gate": deferred_active,
        "deferred_tier_gate": deferred_active,
        "preferred_namespaces": preferred_namespaces(surface),
        "preferred_tiers": preferred_tier_labels(surface),
        "trusted_client_hook": true,
        "mutation_requires_trusted_client": matches!(
            state.daemon_mode(),
            crate::state::RuntimeDaemonMode::MutationEnabled
        ),
        "mutation_preflight_required": matches!(
            surface,
            ToolSurface::Profile(ToolProfile::RefactorFull)
        ),
        "preflight_ttl_seconds": state.preflight_ttl_seconds(),
        "rename_requires_symbol_preflight": true,
        "requires_namespace_listing_before_tool_call": deferred_active,
        "requires_tier_listing_before_tool_call": deferred_active
    }))
}

pub(crate) fn build_agent_activity_payload(
    state: &AppState,
    request: &ResourceRequestContext,
) -> Result<Value, CodeLensError> {
    let snapshot = state.coordination_snapshot_for_session(&request.session)?;
    let mut session_ids = BTreeSet::new();
    for agent in &snapshot.agents {
        session_ids.insert(agent.session_id.clone());
    }
    for claim in &snapshot.claims {
        session_ids.insert(claim.session_id.clone());
    }

    #[cfg(feature = "http")]
    let http_activity = state
        .session_store
        .as_ref()
        .map(|store| {
            store
                .activity_snapshots()
                .into_iter()
                .map(|snapshot| (snapshot.id.clone(), snapshot))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let agents_by_session = snapshot
        .agents
        .iter()
        .map(|entry| (entry.session_id.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let claims_by_session = snapshot
        .claims
        .iter()
        .map(|entry| (entry.session_id.clone(), entry))
        .collect::<BTreeMap<_, _>>();

    let sessions = session_ids
        .into_iter()
        .map(|session_id| {
            let agent = agents_by_session.get(&session_id).copied();
            let claim = claims_by_session.get(&session_id).copied();
            #[cfg(feature = "http")]
            let (recent_tools, recent_file_paths, client_name, requested_profile) = http_activity
                .get(&session_id)
                .map(|entry| {
                    (
                        entry.recent_tools.clone(),
                        entry.recent_files.clone(),
                        entry.client_name.clone(),
                        entry.requested_profile.clone(),
                    )
                })
                .unwrap_or_default();
            #[cfg(not(feature = "http"))]
            let (recent_tools, recent_file_paths, client_name, requested_profile): (
                Vec<String>,
                Vec<String>,
                Option<String>,
                Option<String>,
            ) = Default::default();
            json!({
                "session_id": session_id,
                "agent_name": agent
                    .map(|entry| entry.agent_name.clone())
                    .or_else(|| claim.map(|entry| entry.agent_name.clone()))
                    .unwrap_or_else(|| "unregistered".to_owned()),
                "branch": agent
                    .map(|entry| entry.branch.clone())
                    .or_else(|| claim.map(|entry| entry.branch.clone()))
                    .unwrap_or_default(),
                "worktree": agent
                    .map(|entry| entry.worktree.clone())
                    .or_else(|| claim.map(|entry| entry.worktree.clone()))
                    .unwrap_or_default(),
                "intent": agent
                    .map(|entry| entry.intent.clone())
                    .unwrap_or_else(|| "unregistered".to_owned()),
                "registered": agent.is_some(),
                "expires_at": agent
                    .map(|entry| entry.expires_at)
                    .or_else(|| claim.map(|entry| entry.expires_at))
                    .unwrap_or_default(),
                "recent_tools": recent_tools,
                "recent_file_paths": recent_file_paths,
                "client_name": client_name,
                "requested_profile": requested_profile,
                "claims": claim
                    .map(|entry| vec![json!(entry)])
                    .unwrap_or_default(),
                "claim_count": usize::from(claim.is_some()),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "project_scope": state.project_scope_for_session(&request.session),
        "active_agents": snapshot.counts.active_agents,
        "active_claims": snapshot.counts.active_claims,
        "http_attached_sessions": state.active_session_count(),
        "sessions": sessions,
        "claims": snapshot.claims,
    }))
}
