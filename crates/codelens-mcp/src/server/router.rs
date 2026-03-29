use crate::dispatch::dispatch_tool;
use crate::prompts::{get_prompt, prompts};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::resources::{read_resource, resources};
use crate::tool_defs::{tools, ToolPreset, BALANCED_EXCLUDES, MINIMAL_TOOLS};
use crate::AppState;
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
                }
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
            let current_preset = *state.preset();
            let filtered: Vec<_> = tools()
                .iter()
                .filter(|t| match current_preset {
                    ToolPreset::Full => true,
                    ToolPreset::Minimal => MINIMAL_TOOLS.contains(&t.name),
                    ToolPreset::Balanced => !BALANCED_EXCLUDES.contains(&t.name),
                })
                .collect();
            Some(JsonRpcResponse::result(
                request.id,
                json!({ "tools": filtered }),
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
