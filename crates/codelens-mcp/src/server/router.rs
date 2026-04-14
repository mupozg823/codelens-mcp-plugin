use crate::AppState;
use crate::client_profile::ClientProfile;
use crate::dispatch::dispatch_tool;
use crate::prompts::{get_prompt, prompts};
use crate::protocol::{
    JsonRpcRequest, JsonRpcResponse, RecommendedNextStep, RecommendedNextStepKind, RoutingHint,
};
use crate::resource_context::{
    BOOTSTRAP_ENTRYPOINT, ResourceRequestContext, TOOLS_LIST_ROLE, build_visible_tool_context,
    tools_list_contract_note,
};
use crate::resources::{read_resource, resources};
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{ToolProfile, ToolSurface};
use serde_json::{Map, Value, json};

fn list_param_bool(request: &JsonRpcRequest, camel: &str, snake: &str) -> Option<bool> {
    request
        .params
        .as_ref()
        .and_then(|params| params.get(camel).or_else(|| params.get(snake)))
        .and_then(|value| value.as_bool())
}

fn compact_schema_node(node: &Value) -> Value {
    let Some(obj) = node.as_object() else {
        return node.clone();
    };
    let mut compact = Map::new();
    if let Some(value) = obj.get("type") {
        compact.insert("type".to_owned(), value.clone());
    } else if obj.contains_key("properties") {
        compact.insert("type".to_owned(), Value::String("object".to_owned()));
    } else if obj.contains_key("items") {
        compact.insert("type".to_owned(), Value::String("array".to_owned()));
    }
    if let Some(value) = obj.get("required") {
        compact.insert("required".to_owned(), value.clone());
    }
    if let Some(properties) = obj.get("properties").and_then(|value| value.as_object()) {
        let compact_properties = properties
            .iter()
            .map(|(name, schema)| (name.clone(), compact_schema_node(schema)))
            .collect::<Map<_, _>>();
        compact.insert("properties".to_owned(), Value::Object(compact_properties));
    }
    if let Some(items) = obj.get("items") {
        compact.insert("items".to_owned(), compact_schema_node(items));
    }
    if compact.is_empty() {
        node.clone()
    } else {
        Value::Object(compact)
    }
}

fn compact_input_schema(schema: &Value) -> Value {
    compact_schema_node(schema)
}

fn compact_orchestration_contract(
    contract: &crate::protocol::OrchestrationContract,
) -> crate::protocol::OrchestrationContract {
    let mut compact = crate::harness_host::base_orchestration_contract();
    compact.server_role = contract.server_role.clone();
    compact.orchestration_owner = contract.orchestration_owner.clone();
    compact.retry_policy_owner.clear();
    compact.execution_loop_owner.clear();
    compact.tool_role = contract.tool_role.clone();
    compact.stage_hint = contract.stage_hint.clone();
    compact.interaction_mode = contract.interaction_mode.clone();
    compact
}

fn request_client_profile(
    state: &AppState,
    method: Option<&str>,
    params: Option<&Value>,
) -> ClientProfile {
    if matches!(method, Some("initialize")) {
        return params
            .and_then(|value| value.get("clientInfo"))
            .and_then(|value| value.get("name"))
            .and_then(|value| value.as_str())
            .map(|name| ClientProfile::detect(Some(name)))
            .unwrap_or_else(|| state.client_profile());
    }

    let session = params
        .map(SessionRequestContext::from_json)
        .unwrap_or_default();
    session
        .client_name
        .as_deref()
        .map(|name| ClientProfile::detect(Some(name)))
        .unwrap_or_else(|| state.client_profile())
}

fn request_surface(
    state: &AppState,
    method: Option<&str>,
    params: Option<&Value>,
    client: ClientProfile,
) -> ToolSurface {
    if matches!(method, Some("initialize")) {
        if let Some(profile) = params
            .and_then(|value| value.get("profile"))
            .and_then(|value| value.as_str())
            .and_then(ToolProfile::from_str)
        {
            return ToolSurface::Profile(profile);
        }
        let indexed_files = state
            .symbol_index()
            .stats()
            .ok()
            .map(|stats| stats.indexed_files);
        return client.advertised_default_surface(indexed_files);
    }

    let session = params
        .map(SessionRequestContext::from_json)
        .unwrap_or_default();
    state.execution_surface(&session)
}

