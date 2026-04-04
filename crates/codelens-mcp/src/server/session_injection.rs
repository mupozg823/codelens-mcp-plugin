//! Session metadata injection for HTTP requests.
//!
//! Extracts session state from the SessionStore and injects `_session_*` fields
//! into tool call arguments, tools/list params, and resources/read params.
//! This centralizes the 3 injection paths that were duplicated in transport_http.rs.

use crate::protocol::JsonRpcRequest;
use crate::server::session::SessionStore;
use crate::AppState;
use std::sync::Arc;

/// Inject session metadata into a tools/call request's arguments.
pub(super) fn inject_tool_call_session(
    request: &mut JsonRpcRequest,
    session_id: &str,
    store: &SessionStore,
) {
    let session = match store.get(session_id) {
        Some(s) => s,
        None => return,
    };
    let metadata = session.client_metadata();
    let Some(params) = request.params.as_mut().and_then(|v| v.as_object_mut()) else {
        return;
    };
    let arguments = params
        .entry("arguments".to_owned())
        .or_insert_with(|| serde_json::json!({}));
    let Some(args) = arguments.as_object_mut() else {
        return;
    };
    args.insert("_session_id".to_owned(), serde_json::json!(session_id));
    args.insert(
        "_session_trusted_client".to_owned(),
        serde_json::json!(metadata.trusted_client),
    );
    args.insert(
        "_session_requested_profile".to_owned(),
        serde_json::json!(metadata.requested_profile),
    );
    args.insert(
        "_session_client_name".to_owned(),
        serde_json::json!(metadata.client_name),
    );
    args.insert(
        "_session_client_version".to_owned(),
        serde_json::json!(metadata.client_version),
    );
    args.insert(
        "_session_deferred_tool_loading".to_owned(),
        serde_json::json!(metadata.deferred_tool_loading),
    );
    args.insert(
        "_session_loaded_namespaces".to_owned(),
        serde_json::json!(metadata.loaded_namespaces),
    );
    args.insert(
        "_session_loaded_tiers".to_owned(),
        serde_json::json!(metadata.loaded_tiers),
    );
    args.insert(
        "_session_full_tool_exposure".to_owned(),
        serde_json::json!(metadata.full_tool_exposure),
    );
}

/// Update deferred loading state and inject session metadata for tools/list.
pub(super) fn inject_tools_list_session(
    request: &mut JsonRpcRequest,
    session_id: &str,
    store: &SessionStore,
    state: &Arc<AppState>,
) {
    let session = match store.get(session_id) {
        Some(s) => s,
        None => return,
    };
    let requested_namespace = request
        .params
        .as_ref()
        .and_then(|v| v.get("namespace"))
        .and_then(|v| v.as_str());
    let requested_tier = request
        .params
        .as_ref()
        .and_then(|v| v.get("tier"))
        .and_then(|v| v.as_str());
    let full_listing = request
        .params
        .as_ref()
        .and_then(|v| v.get("full"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    update_deferred_state(
        &session,
        state,
        requested_namespace,
        requested_tier,
        full_listing,
    );

    inject_deferred_params(request, &session);
}

/// Update deferred loading state and inject session metadata for resources/read.
pub(super) fn inject_resources_read_session(
    request: &mut JsonRpcRequest,
    session_id: &str,
    store: &SessionStore,
    state: &Arc<AppState>,
) {
    let session = match store.get(session_id) {
        Some(s) => s,
        None => return,
    };
    let uri = request
        .params
        .as_ref()
        .and_then(|v| v.get("uri"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if matches!(uri, "codelens://tools/list" | "codelens://tools/list/full") {
        let requested_namespace = request
            .params
            .as_ref()
            .and_then(|v| v.get("namespace"))
            .and_then(|v| v.as_str());
        let requested_tier = request
            .params
            .as_ref()
            .and_then(|v| v.get("tier"))
            .and_then(|v| v.as_str());
        let full_listing = uri == "codelens://tools/list/full"
            || request
                .params
                .as_ref()
                .and_then(|v| v.get("full"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

        update_deferred_state(
            &session,
            state,
            requested_namespace,
            requested_tier,
            full_listing,
        );
    }

    inject_deferred_params(request, &session);
}

// ── Shared helpers ──────────────────────────────────────────────────────

fn update_deferred_state(
    session: &crate::server::session::SessionState,
    state: &Arc<AppState>,
    requested_namespace: Option<&str>,
    requested_tier: Option<&str>,
    full_listing: bool,
) {
    if full_listing {
        session.enable_full_tool_exposure();
    } else if let Some(namespace) = requested_namespace {
        if crate::tool_defs::visible_namespaces(*state.surface()).contains(&namespace) {
            session.record_loaded_namespace(namespace);
        }
    }
    if !full_listing {
        if let Some(tier) = requested_tier {
            if crate::tool_defs::parse_tier_label(tier).is_some() {
                session.record_loaded_tier(tier);
            }
        }
    }
}

fn inject_deferred_params(
    request: &mut JsonRpcRequest,
    session: &crate::server::session::SessionState,
) {
    let metadata = session.client_metadata();
    let deferred_fields = serde_json::json!({
        "_session_deferred_tool_loading": metadata.deferred_tool_loading,
        "_session_loaded_namespaces": metadata.loaded_namespaces,
        "_session_loaded_tiers": metadata.loaded_tiers,
        "_session_full_tool_exposure": metadata.full_tool_exposure,
    });

    if let Some(params) = request.params.as_mut().and_then(|v| v.as_object_mut()) {
        for (key, value) in deferred_fields.as_object().unwrap() {
            params.insert(key.clone(), value.clone());
        }
    } else {
        request.params = Some(deferred_fields);
    }
}
