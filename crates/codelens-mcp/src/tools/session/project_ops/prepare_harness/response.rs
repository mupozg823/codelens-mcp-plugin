use crate::AppState;
use crate::resource_context::{
    ResourceRequestContext, VisibleToolContext, build_http_session_payload,
};
use crate::tool_defs::ToolSurface;
use serde_json::{Value, json};

use super::super::host_environment::HostEnvironmentSnapshot;
use super::routing::PrepareHarnessRouting;

pub(super) struct PrepareHarnessResponseInput<'a> {
    pub(super) detail: &'a str,
    pub(super) state: &'a AppState,
    pub(super) request: &'a ResourceRequestContext,
    pub(super) visible: &'a VisibleToolContext,
    pub(super) activate_payload: &'a Value,
    pub(super) active_surface: ToolSurface,
    pub(super) token_budget: usize,
    pub(super) surface_generation: &'a Value,
    pub(super) config_payload: &'a Value,
    pub(super) index_recovery: &'a Value,
    pub(super) capabilities_payload: &'a Value,
    pub(super) health_summary: &'a Value,
    pub(super) warnings: &'a [Value],
    pub(super) skill_hints: &'a Option<Value>,
    pub(super) host_environment: &'a HostEnvironmentSnapshot,
    pub(super) routing: &'a PrepareHarnessRouting,
}

pub(super) fn prepare_harness_response(input: PrepareHarnessResponseInput<'_>) -> Value {
    if input.detail == "full" {
        // Token economy (T3): trim null/empty scaffold (e.g. the six
        // `embedding_coreml_*` fields that are null off macOS/CoreML, empty
        // warning arrays, absent host-environment slots) from the full
        // bootstrap payload. Stripping is applied uniformly so the two
        // health-summary copies (`health_summary` and `capabilities.health_summary`)
        // stay byte-identical.
        let mut value = full_response(input);
        strip_empty_fields(&mut value);
        value
    } else {
        compact_response(input)
    }
}

/// Recursively drop null / empty-string / empty-array / empty-object fields.
///
/// Mirrors `crate::dispatch::response_support::payload_compact::strip_empty_fields`
/// (the canonical implementation), replicated here because that module is
/// private to `dispatch` and not reachable from the tool layer. Boolean
/// `false` and numeric `0` are intentionally preserved — they are signal, not
/// empty scaffold (e.g. `after.stale_files: 0`, deferred gate `false`).
fn strip_empty_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, v| {
                strip_empty_fields(v);
                !is_empty_value(v)
            });
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_empty_fields(item);
            }
        }
        _ => {}
    }
}

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

fn full_response(input: PrepareHarnessResponseInput<'_>) -> Value {
    json!({
        "activated": true,
        "project": input.activate_payload,
        "active_surface": input.active_surface.as_label(),
        "token_budget": input.token_budget,
        "surface_generation": input.surface_generation,
        "config": input.config_payload,
        "index_recovery": input.index_recovery,
        "capabilities": input.capabilities_payload,
        "health_summary": input.health_summary,
        "warnings": input.warnings,
        "skill_hints": input.skill_hints,
        "host_environment": input.host_environment.payload(),
        "http_session": build_http_session_payload(input.state, input.request),
        "visible_tools": {
            "tool_count": input.visible.tools.len(),
            "tool_count_total": input.visible.total_tool_count,
            "default_listed_tool_count": input.routing.default_listed_tool_count,
            "default_listed_tool_names": &input.routing.default_listed_tool_names,
            "tool_names": &input.routing.visible_tool_names,
            "preferred_executors": &input.routing.visible_executor_counts,
            "all_namespaces": &input.visible.all_namespaces,
            "all_tiers": &input.visible.all_tiers,
            "preferred_namespaces": &input.visible.preferred_namespaces,
            "preferred_tiers": &input.visible.preferred_tiers,
            "loaded_namespaces": &input.visible.loaded_namespaces,
            "loaded_tiers": &input.visible.loaded_tiers,
            "effective_namespaces": &input.visible.effective_namespaces,
            "effective_tiers": &input.visible.effective_tiers,
            "selected_namespace": &input.visible.selected_namespace,
            "selected_tier": &input.visible.selected_tier,
            "deferred_loading_active": input.visible.deferred_loading_active,
            "full_tool_exposure": input.visible.full_tool_exposure,
        },
        "routing": {
            "preferred_entrypoints": &input.routing.preferred_entrypoints,
            "preferred_entrypoints_source": input.routing.preferred_entrypoints_source,
            "agent_role": input.routing.overlay_agent_role,
            "preferred_entrypoints_visible": &input.routing.preferred_entrypoints_visible,
            "preferred_entrypoints_omitted": &input.routing.preferred_entrypoints_omitted,
            "preferred_entrypoints_with_executors": &input.routing.preferred_entrypoints_with_executors,
            "recommended_entrypoint": &input.routing.recommended_entrypoint,
            "recommended_entrypoint_preferred_executor": input.routing.recommended_entrypoint_preferred_executor,
        },
        "harness": {
            "effort_level": input.state.effort_level().as_str(),
            "compression_offset": input.state.effort_level().compression_threshold_offset(),
            "meta_max_result_size": true,
            "rapid_burst_detection": true,
            "schema_pre_validation": true,
            "doom_loop_threshold": 3,
            "preflight_ttl_seconds": input.state.preflight_ttl_seconds(),
        }
    })
}

fn compact_response(input: PrepareHarnessResponseInput<'_>) -> Value {
    let project_name = input
        .activate_payload
        .get("project_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let indexed_files = input
        .activate_payload
        .get("indexed_files")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let capabilities_available = input
        .capabilities_payload
        .get("available")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let first_five_tools = first_compact_tools(&input.routing.visible_tool_names);
    let tool_names_omitted_count = input
        .routing
        .visible_tool_names
        .len()
        .saturating_sub(first_five_tools.len());
    let health_status = input
        .health_summary
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("ok");

    json!({
        "activated": true,
        "project": {
            "project_name": project_name,
            "indexed_files": indexed_files,
        },
        "capabilities": {
            "available": capabilities_available,
        },
        "surface_generation": input.surface_generation,
        "visible_tools": {
            "tool_count": input.visible.tools.len(),
            "default_listed_tool_count": input.routing.default_listed_tool_count,
            "default_listed_tool_names": &input.routing.default_listed_tool_names,
            "tool_names": first_five_tools,
            "tool_names_omitted_count": tool_names_omitted_count,
        },
        "health_summary": {
            "status": health_status,
        },
        "warnings": input.warnings,
        "skill_hints": input.skill_hints,
        "host_environment": input.host_environment.compact_payload(),
        "routing": {
            "recommended_entrypoint": &input.routing.recommended_entrypoint,
            "agent_role": input.routing.overlay_agent_role,
            "preferred_entrypoints_visible": &input.routing.preferred_entrypoints_visible,
            "preferred_entrypoints_omitted": &input.routing.preferred_entrypoints_omitted,
            "preferred_entrypoints_visible_omitted_count":
                input.routing.preferred_entrypoints_visible_omitted_count(),
        },
    })
}

fn first_compact_tools(visible_tool_names: &[String]) -> Vec<String> {
    const COMPACT_TOOL_NAMES_LIMIT: usize = 5;
    visible_tool_names
        .iter()
        .take(COMPACT_TOOL_NAMES_LIMIT)
        .cloned()
        .collect()
}
