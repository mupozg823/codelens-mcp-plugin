use crate::AppState;
use crate::resource_context::ResourceRequestContext;
use crate::tool_defs::{AgentRole, HostContext, SurfaceCompilerInput, TaskOverlay, ToolProfile};
use serde_json::{Value, json};

use super::format::{json_resource, schema_resource};
use super::profiles::{profile_guide, profile_guide_summary};

pub(super) fn surface_manifest_resource(state: &AppState, uri: &str) -> Value {
    json_resource(
        uri,
        crate::surface_manifest::build_surface_manifest_for_state(state),
    )
}

pub(super) fn surface_overlay_resource(
    state: &AppState,
    uri: &str,
    params: Option<&Value>,
    request: &ResourceRequestContext,
) -> Value {
    let surface = state.execution_surface(&request.session);
    let requested_host = string_param(params, "host");
    let requested_task = string_param(params, "task");
    let requested_role = string_param(params, "role");
    let mut input = SurfaceCompilerInput::new(surface);
    if let Some(host) = requested_host.and_then(HostContext::from_str) {
        input = input.with_host(host);
    }
    if let Some(task) = requested_task.and_then(TaskOverlay::from_str) {
        input = input.with_task(task);
    }
    if let Some(role) = requested_role.and_then(AgentRole::from_str) {
        input = input.with_agent_role(role);
    }
    let plan = input.compile();
    let unknown_host = unknown_value(requested_host, HostContext::from_str);
    let unknown_task = unknown_value(requested_task, TaskOverlay::from_str);
    let unknown_role = unknown_value(requested_role, AgentRole::from_str);
    json_resource(
        uri,
        json!({
            "surface": surface.as_label(),
            "host_context": plan.host_context.map(|value| value.as_str()),
            "task_overlay": plan.task_overlay.map(|value| value.as_str()),
            "agent_role": plan.agent_role.map(|value| value.as_str()),
            "applied": plan.applied(),
            "preferred_executor_bias": plan.preferred_executor_bias,
            "preferred_entrypoints": plan.preferred_entrypoints,
            "emphasized_tools": plan.emphasized_tools,
            "avoid_tools": plan.avoid_tools,
            "routing_notes": plan.routing_notes,
            "requested_host": requested_host,
            "requested_task": requested_task,
            "requested_role": requested_role,
            "unknown_host": unknown_host,
            "unknown_task": unknown_task,
            "unknown_role": unknown_role,
        }),
    )
}

pub(super) fn harness_modes_resource(state: &AppState, uri: &str) -> Value {
    manifest_field_resource(state, uri, "harness_modes")
}

pub(super) fn harness_spec_resource(state: &AppState, uri: &str) -> Value {
    manifest_field_resource(state, uri, "harness_spec")
}

pub(super) fn harness_host_adapters_resource(state: &AppState, uri: &str) -> Value {
    manifest_field_resource(state, uri, "host_adapters")
}

pub(super) fn harness_host_resource(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let requested_host = string_param(params, "host").unwrap_or("claude-code");
    let selection_source = if string_param(params, "host").is_some() {
        "request_param"
    } else {
        "default"
    };
    let body = crate::surface_manifest::harness_host_compat_bundle_for_project(
        requested_host,
        selection_source,
        Some(state.project().as_path()),
    )
    .unwrap_or_else(|| {
        json!({
            "error": format!("Unknown host `{requested_host}`"),
            "requested_host": requested_host,
            "selection_source": selection_source
        })
    });
    json_resource(uri, body)
}

pub(super) fn host_instructions_audit_resource(state: &AppState, uri: &str) -> Value {
    json_resource(
        uri,
        crate::instruction_audit::instruction_manifest_audit(state.project().as_path()),
    )
}

pub(super) fn host_plugin_stack_benchmark_resource(state: &AppState, uri: &str) -> Value {
    json_resource(
        uri,
        crate::instruction_audit::host_plugin_stack_benchmark(state.project().as_path()),
    )
}

pub(super) fn agent_experience_resource(state: &AppState, uri: &str) -> Value {
    manifest_field_resource(state, uri, "agent_experience")
}

pub(super) fn codex_skill_catalog_resource(uri: &str) -> Value {
    json_resource(uri, crate::skill_catalog::codex_skill_catalog_resource())
}

pub(super) fn host_adapter_bundle_resource(state: &AppState, uri: &str) -> Value {
    let host = uri.trim_start_matches("codelens://host-adapters/");
    let body = crate::surface_manifest::host_adapter_bundle_for_project(
        host,
        Some(state.project().as_path()),
    )
    .unwrap_or_else(|| json!({"error": format!("Unknown host adapter `{host}`")}));
    json_resource(uri, body)
}

pub(super) fn handoff_schema_resource(uri: &str) -> Value {
    schema_resource(uri, crate::surface_manifest::handoff_artifact_schema_json())
}

pub(super) fn profile_guide_summary_resource(uri: &str) -> Value {
    let profile_name = uri
        .trim_start_matches("codelens://profile/")
        .trim_end_matches("/guide");
    let body = ToolProfile::from_str(profile_name)
        .map(profile_guide_summary)
        .unwrap_or_else(|| json!({"error": format!("Unknown profile `{profile_name}`")}));
    json_resource(uri, body)
}

pub(super) fn profile_guide_full_resource(uri: &str) -> Value {
    let profile_name = uri
        .trim_start_matches("codelens://profile/")
        .trim_end_matches("/guide/full");
    let body = ToolProfile::from_str(profile_name)
        .map(profile_guide)
        .unwrap_or_else(|| json!({"error": format!("Unknown profile `{profile_name}`")}));
    json_resource(uri, body)
}

fn manifest_field_resource(state: &AppState, uri: &str, field: &str) -> Value {
    json_resource(
        uri,
        crate::surface_manifest::build_surface_manifest_for_state(state)[field].clone(),
    )
}

fn string_param<'a>(params: Option<&'a Value>, key: &str) -> Option<&'a str> {
    params
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_str())
}

fn unknown_value<T>(value: Option<&str>, parse: impl Fn(&str) -> Option<T>) -> Option<String> {
    value
        .filter(|name| parse(name).is_none())
        .map(str::to_owned)
}
