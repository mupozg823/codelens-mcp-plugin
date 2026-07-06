use crate::client_profile::ClientProfile;
use serde_json::Value;

/// Resource-level request context. Embeds `SessionRequestContext` for
/// fields parsed from `_session_*` keys; only fields that are derived
/// (client_profile) or sourced from the URI/params (requested_namespace,
/// requested_tier, full_listing, deferred_loading_requested) live here.
///
/// Read shared fields via `request.session.*`:
///   loaded_namespaces, loaded_tiers, full_tool_exposure, client_name.
#[derive(Clone, Debug)]
pub(crate) struct ResourceRequestContext {
    pub(crate) session: crate::session_context::SessionRequestContext,
    pub(crate) deferred_loading_requested: bool,
    pub(crate) requested_namespace: Option<String>,
    pub(crate) requested_tier: Option<String>,
    pub(crate) full_listing: bool,
    pub(crate) client_profile: ClientProfile,
}

impl Default for ResourceRequestContext {
    fn default() -> Self {
        Self {
            session: crate::session_context::SessionRequestContext::default(),
            deferred_loading_requested: false,
            requested_namespace: None,
            requested_tier: None,
            full_listing: false,
            client_profile: ClientProfile::Generic,
        }
    }
}

impl ResourceRequestContext {
    pub(crate) fn from_request(uri: &str, params: Option<&Value>) -> Self {
        let session = params
            .map(crate::session_context::SessionRequestContext::from_json)
            .unwrap_or_default();
        let host_context = string_param(params, "host_context");
        let client_profile =
            ClientProfile::detect_request(session.client_name.as_deref(), host_context.as_deref());
        let deferred_loading_requested = params
            .and_then(|value| value.get("_session_deferred_tool_loading"))
            .and_then(|value| value.as_bool())
            .or_else(|| client_profile.default_deferred_tool_loading())
            .unwrap_or(false);
        Self {
            session,
            deferred_loading_requested,
            requested_namespace: string_param(params, "namespace"),
            requested_tier: string_param(params, "tier"),
            full_listing: uri == "codelens://tools/list/full" || bool_param(params, "full"),
            client_profile,
        }
    }

    pub(crate) fn deferred_loading_active(&self) -> bool {
        self.deferred_loading_requested
            && self.requested_namespace.is_none()
            && self.requested_tier.is_none()
            && !self.full_listing
            && !self.session.full_tool_exposure
    }

    pub(crate) fn tool_contract_mode(&self) -> &'static str {
        self.client_profile.default_tool_contract_mode()
    }

    pub(crate) fn lean_tool_contract(&self) -> bool {
        self.tool_contract_mode() == "lean"
            && !self.full_listing
            && !self.session.full_tool_exposure
    }

    pub(crate) fn default_listing_requested(&self) -> bool {
        !self.full_listing
            && !self.session.full_tool_exposure
            && !self.deferred_loading_active()
            && self.requested_namespace.is_none()
            && self.requested_tier.is_none()
    }
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
