use crate::client_profile::ClientProfile;
use crate::protocol::Tool;
use crate::tool_defs::{
    is_deferred_control_tool, preferred_bootstrap_tools, preferred_namespaces,
    preferred_tier_labels, tool_namespace, tool_tier_label, visible_namespaces, visible_tiers,
    visible_tools, ToolProfile, ToolSurface,
};
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use crate::AppState;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(crate) struct ResourceRequestContext {
    pub(crate) session: crate::session_context::SessionRequestContext,
    pub(crate) deferred_loading_requested: bool,
    pub(crate) loaded_namespaces: Vec<String>,
    pub(crate) loaded_tiers: Vec<String>,
    pub(crate) full_tool_exposure: bool,
    pub(crate) requested_namespace: Option<String>,
    pub(crate) requested_tier: Option<String>,
    pub(crate) full_listing: bool,
    pub(crate) client_profile: ClientProfile,
    pub(crate) client_name: Option<String>,
}

impl Default for ResourceRequestContext {
    fn default() -> Self {
        Self {
            session: crate::session_context::SessionRequestContext::default(),
            deferred_loading_requested: false,
            loaded_namespaces: Vec::new(),
            loaded_tiers: Vec::new(),
            full_tool_exposure: false,
            requested_namespace: None,
            requested_tier: None,
            full_listing: false,
            client_profile: ClientProfile::Generic,
            client_name: None,
        }
    }
}

impl ResourceRequestContext {
    pub(crate) fn from_request(uri: &str, params: Option<&Value>) -> Self {
        let session = params
            .map(crate::session_context::SessionRequestContext::from_json)
            .unwrap_or_default();
        let loaded_namespaces = session.loaded_namespaces.clone();
        let loaded_tiers = session.loaded_tiers.clone();
        let full_tool_exposure = session.full_tool_exposure;
        let client_name = session.client_name.clone();
        let client_profile = session
            .client_name
            .as_deref()
            .map(|name| ClientProfile::detect(Some(name)))
            .unwrap_or(ClientProfile::Generic);
        let deferred_loading_requested = params
            .and_then(|value| value.get("_session_deferred_tool_loading"))
            .and_then(|value| value.as_bool())
            .or_else(|| client_profile.default_deferred_tool_loading())
            .unwrap_or(false);
        Self {
            session,
            deferred_loading_requested,
            loaded_namespaces,
            loaded_tiers,
            full_tool_exposure,
            requested_namespace: string_param(params, "namespace"),
            requested_tier: string_param(params, "tier"),
            full_listing: uri == "codelens://tools/list/full" || bool_param(params, "full"),
            client_profile,
            client_name,
        }
    }

    pub(crate) fn deferred_loading_active(&self) -> bool {
        self.deferred_loading_requested
            && self.requested_namespace.is_none()
            && self.requested_tier.is_none()
            && !self.full_listing
            && !self.full_tool_exposure
    }

    pub(crate) fn tool_contract_mode(&self) -> &'static str {
        self.client_profile.default_tool_contract_mode()
    }

    pub(crate) fn lean_tool_contract(&self) -> bool {
        self.tool_contract_mode() == "lean" && !self.full_listing && !self.full_tool_exposure
    }
}

pub(crate) struct VisibleToolContext {
    pub(crate) tools: Vec<&'static Tool>,
    pub(crate) total_tool_count: usize,
    pub(crate) all_namespaces: Vec<&'static str>,
    pub(crate) all_tiers: Vec<&'static str>,
    pub(crate) preferred_namespaces: Vec<&'static str>,
    pub(crate) preferred_tiers: Vec<&'static str>,
    pub(crate) loaded_namespaces: Vec<String>,
    pub(crate) loaded_tiers: Vec<String>,
    pub(crate) effective_namespaces: Vec<String>,
    pub(crate) effective_tiers: Vec<String>,
    pub(crate) selected_namespace: Option<String>,
    pub(crate) selected_tier: Option<String>,
    pub(crate) deferred_loading_active: bool,
    pub(crate) full_tool_exposure: bool,
}

