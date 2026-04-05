use crate::AppState;
use crate::protocol::Tool;
use crate::tool_defs::{
    ToolProfile, ToolSurface, preferred_namespaces, preferred_tier_labels, tool_namespace,
    tool_tier_label, visible_namespaces, visible_tiers, visible_tools,
};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default)]
pub(crate) struct ResourceRequestContext {
    pub(crate) deferred_loading_requested: bool,
    pub(crate) loaded_namespaces: Vec<String>,
    pub(crate) loaded_tiers: Vec<String>,
    pub(crate) full_tool_exposure: bool,
    pub(crate) requested_namespace: Option<String>,
    pub(crate) requested_tier: Option<String>,
    pub(crate) full_listing: bool,
}

impl ResourceRequestContext {
    pub(crate) fn from_request(uri: &str, params: Option<&Value>) -> Self {
        let session = params
            .map(crate::session_context::SessionRequestContext::from_json)
            .unwrap_or_default();
        Self {
            deferred_loading_requested: session.deferred_loading,
            loaded_namespaces: session.loaded_namespaces,
            loaded_tiers: session.loaded_tiers,
            full_tool_exposure: session.full_tool_exposure,
            requested_namespace: string_param(params, "namespace"),
            requested_tier: string_param(params, "tier"),
            full_listing: uri == "codelens://tools/list/full" || bool_param(params, "full"),
        }
    }

    pub(crate) fn deferred_loading_active(&self) -> bool {
        self.deferred_loading_requested
            && self.requested_namespace.is_none()
            && self.requested_tier.is_none()
            && !self.full_listing
            && !self.full_tool_exposure
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
    let surface = *state.surface();
    let all_tools = visible_tools(surface);
    let preferred = preferred_namespaces(surface);
    let preferred_tiers = preferred_tier_labels(surface);
    let tools = all_tools
        .iter()
        .copied()
        .filter(|tool| match request.requested_namespace.as_deref() {
            Some(namespace) => tool_namespace(tool.name) == namespace,
            None if request.deferred_loading_active() => {
                let namespace = tool_namespace(tool.name);
                preferred.contains(&namespace)
                    || request
                        .loaded_namespaces
                        .iter()
                        .any(|value| value == namespace)
            }
            None => true,
        })
        .filter(|tool| match request.requested_tier.as_deref() {
            Some(tier) => tool_tier_label(tool.name) == tier,
            None if request.deferred_loading_active() => {
                let tier = tool_tier_label(tool.name);
                preferred_tiers.contains(&tier)
                    || request.loaded_tiers.iter().any(|value| value == tier)
            }
            None => true,
        })
        .collect::<Vec<_>>();

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
    let surface = *state.surface();
    json!({
        "enabled": state.session_resume_supported(),
        "active_sessions": state.active_session_count(),
        "timeout_seconds": state.session_timeout_seconds(),
        "resume_supported": state.session_resume_supported(),
        "daemon_mode": state.daemon_mode().as_str(),
        "active_surface": surface.as_label(),
        "deferred_loading_supported": true,
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
