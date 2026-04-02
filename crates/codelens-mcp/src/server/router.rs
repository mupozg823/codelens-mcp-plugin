use crate::AppState;
use crate::dispatch::dispatch_tool;
use crate::prompts::{get_prompt, prompts};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::resources::{read_resource, resources};
use crate::tool_defs::{
    preferred_namespaces, visible_namespaces, visible_tools, visible_tools_for_namespace,
};
use serde_json::json;

pub(crate) fn handle_request(state: &AppState, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
    if request.jsonrpc != "2.0" {
        return Some(JsonRpcResponse::error(
            request.id,
            -32600,
            "Unsupported jsonrpc version",
        ));
    }

    // JSON-RPC 2.0: notifications (no id) MUST NOT receive a response
    let is_notification = request.id.is_none();

    match request.method.as_str() {
        // Notifications — silently accept, never respond
        "notifications/initialized"
        | "notifications/cancelled"
        | "notifications/progress"
        | "notifications/roots/list_changed" => None,

        "initialize" => Some(JsonRpcResponse::result(
            request.id,
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": {},
                    "resources": { "listChanged": false },
                    "prompts": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "codelens-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "CodeLens is a compressed context provider for agent harnesses. Prefer high-level workflow tools such as analyze_change_request, impact_report, diff_aware_references, module_boundary_report, refactor_safety_report, dead_code_report, and safe_rename_report before expanding raw symbols or graph data. Keep the visible context bounded, and use get_analysis_section or analysis resources only when you need one section in more detail. For longer reports, start_analysis_job and poll with get_analysis_job."
            }),
        )),
        "resources/list" => Some(JsonRpcResponse::result(
            request.id,
            json!({ "resources": resources(state) }),
        )),
        "resources/read" => {
            let uri = request
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Some(JsonRpcResponse::result(
                request.id,
                read_resource(state, uri),
            ))
        }
        "prompts/list" => Some(JsonRpcResponse::result(
            request.id,
            json!({ "prompts": prompts() }),
        )),
        "prompts/get" => {
            let name = request
                .params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = request
                .params
                .as_ref()
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));
            Some(JsonRpcResponse::result(
                request.id,
                get_prompt(state, name, &args),
            ))
        }
        "tools/list" => {
            let surface = *state.surface();
            let all_tools = visible_tools(surface);
            let all_namespaces = visible_namespaces(surface);
            let requested_namespace = request
                .params
                .as_ref()
                .and_then(|params| params.get("namespace"))
                .and_then(|value| value.as_str());
            let full_listing = request
                .params
                .as_ref()
                .and_then(|params| params.get("full"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let deferred_loading_requested = request
                .params
                .as_ref()
                .and_then(|params| params.get("_session_deferred_tool_loading"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let preferred_namespaces = preferred_namespaces(surface);
            let filtered = match requested_namespace {
                Some(namespace) => visible_tools_for_namespace(surface, namespace),
                None if deferred_loading_requested && !full_listing => all_tools
                    .iter()
                    .copied()
                    .filter(|tool| preferred_namespaces.contains(&crate::tool_defs::tool_namespace(tool.name)))
                    .collect(),
                None => all_tools.clone(),
            };
            let token_estimate = serde_json::to_string(&filtered)
                .map(|body| crate::tools::estimate_tokens(&body))
                .unwrap_or(0);
            state.metrics().record_call_with_tokens(
                "tools/list",
                0,
                true,
                token_estimate,
                surface.as_label(),
                false,
            );
            Some(JsonRpcResponse::result(
                request.id,
                json!({
                    "active_surface": surface.as_label(),
                    "visible_namespaces": all_namespaces,
                    "preferred_namespaces": preferred_namespaces,
                    "selected_namespace": requested_namespace,
                    "deferred_loading_active": deferred_loading_requested && requested_namespace.is_none() && !full_listing,
                    "full_listing": full_listing,
                    "tool_count": filtered.len(),
                    "tool_count_total": all_tools.len(),
                    "tools": filtered
                }),
            ))
        }
        "tools/call" => match request.params {
            Some(params) => Some(dispatch_tool(state, request.id, params)),
            None => Some(JsonRpcResponse::error(request.id, -32602, "Missing params")),
        },
        // Unknown notification — silently ignore per JSON-RPC 2.0
        _ if is_notification => None,
        method => Some(JsonRpcResponse::error(
            request.id,
            -32601,
            format!("Method not found: {method}"),
        )),
    }
}