pub(crate) fn build_visible_tool_context(
    state: &AppState,
    request: &ResourceRequestContext,
) -> VisibleToolContext {
    let surface = state.execution_surface(&request.session);
    let all_tools = visible_tools(surface);
    let preferred = preferred_namespaces(surface);
    let preferred_bootstrap = preferred_bootstrap_tools(surface);
    let preferred_tiers = preferred_tier_labels(surface);
    let has_loaded_expansions =
        !request.loaded_namespaces.is_empty() || !request.loaded_tiers.is_empty();
    let mut tools = all_tools
        .iter()
        .copied()
        .filter(|tool| match request.requested_namespace.as_deref() {
            _ if request.deferred_loading_active() && is_deferred_control_tool(tool.name) => true,
            Some(namespace) => tool_namespace(tool.name) == namespace,
            None if request.deferred_loading_active() => {
                if is_deferred_control_tool(tool.name) {
                    return true;
                }
                let namespace = tool_namespace(tool.name);
                let tier = tool_tier_label(tool.name);
                preferred.contains(&namespace)
                    || request.loaded_tiers.iter().any(|value| value == tier)
                    || request
                        .loaded_namespaces
                        .iter()
                        .any(|value| value == namespace)
            }
            None => true,
        })
        .filter(|tool| match request.requested_tier.as_deref() {
            _ if request.deferred_loading_active() && is_deferred_control_tool(tool.name) => true,
            Some(tier) => tool_tier_label(tool.name) == tier,
            None if request.deferred_loading_active() => {
                if is_deferred_control_tool(tool.name) {
                    return true;
                }
                let namespace = tool_namespace(tool.name);
                let tier = tool_tier_label(tool.name);
                preferred_tiers.contains(&tier)
                    || request
                        .loaded_namespaces
                        .iter()
                        .any(|value| value == namespace)
                    || request.loaded_tiers.iter().any(|value| value == tier)
            }
            None => true,
        })
        .filter(|tool| match preferred_bootstrap {
            _ if request.deferred_loading_active() && is_deferred_control_tool(tool.name) => true,
            Some(tool_names) if request.deferred_loading_active() && !has_loaded_expansions => {
                tool_names.contains(&tool.name)
            }
            _ => true,
        })
        .collect::<Vec<_>>();
    if request.deferred_loading_active() && has_loaded_expansions {
        tools.sort_by_key(|tool| {
            let namespace = tool_namespace(tool.name);
            let tier = tool_tier_label(tool.name);
            let namespace_rank = if request
                .loaded_namespaces
                .iter()
                .any(|value| value == namespace)
            {
                0usize
            } else {
                1
            };
            let tier_rank = if request.loaded_tiers.iter().any(|value| value == tier) {
                0usize
            } else {
                1
            };
            let control_rank = if is_deferred_control_tool(tool.name) {
                2usize
            } else {
                0
            };
            (namespace_rank, tier_rank, control_rank)
        });
    }

    let mut effective_namespaces = preferred
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    for namespace in &request.loaded_namespaces {
        if !effective_namespaces.iter().any(|value| value == namespace) {
            effective_namespaces.push(namespace.clone());
        }
    }
    effective_namespaces.sort();

    let mut effective_tiers = preferred_tiers
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    for tier in &request.loaded_tiers {
        if !effective_tiers.iter().any(|value| value == tier) {
            effective_tiers.push(tier.clone());
        }
    }
    effective_tiers.sort();

    VisibleToolContext {
        tools,
        total_tool_count: all_tools.len(),
        all_namespaces: visible_namespaces(surface),
        all_tiers: visible_tiers(surface),
        preferred_namespaces: preferred,
        preferred_tiers,
        loaded_namespaces: request.loaded_namespaces.clone(),
        loaded_tiers: request.loaded_tiers.clone(),
        effective_namespaces,
        effective_tiers,
        selected_namespace: request.requested_namespace.clone(),
        selected_tier: request.requested_tier.clone(),
        deferred_loading_active: request.deferred_loading_active(),
        full_tool_exposure: request.full_tool_exposure,
    }
}

pub(crate) fn build_http_session_payload(
    state: &AppState,
    request: &ResourceRequestContext,
) -> Value {
    let surface = state.execution_surface(&request.session);
    let runtime_health = collect_runtime_health_snapshot(state, surface);
    let coordination = state.coordination_counts_for_session(&request.session);
    json!({
        "enabled": state.session_resume_supported(),
        "active_sessions": state.active_session_count(),
        "active_coordination_agents": coordination.active_agents,
        "active_coordination_claims": coordination.active_claims,
        "timeout_seconds": state.session_timeout_seconds(),
        "resume_supported": state.session_resume_supported(),
        "daemon_mode": state.daemon_mode().as_str(),
        "client_profile": request.client_profile.as_str(),
        "client_name": request.client_name,
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
        "loaded_namespaces": request.loaded_namespaces,
        "loaded_tiers": request.loaded_tiers,
        "full_tool_exposure": request.full_tool_exposure,
        "deferred_namespace_gate": true,
        "deferred_tier_gate": true,
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
        "requires_namespace_listing_before_tool_call": true,
        "requires_tier_listing_before_tool_call": true
    })
}

pub(crate) fn build_agent_activity_payload(
    state: &AppState,
    request: &ResourceRequestContext,
) -> Value {
    let snapshot = state.coordination_snapshot_for_session(&request.session);
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

    json!({
        "project_scope": state.project_scope_for_session(&request.session),
        "active_agents": snapshot.counts.active_agents,
        "active_claims": snapshot.counts.active_claims,
        "http_attached_sessions": state.active_session_count(),
        "sessions": sessions,
        "claims": snapshot.claims,
    })
}

fn bool_param(params: Option<&Value>, key: &str) -> bool {
    params
        .and_then(|params| params.get(key))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn string_param(params: Option<&Value>, key: &str) -> Option<String> {
    params
        .and_then(|params| params.get(key))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}