fn protocol_error_next_steps(method: Option<&str>) -> Vec<RecommendedNextStep> {
    let mut steps = Vec::new();
    if matches!(method, Some("tools/call") | Some("tools/list")) {
        steps.push(RecommendedNextStep {
            kind: RecommendedNextStepKind::Resource,
            target: "codelens://tools/list".to_owned(),
            reason: "Read the advertised tool surface and request shape before retrying."
                .to_owned(),
        });
    }
    steps.push(RecommendedNextStep {
        kind: RecommendedNextStepKind::Handoff,
        target: "host_orchestrator".to_owned(),
        reason: "Repair the JSON-RPC request in the host and retry without widening execution."
            .to_owned(),
    });
    steps
}

pub(crate) fn protocol_error_response(
    state: &AppState,
    id: Option<Value>,
    code: i64,
    message: impl Into<String>,
    method: Option<&str>,
    params: Option<&Value>,
    error_scope: &str,
    request_stage: &str,
) -> JsonRpcResponse {
    let client = request_client_profile(state, method, params);
    let surface = request_surface(state, method, params, client);
    let mut orchestration_contract = crate::harness_host::response_orchestration_contract(
        client,
        surface,
        method.unwrap_or("protocol_error"),
        RoutingHint::Sync,
    );
    orchestration_contract.tool_role = "protocol_error".to_owned();
    orchestration_contract.stage_hint = request_stage.to_owned();
    orchestration_contract.interaction_mode = "inline_error".to_owned();
    orchestration_contract.preferred_client_behavior =
        Some("repair the request in host and retry with a valid JSON-RPC envelope".to_owned());

    JsonRpcResponse::error_with_data(
        id,
        code,
        message,
        json!({
            "error_class": "protocol",
            "error_scope": error_scope,
            "request_stage": request_stage,
            "method": method,
            "routing_hint": RoutingHint::Sync,
            "orchestration_contract": orchestration_contract,
            "recommended_next_steps": protocol_error_next_steps(method),
        }),
    )
}

