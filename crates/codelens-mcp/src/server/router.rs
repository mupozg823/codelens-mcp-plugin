use crate::AppState;
use crate::client_profile::ClientProfile;
use crate::dispatch::dispatch_tool;
use crate::prompts::{get_prompt, prompts};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::resources::{read_resource, resources};
use crate::tool_defs::{
    is_deferred_control_tool, preferred_bootstrap_tools, preferred_namespaces,
    preferred_phase_labels, preferred_tier_labels, tool_namespace, tool_phase_label,
    tool_preferred_executor_label, tool_tier_label, visible_tools,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

fn visible_axes_from_tools(
    tools: &[&'static crate::protocol::Tool],
) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut namespaces = BTreeSet::new();
    let mut tiers = BTreeSet::new();
    for tool in tools {
        namespaces.insert(tool_namespace(tool.name));
        tiers.insert(tool_tier_label(tool.name));
    }
    (
        namespaces.into_iter().collect(),
        tiers.into_iter().collect(),
    )
}

fn merged_string_set<'a>(base: impl IntoIterator<Item = &'a str>, extra: &[&str]) -> Vec<String> {
    let mut merged = base
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    for value in extra {
        merged.insert((*value).to_owned());
    }
    merged.into_iter().collect()
}

fn list_param_bool(request: &JsonRpcRequest, camel: &str, snake: &str) -> Option<bool> {
    request
        .params
        .as_ref()
        .and_then(|params| params.get(camel).or_else(|| params.get(snake)))
        .and_then(|value| value.as_bool())
}

