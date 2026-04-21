use crate::AppState;
use crate::resources::context::{ResourceRequestContext, build_visible_tool_context};
use crate::surface_manifest::{HARNESS_HOST_COMPAT_RESOURCE_URI, HOST_ADAPTER_HOSTS};
use crate::tool_defs::{tool_namespace, tool_preferred_executor_label, tool_tier_label};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) fn static_resource_entries(project_name: &str) -> Vec<Value> {
    let mut items = vec![
        json!({
            "uri": "codelens://project/overview",
            "name": format!("Project: {project_name}"),
            "description": "Compressed project overview with active surface and index status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://project/architecture",
            "name": "Project Architecture",
            "description": "High-level architecture summary for harness planning",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list",
            "name": "Visible Tool Surface",
            "description": "Compressed role-aware tool surface summary",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list/full",
            "name": "Visible Tool Surface (Full)",
            "description": "Expanded role-aware tool surface with descriptions",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://surface/manifest",
            "name": "Surface Manifest",
            "description": "Canonical runtime and documentation surface manifest",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://surface/overlay",
            "name": "Surface Overlay Preview",
            "description": "Runtime preview of the (profile × host_context × task_overlay) compiled plan — query with ?host=<id>&task=<id>",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://backend/capabilities",
            "name": "Semantic Backend Capabilities",
            "description": "Runtime-backed capability map for the Rust engine, LSP bridge, and SCIP bridge backends in the active tool surface",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://registry/projects",
            "name": "Project Registry",
            "description": "Active project plus registered secondary projects with memory availability, without requiring a tool call",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://registry/memory-scopes",
            "name": "Memory Scope Registry",
            "description": "Supported memory scopes with current paths and mutation-wiring status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://operator/dashboard",
            "name": "Operator Dashboard",
            "description": "Point-in-time operator snapshot — project + surface + index health + job queue + analysis summary + backends + memory scopes, aggregated from existing telemetry",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/modes",
            "name": "Harness Modes",
            "description": "Canonical harness-mode topology and communication policy",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/spec",
            "name": "Harness Spec",
            "description": "Portable harness contract with preflight, coordination, audit, and handoff templates",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/host-adapters",
            "name": "Host Adapter Spec",
            "description": "Portable host-adaptation guidance for Claude Code, Codex, Cursor, Cline, Windsurf, and similar agent hosts",
            "mimeType": "application/json"
        }),
        json!({
            "uri": HARNESS_HOST_COMPAT_RESOURCE_URI,
            "name": "Resolved Harness Host",
            "description": "Compatibility summary for hosts that expect one resolved harness-host contract resource",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://design/agent-experience",
            "name": "Agent Experience Spec",
            "description": "Portable UX, user-flow, agent-flow, tool-flow, and harness-flow contract",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://schemas/handoff-artifact/v1",
            "name": "Handoff Artifact Schema v1",
            "description": "JSON schema for planner -> builder -> reviewer handoff artifacts",
            "mimeType": "application/schema+json"
        }),
        json!({
            "uri": "codelens://stats/token-efficiency",
            "name": "Token Efficiency Stats",
            "description": "Session-level token, chain, and handle reuse metrics",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://session/http",
            "name": "HTTP Session Runtime",
            "description": "Shared daemon session counts, timeout, and resume support",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://activity/current",
            "name": "Current Agent Activity",
            "description": "Active agent registrations, advisory claims, and recent per-session activity",
            "mimeType": "application/json"
        }),
    ];
    items.extend(HOST_ADAPTER_HOSTS.iter().map(|host| {
        json!({
            "uri": format!("codelens://host-adapters/{host}"),
            "name": format!("Host Adapter: {host}"),
            "description": "Concrete host-native routing and template bundle",
            "mimeType": "application/json"
        })
    }));
    items
}