pub(crate) fn handle_request(state: &AppState, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
    if request.jsonrpc != "2.0" {
        return Some(protocol_error_response(
            state,
            request.id,
            -32600,
            "Unsupported jsonrpc version",
            Some(request.method.as_str()),
            request.params.as_ref(),
            "router",
            "jsonrpc_envelope",
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
                "instructions": "CodeLens is a bounded contract provider for agent harnesses, not an execution engine. The host keeps orchestration ownership: in Claude Code, QueryEngine/query remains the orchestrator. Prefer problem-first entrypoints such as explore_codebase, review_architecture, analyze_change_impact, plan_safe_refactor, audit_security_context, trace_request_path, and cleanup_duplicate_logic before expanding raw symbols or graph data. Keep the visible context bounded, and use get_analysis_section or analysis resources only when you need one section in more detail. For longer reports, start_analysis_job and poll with get_analysis_job."
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
            let request_context = ResourceRequestContext::from_request(
                "codelens://tools/list",
                request.params.as_ref(),
                state.client_profile(),
            );
            let surface = state.execution_surface(&request_context.session);
            let context = build_visible_tool_context(state, &request_context);
            let client_profile = request_context.client_profile;
            let lean_contract = request_context.lean_tool_contract();
            let include_output_schema =
                list_param_bool(&request, "includeOutputSchema", "include_output_schema")
                    .unwrap_or(!(context.deferred_loading_active || lean_contract));
            let include_annotations =
                list_param_bool(&request, "includeAnnotations", "include_annotations")
                    .unwrap_or(!lean_contract);
            let response_tools = context
                .tools
                .iter()
                .map(|tool| {
                    let mut tool = (*tool).clone();
                    if !include_output_schema {
                        tool.output_schema = None;
                    }
                    if !include_annotations {
                        tool.annotations = None;
                    }
                    if lean_contract {
                        tool.input_schema = compact_input_schema(&tool.input_schema);
                        if let Some(contract) = tool.orchestration_contract.take() {
                            tool.orchestration_contract =
                                Some(compact_orchestration_contract(&contract));
                        }
                    }
                    tool
                })
                .collect::<Vec<_>>();
            let token_estimate = if include_output_schema {
                context.tools.iter().map(|tool| tool.estimated_tokens).sum()
            } else {
                serde_json::to_string(&response_tools)
                    .map(|body| body.len() / 4)
                    .unwrap_or(0)
            };
            if request_context.deferred_loading_requested
                && (request_context.requested_namespace.is_some()
                    || request_context.requested_tier.is_some())
            {
                state.metrics().record_deferred_namespace_expansion();
            }
            state.metrics().record_call_with_tokens(
                "tools/list",
                0,
                true,
                token_estimate,
                surface.as_label(),
                false,
                None,
            );
            let mut payload = Map::new();
            payload.insert(
                "client_profile".to_owned(),
                Value::String(client_profile.as_str().to_owned()),
            );
            payload.insert(
                "bootstrap_entrypoint".to_owned(),
                Value::String(BOOTSTRAP_ENTRYPOINT.to_owned()),
            );
            payload.insert("list_role".to_owned(), Value::String(TOOLS_LIST_ROLE.to_owned()));
            payload.insert(
                "active_surface".to_owned(),
                Value::String(surface.as_label().to_owned()),
            );
            payload.insert(
                "preferred_namespaces".to_owned(),
                json!(context.preferred_namespaces),
            );
            payload.insert("preferred_tiers".to_owned(), json!(context.preferred_tiers));
            payload.insert("loaded_namespaces".to_owned(), json!(context.loaded_namespaces));
            payload.insert("loaded_tiers".to_owned(), json!(context.loaded_tiers));
            payload.insert(
                "effective_namespaces".to_owned(),
                json!(context.effective_namespaces),
            );
            payload.insert("effective_tiers".to_owned(), json!(context.effective_tiers));
            payload.insert(
                "deferred_loading_active".to_owned(),
                Value::Bool(context.deferred_loading_active),
            );
            payload.insert(
                "include_output_schema".to_owned(),
                Value::Bool(include_output_schema),
            );
            payload.insert(
                "include_annotations".to_owned(),
                Value::Bool(include_annotations),
            );
            payload.insert(
                "default_contract_mode".to_owned(),
                Value::String(client_profile.default_tool_contract_mode().to_owned()),
            );
            payload.insert("tool_count".to_owned(), json!(response_tools.len()));
            payload.insert("tool_count_total".to_owned(), json!(context.total_tool_count));
            payload.insert("tools".to_owned(), json!(response_tools));
            payload.insert("note".to_owned(), json!(tools_list_contract_note()));

            if !lean_contract {
                payload.insert("visible_namespaces".to_owned(), json!(context.all_namespaces));
                payload.insert("visible_tiers".to_owned(), json!(context.all_tiers));
                payload.insert(
                    "full_listing".to_owned(),
                    Value::Bool(request_context.full_listing),
                );
                payload.insert(
                    "full_tool_exposure".to_owned(),
                    Value::Bool(context.full_tool_exposure),
                );
            }
            if let Some(namespace) = &context.selected_namespace {
                payload.insert(
                    "selected_namespace".to_owned(),
                    Value::String(namespace.clone()),
                );
            }
            if let Some(tier) = &context.selected_tier {
                payload.insert("selected_tier".to_owned(), Value::String(tier.clone()));
            }

            Some(JsonRpcResponse::result(request.id, Value::Object(payload)))
        }
        "tools/call" => match request.params {
            Some(params) => Some(dispatch_tool(state, request.id, params)),
            None => Some(protocol_error_response(
                state,
                request.id,
                -32602,
                "Missing params",
                Some("tools/call"),
                None,
                "router",
                "method_params",
            )),
        },
        // Unknown notification — silently ignore per JSON-RPC 2.0
        _ if is_notification => None,
        method => Some(protocol_error_response(
            state,
            request.id,
            -32601,
            format!("Method not found: {method}"),
            Some(method),
            request.params.as_ref(),
            "router",
            "method_dispatch",
        )),
    }
}
