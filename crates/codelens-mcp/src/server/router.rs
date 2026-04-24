use crate::AppState;
use crate::dispatch::dispatch_tool;
use crate::prompts::{get_prompt, prompts};
use crate::protocol::{
    JsonRpcRequest, JsonRpcResponse, LATEST_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS,
};
use crate::resources::{read_resource, resources};
use crate::server::tools_list::build_tools_list_response;
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
    let compat_mode = state.compat_mode();
    if compat_mode.tools_only()
        && (request.method.starts_with("resources/") || request.method.starts_with("prompts/"))
    {
        return Some(JsonRpcResponse::error(
            request.id,
            -32601,
            format!("Method not found: {}", request.method),
        ));
    }

    match request.method.as_str() {
        // Notifications — silently accept, never respond
        "notifications/initialized"
        | "notifications/cancelled"
        | "notifications/progress"
        | "notifications/roots/list_changed"
        | "notifications/tools/list_changed"
        | "notifications/resources/list_changed"
        | "notifications/prompts/list_changed" => None,

        "initialize" => {
            // Per spec §lifecycle/version negotiation: echo the client's requested
            // protocol version when we support it, otherwise reply with our latest.
            // The client is then expected to disconnect if the returned version is
            // not acceptable.
            let requested = request
                .params
                .as_ref()
                .and_then(|params| params.get("protocolVersion"))
                .and_then(|value| value.as_str());
            let negotiated = match requested {
                Some(version) if SUPPORTED_PROTOCOL_VERSIONS.contains(&version) => version,
                _ => LATEST_PROTOCOL_VERSION,
            };
            let capabilities = if compat_mode.tools_only() {
                json!({
                    "tools": {
                        "listChanged": state.session_resume_supported()
                    }
                })
            } else {
                json!({
                    "tools": {
                        "listChanged": state.session_resume_supported()
                    },
                    "resources": { "listChanged": false },
                    "prompts": { "listChanged": false }
                })
            };
            Some(JsonRpcResponse::result(
                request.id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": capabilities,
                    "serverInfo": {
                        "name": "codelens-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": "CodeLens is a compressed context provider for agent harnesses. Prefer problem-first workflow entrypoints such as explore_codebase, review_architecture, plan_safe_refactor, trace_request_path, review_changes, cleanup_duplicate_logic, and semantic_code_review before expanding raw symbols or graph data. Legacy report tools remain available, but the workflow-first surface is the default path. Keep the visible context bounded, and use get_analysis_section or analysis resources only when you need one section in more detail. For longer reports, start_analysis_job and poll with get_analysis_job."
                }),
            ))
        }
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
                read_resource(state, uri, request.params.as_ref()),
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
            let result = build_tools_list_response(state, &request, compat_mode.tools_only());
            Some(JsonRpcResponse::result(request.id, result))
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