pub(crate) fn visible_tool_summary(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let lean_contract = request.lean_tool_contract();
    let mut namespace_counts = BTreeMap::new();
    let mut tier_counts = BTreeMap::new();
    let mut executor_counts = BTreeMap::new();
    for tool in &context.tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *tier_counts
            .entry(tool_tier_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *executor_counts
            .entry(tool_preferred_executor_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    let prioritized = context
        .tools
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "tier": tool_tier_label(tool.name),
                "preferred_executor": tool_preferred_executor_label(tool.name)
            })
        })
        .collect::<Vec<_>>();
    let mut payload = serde_json::Map::new();
    payload.insert(
        "client_profile".to_owned(),
        json!(context_request_client_profile(&request)),
    );
    payload.insert("active_surface".to_owned(), json!(surface.as_label()));
    payload.insert(
        "default_contract_mode".to_owned(),
        json!(request.tool_contract_mode()),
    );
    payload.insert("tool_count".to_owned(), json!(context.tools.len()));
    payload.insert(
        "tool_count_total".to_owned(),
        json!(context.total_tool_count),
    );
    payload.insert(
        "preferred_namespaces".to_owned(),
        json!(context.preferred_namespaces),
    );
    payload.insert("preferred_tiers".to_owned(), json!(context.preferred_tiers));
    payload.insert(
        "loaded_namespaces".to_owned(),
        json!(context.loaded_namespaces),
    );
    payload.insert("loaded_tiers".to_owned(), json!(context.loaded_tiers));
    payload.insert(
        "effective_namespaces".to_owned(),
        json!(context.effective_namespaces),
    );
    payload.insert("effective_tiers".to_owned(), json!(context.effective_tiers));
    payload.insert(
        "deferred_loading_active".to_owned(),
        json!(context.deferred_loading_active),
    );
    payload.insert("preferred_executors".to_owned(), json!(executor_counts));
    payload.insert("recommended_tools".to_owned(), json!(prioritized));
    payload.insert(
        "note".to_owned(),
        json!("Read `codelens://tools/list/full` only when summary is insufficient."),
    );
    if !lean_contract {
        payload.insert("visible_namespaces".to_owned(), json!(namespace_counts));
        payload.insert("visible_tiers".to_owned(), json!(tier_counts));
        payload.insert("all_namespaces".to_owned(), json!(context.all_namespaces));
        payload.insert("all_tiers".to_owned(), json!(context.all_tiers));
        payload.insert(
            "full_tool_exposure".to_owned(),
            json!(context.full_tool_exposure),
        );
    }
    if let Some(namespace) = context.selected_namespace {
        payload.insert("selected_namespace".to_owned(), json!(namespace));
    }
    if let Some(tier) = context.selected_tier {
        payload.insert("selected_tier".to_owned(), json!(tier));
    }
    Value::Object(payload)
}

pub(crate) fn visible_tool_details(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let tools = context
        .tools
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "description": tool.description,
                "tier": tool_tier_label(tool.name),
                "preferred_executor": tool_preferred_executor_label(tool.name)
            })
        })
        .collect::<Vec<_>>();
    json!({
        "client_profile": context_request_client_profile(&request),
        "active_surface": surface.as_label(),
        "default_contract_mode": request.client_profile.default_tool_contract_mode(),
        "tool_count": tools.len(),
        "tool_count_total": context.total_tool_count,
        "all_namespaces": context.all_namespaces,
        "all_tiers": context.all_tiers,
        "preferred_namespaces": context.preferred_namespaces,
        "preferred_tiers": context.preferred_tiers,
        "loaded_namespaces": context.loaded_namespaces,
        "loaded_tiers": context.loaded_tiers,
        "effective_namespaces": context.effective_namespaces,
        "effective_tiers": context.effective_tiers,
        "selected_namespace": context.selected_namespace,
        "selected_tier": context.selected_tier,
        "deferred_loading_active": context.deferred_loading_active,
        "full_tool_exposure": context.full_tool_exposure,
        "tools": tools
    })
}

fn context_request_client_profile(
    request: &crate::resources::context::ResourceRequestContext,
) -> &'static str {
    request.client_profile.as_str()
}
