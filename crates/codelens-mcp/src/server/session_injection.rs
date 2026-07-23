//! Session metadata injection for HTTP requests.
//!
//! Extracts session state from the SessionStore and injects `_session_*` fields
//! into tool call arguments, tools/list params, and resources/read params.
//! This centralizes the 3 injection paths that were duplicated in transport_http.rs.

use crate::AppState;
use crate::protocol::JsonRpcRequest;
use crate::server::session::{RequestProjectPin, SessionStore};
use std::sync::Arc;

/// Inject session metadata into a tools/call request's arguments.
pub(super) fn inject_tool_call_session(
    request: &mut JsonRpcRequest,
    session_id: &str,
    store: &SessionStore,
    project_header: Option<&str>,
) -> Option<RequestProjectPin> {
    let snapshot = store.client_metadata_for_project_header(session_id, project_header)?;
    let metadata = snapshot.metadata;
    let project_pin = snapshot.project_pin;
    let Some(params) = request.params.as_mut().and_then(|v| v.as_object_mut()) else {
        return Some(project_pin);
    };
    let arguments = params
        .entry("arguments".to_owned())
        .or_insert_with(|| serde_json::json!({}));
    let Some(args) = arguments.as_object_mut() else {
        return Some(project_pin);
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
        "_session_host_context".to_owned(),
        serde_json::json!(metadata.host_context),
    );
    args.insert(
        "_session_deferred_tool_loading".to_owned(),
        serde_json::json!(metadata.deferred_tool_loading),
    );
    args.insert(
        "_session_project_path".to_owned(),
        serde_json::json!(metadata.project_path),
    );
    args.insert(
        "_session_project_binding_source".to_owned(),
        serde_json::json!(metadata.project_binding_source.as_str()),
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
    args.insert(
        "_session_available_mcp_servers".to_owned(),
        serde_json::json!(metadata.available_mcp_servers),
    );
    args.insert(
        "_session_available_mcp_tools".to_owned(),
        serde_json::json!(metadata.available_mcp_tools),
    );
    args.insert(
        "_session_skill_roots".to_owned(),
        serde_json::json!(metadata.skill_roots),
    );
    args.insert(
        "_session_memory_roots".to_owned(),
        serde_json::json!(metadata.memory_roots),
    );
    args.insert(
        "_session_host_setting_keys".to_owned(),
        serde_json::json!(metadata.host_setting_keys),
    );
    args.insert(
        "_session_harness_profile".to_owned(),
        serde_json::json!(metadata.harness_profile),
    );
    args.insert(
        "_session_host_capabilities".to_owned(),
        serde_json::json!(metadata.host_capabilities),
    );
    Some(project_pin)
}

/// Update deferred loading state and inject session metadata for tools/list.
pub(super) fn inject_tools_list_session(
    request: &mut JsonRpcRequest,
    session_id: &str,
    store: &SessionStore,
    state: &Arc<AppState>,
    project_header: Option<&str>,
) -> Option<RequestProjectPin> {
    let session = store.get(session_id)?;
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

    let snapshot = store.client_metadata_for_project_header(session_id, project_header)?;
    inject_deferred_params(request, session_id, &snapshot.metadata);
    Some(snapshot.project_pin)
}

/// Update deferred loading state and inject session metadata for resources/read.
pub(super) fn inject_resources_read_session(
    request: &mut JsonRpcRequest,
    session_id: &str,
    store: &SessionStore,
    state: &Arc<AppState>,
    project_header: Option<&str>,
) -> Option<RequestProjectPin> {
    let session = store.get(session_id)?;
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

    let snapshot = store.client_metadata_for_project_header(session_id, project_header)?;
    inject_deferred_params(request, session_id, &snapshot.metadata);
    Some(snapshot.project_pin)
}

// ── Shared helpers ──────────────────────────────────────────────────────

fn update_deferred_state(
    session: &crate::server::session::SessionState,
    _state: &Arc<AppState>,
    requested_namespace: Option<&str>,
    requested_tier: Option<&str>,
    full_listing: bool,
) {
    if full_listing {
        session.enable_full_tool_exposure();
    } else if let Some(namespace) = requested_namespace
        && crate::tool_defs::visible_namespaces(session.surface()).contains(&namespace)
    {
        session.record_loaded_namespace(namespace);
        for tool in crate::tool_defs::visible_tools(session.surface())
            .into_iter()
            .filter(|tool| crate::tool_defs::tool_namespace(tool.name) == namespace)
        {
            session.record_loaded_tier(crate::tool_defs::tool_tier_label(tool.name));
        }
    }
    if !full_listing
        && let Some(tier) = requested_tier
        && crate::tool_defs::parse_tier_label(tier).is_some()
    {
        session.record_loaded_tier(tier);
    }
}

fn inject_deferred_params(
    request: &mut JsonRpcRequest,
    session_id: &str,
    metadata: &crate::server::session::SessionClientMetadata,
) {
    let deferred_fields = serde_json::json!({
        "_session_id": session_id,
        "_session_requested_profile": metadata.requested_profile,
        "_session_client_name": metadata.client_name,
        "_session_client_version": metadata.client_version,
        "_session_host_context": metadata.host_context,
        "_session_deferred_tool_loading": metadata.deferred_tool_loading,
        "_session_project_path": metadata.project_path,
        "_session_project_binding_source": metadata.project_binding_source.as_str(),
        "_session_loaded_namespaces": metadata.loaded_namespaces,
        "_session_loaded_tiers": metadata.loaded_tiers,
        "_session_full_tool_exposure": metadata.full_tool_exposure,
        "_session_available_mcp_servers": metadata.available_mcp_servers,
        "_session_available_mcp_tools": metadata.available_mcp_tools,
        "_session_skill_roots": metadata.skill_roots,
        "_session_memory_roots": metadata.memory_roots,
        "_session_host_setting_keys": metadata.host_setting_keys,
        "_session_harness_profile": metadata.harness_profile,
        "_session_host_capabilities": metadata.host_capabilities,
    });

    if let Some(params) = request.params.as_mut().and_then(|v| v.as_object_mut()) {
        for (key, value) in deferred_fields.as_object().unwrap() {
            params.insert(key.clone(), value.clone());
        }
    } else {
        request.params = Some(deferred_fields);
    }
}
