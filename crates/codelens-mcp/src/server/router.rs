use crate::AppState;
use crate::dispatch::dispatch_tool;
use crate::prompts::{get_prompt, prompts};
use crate::protocol::{
    JsonRpcRequest, JsonRpcResponse, LATEST_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS,
};
use crate::resources::{read_resource, resources};
use crate::server::tools_list::build_tools_list_response;
use serde_json::json;

/// How the server tells a client to bind. A daemon is shared, so an unbound
/// session reads its default project; once bound, binding again only costs a
/// round trip (an evaluation run saw 28 redundant `prepare_harness_session`
/// calls on sessions the URL had already bound).
const UNBOUND_BINDING_GUIDANCE: &str = "FIRST CALL of every session: prepare_harness_session with project=<absolute path of YOUR current workspace> — a shared CodeLens daemon otherwise targets its own default project and every read silently comes from the wrong repo. (Hosts can bind automatically via the x-codelens-project HTTP header, a `?project=` query on the endpoint URL, or an initialize `project` param; if a response carries a `project_binding` hint, the session is still unbound.) Project binding reselects the project-local symbol index, graph/LSP runtime, analysis cache, and SCIP precise index; switching projects auto-builds the symbol index on first use.";

const WORKFLOW_GUIDANCE: &str = "After binding, prefer problem-first workflow entrypoints such as explore_codebase, review_architecture, plan_safe_refactor, trace_request_path, review_changes, and cleanup_duplicate_logic before expanding raw symbols or graph data. Keep the visible context bounded, and use get_analysis_section or analysis resources only when you need one section in more detail. For longer reports, start_analysis_job and poll with get_analysis_job.";

pub(crate) fn unbound_session_instructions() -> String {
    format!("{UNBOUND_BINDING_GUIDANCE} {WORKFLOW_GUIDANCE}")
}

/// Instructions for a session the host bound at `initialize` (header, URL
/// query or `project` param): it must not be told to bind again.
#[cfg(feature = "http")]
pub(crate) fn bound_session_instructions(project_root: &str) -> String {
    format!(
        "This session is already bound to `{project_root}`, the directory the host was launched in. Call prepare_harness_session only to switch to a different project; a response carrying a `project_binding` hint means the session is unbound. Switching projects reselects the project-local symbol index, graph/LSP runtime, analysis cache, and SCIP precise index, and auto-builds the symbol index on first use. {WORKFLOW_GUIDANCE}"
    )
}

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
                    "instructions": unbound_session_instructions(),
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
            Some(match read_resource(state, uri, request.params.as_ref()) {
                Ok(payload) => JsonRpcResponse::result(request.id, payload),
                Err(error) => {
                    JsonRpcResponse::error(request.id, error.jsonrpc_code(), error.to_string())
                }
            })
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