fn list_param_str<'a>(request: &'a JsonRpcRequest, key: &str) -> Option<&'a str> {
    request
        .params
        .as_ref()
        .and_then(|params| params.get(key))
        .and_then(|value| value.as_str())
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

    match request.method.as_str() {
        // Notifications — silently accept, never respond
        "notifications/initialized"
        | "notifications/cancelled"
        | "notifications/progress"
        | "notifications/roots/list_changed"
        | "notifications/tools/list_changed"
        | "notifications/resources/list_changed"
        | "notifications/prompts/list_changed" => None,

        "initialize" => Some(JsonRpcResponse::result(
            request.id,
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": {
                        "listChanged": state.session_resume_supported()
                    },
                    "resources": { "listChanged": false },
                    "prompts": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "codelens-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "CodeLens is a compressed context provider for agent harnesses. Prefer problem-first workflow entrypoints such as explore_codebase, review_architecture, plan_safe_refactor, trace_request_path, review_changes, cleanup_duplicate_logic, and semantic_code_review before expanding raw symbols or graph data. Legacy report tools remain available, but the workflow-first surface is the default path. Keep the visible context bounded, and use get_analysis_section or analysis resources only when you need one section in more detail. For longer reports, start_analysis_job and poll with get_analysis_job."
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
            let session = request
                .params
                .as_ref()
                .map(crate::session_context::SessionRequestContext::from_json)
                .unwrap_or_default();
            let surface = state.execution_surface(&session);
            let all_tools = visible_tools(surface);
            let (all_namespaces, all_tiers) = visible_axes_from_tools(&all_tools);
            let requested_namespace = request
                .params
                .as_ref()
                .and_then(|params| params.get("namespace"))
                .and_then(|value| value.as_str());
            let requested_tier = request
                .params
                .as_ref()
                .and_then(|params| params.get("tier"))
                .and_then(|value| value.as_str());
            let requested_phase = request
                .params
                .as_ref()
                .and_then(|params| params.get("phase"))
                .and_then(|value| value.as_str())
                .and_then(crate::protocol::ToolPhase::from_label);
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
                .and_then(|value| value.as_bool());
            let client_profile = list_param_str(&request, "_session_client_name")
                .map(|name| ClientProfile::detect(Some(name)))
                .unwrap_or(ClientProfile::Generic);
            let deferred_loading_requested = deferred_loading_requested
                .or_else(|| client_profile.default_deferred_tool_loading())
                .unwrap_or(false);
            let loaded_namespaces = request
                .params
                .as_ref()
                .and_then(|params| params.get("_session_loaded_namespaces"))
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let loaded_tiers = request
                .params
                .as_ref()
                .and_then(|params| params.get("_session_loaded_tiers"))
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let full_tool_exposure = request
                .params
                .as_ref()
                .and_then(|params| params.get("_session_full_tool_exposure"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let preferred_namespaces = preferred_namespaces(surface);
            let preferred_bootstrap = preferred_bootstrap_tools(surface);
            let preferred_tiers = preferred_tier_labels(surface);
            let has_loaded_expansions = !loaded_namespaces.is_empty() || !loaded_tiers.is_empty();
            let deferred_loading_active = deferred_loading_requested
                && requested_namespace.is_none()
                && requested_tier.is_none()
                && !full_listing
                && !full_tool_exposure;
            let lean_contract = client_profile.default_tool_contract_mode() == "lean"
                && !full_listing
                && !full_tool_exposure;
            let include_output_schema =
                list_param_bool(&request, "includeOutputSchema", "include_output_schema")
                    .unwrap_or(!(deferred_loading_active || lean_contract));
            let include_annotations =
                list_param_bool(&request, "includeAnnotations", "include_annotations")
                    .unwrap_or(!lean_contract);
            let include_deprecated =
                list_param_bool(&request, "includeDeprecated", "include_deprecated")
                    .unwrap_or(false);
            let filtered = all_tools
                .iter()
                .copied()
                .filter(|tool| match requested_namespace {
                    _ if deferred_loading_active && is_deferred_control_tool(tool.name) => true,
                    Some(namespace) => tool_namespace(tool.name) == namespace,
                    None if deferred_loading_active => {
                        let namespace = tool_namespace(tool.name);
                        let tier = tool_tier_label(tool.name);
                        preferred_namespaces.contains(&namespace)
                            || loaded_tiers.contains(&tier)
                            || loaded_namespaces.contains(&namespace)
                    }
                    None => true,
                })
                .filter(|tool| match requested_tier {
                    _ if deferred_loading_active && is_deferred_control_tool(tool.name) => true,
                    Some(tier) => tool_tier_label(tool.name) == tier,
                    None if deferred_loading_active => {
                        let namespace = tool_namespace(tool.name);
                        let tier = tool_tier_label(tool.name);
                        preferred_tiers.contains(&tier)
                            || loaded_namespaces.contains(&namespace)
                            || loaded_tiers.contains(&tier)
                    }
                    None => true,
                })
                .filter(|tool| match preferred_bootstrap {
                    _ if deferred_loading_active && is_deferred_control_tool(tool.name) => true,
                    Some(tool_names) if deferred_loading_active && !has_loaded_expansions => {
                        tool_names.contains(&tool.name)
                    }
                    _ => true,
                })
                .filter(|tool| match requested_phase {
                    // Phase-agnostic tools (filesystem, memory, session
                    // coordination, deferred-loading controls) always pass so
                    // a phase-scoped listing is not stripped of infrastructure.
                    Some(phase) => {
                        is_deferred_control_tool(tool.name)
                            || match tool_phase_label(tool.name) {
                                Some(label) => label == phase.as_label(),
                                None => true,
                            }
                    }
                    None => true,
                })
                .filter(|tool| {
                    include_deprecated || crate::tool_defs::tool_deprecation(tool.name).is_none()
                })
                .collect::<Vec<_>>();
            let response_tools = filtered
                .iter()
                .map(|tool| {
                    let mut tool = (*tool).clone();
                    let mut meta = json!({
                        "codelens/preferredExecutor": tool_preferred_executor_label(tool.name),
                    });
                    if let Some(search_hint) =
                        crate::tool_defs::tool_anthropic_search_hint(tool.name)
                    {
                        meta["anthropic/searchHint"] = json!(search_hint);
                    }
                    if crate::tool_defs::tool_anthropic_always_load(tool.name) {
                        meta["anthropic/alwaysLoad"] = Value::Bool(true);
                    }
                    if let Some((since, replacement, removal)) =
                        crate::tool_defs::tool_deprecation(tool.name)
                    {
                        meta["codelens/deprecatedSince"] = json!(since);
                        meta["codelens/deprecatedReplacement"] = json!(replacement);
                        meta["codelens/deprecatedRemovalTarget"] = json!(removal);
                    }
                    tool.meta = Some(meta);
                    if !include_output_schema {
                        tool.output_schema = None;
                    }
                    if !include_annotations {
                        tool.annotations = None;
                    }
                    tool
                })
                .collect::<Vec<_>>();
            let effective_namespaces =
                merged_string_set(preferred_namespaces.iter().copied(), &loaded_namespaces);
            let effective_tiers = merged_string_set(preferred_tiers.iter().copied(), &loaded_tiers);
            let token_estimate = if include_output_schema {
                filtered.iter().map(|tool| tool.estimated_tokens).sum()
            } else {
                serde_json::to_string(&response_tools)
                    .map(|body| body.len() / 4)
                    .unwrap_or(0)
            };
            if deferred_loading_requested
                && (requested_namespace.is_some() || requested_tier.is_some())
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
                "active_surface".to_owned(),
                Value::String(surface.as_label().to_owned()),
            );
            payload.insert(
                "preferred_namespaces".to_owned(),
                json!(preferred_namespaces),
            );
            payload.insert("preferred_tiers".to_owned(), json!(preferred_tiers));
            payload.insert(
                "preferred_phases".to_owned(),
                json!(preferred_phase_labels(surface)),
            );
            payload.insert("loaded_namespaces".to_owned(), json!(loaded_namespaces));
            payload.insert("loaded_tiers".to_owned(), json!(loaded_tiers));
            payload.insert(
                "effective_namespaces".to_owned(),
                json!(effective_namespaces),
            );
            payload.insert("effective_tiers".to_owned(), json!(effective_tiers));
            payload.insert(
                "deferred_loading_active".to_owned(),
                Value::Bool(deferred_loading_active),
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
            payload.insert("tool_count_total".to_owned(), json!(all_tools.len()));
            payload.insert("tools".to_owned(), json!(response_tools));

            if !lean_contract {
                payload.insert("visible_namespaces".to_owned(), json!(all_namespaces));
                payload.insert("visible_tiers".to_owned(), json!(all_tiers));
                payload.insert("full_listing".to_owned(), Value::Bool(full_listing));
                payload.insert(
                    "full_tool_exposure".to_owned(),
                    Value::Bool(full_tool_exposure),
                );
            }
            if let Some(namespace) = requested_namespace {
                payload.insert(
                    "selected_namespace".to_owned(),
                    Value::String(namespace.to_owned()),
                );
            }
            if let Some(tier) = requested_tier {
                payload.insert("selected_tier".to_owned(), Value::String(tier.to_owned()));
            }

            Some(JsonRpcResponse::result(request.id, Value::Object(payload)))
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
