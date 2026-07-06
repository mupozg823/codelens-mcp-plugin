use crate::AppState;
use crate::tool_defs::{AgentRole, HostContext, TaskOverlay, ToolSurface};
use serde_json::{Value, json};
use std::collections::HashSet;

use super::super::prep_warnings::{
    WATCHER_UNAVAILABLE_CODE, collect_prepare_harness_warnings, push_prepare_harness_warning,
    push_prepare_harness_warning_with_extras, push_rbac_permissive_default_warning,
    watcher_unavailable_warning,
};

mod index;
mod project_binding;

pub(super) struct PrepareHarnessWarningInput<'a> {
    pub(super) capabilities_payload: &'a Value,
    pub(super) arguments: &'a Value,
    pub(super) reported_client_tool_schema_fingerprint: Option<&'a str>,
    pub(super) current_tool_schema_fingerprint: &'a str,
    pub(super) index_recovery: &'a Value,
    pub(super) active_surface: ToolSurface,
    pub(super) state: &'a AppState,
    pub(super) requested_host_context: Option<&'a str>,
    pub(super) host_context: Option<HostContext>,
    pub(super) requested_task_overlay: Option<&'a str>,
    pub(super) task_overlay: Option<TaskOverlay>,
    pub(super) requested_agent_role: Option<&'a str>,
    pub(super) agent_role: Option<AgentRole>,
    pub(super) explicit_project_request: bool,
    pub(super) activate_payload: &'a Value,
    pub(super) preset_dropped_for_profile: bool,
    pub(super) requested_profile: Option<&'a str>,
    pub(super) requested_preset: Option<&'a str>,
}

pub(super) fn prepare_harness_warnings(input: PrepareHarnessWarningInput<'_>) -> Vec<Value> {
    let mut warnings = collect_prepare_harness_warnings(
        input.capabilities_payload,
        input
            .arguments
            .get("file_path")
            .and_then(|value| value.as_str())
            .is_some(),
    );
    let mut warning_codes = warnings
        .iter()
        .filter_map(|warning| {
            warning
                .get("code")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<HashSet<_>>();

    push_tool_schema_warning(&input, &mut warnings, &mut warning_codes);
    index::push_index_recovery_warning(&input, &mut warnings, &mut warning_codes);
    push_watcher_warning(&input, &mut warnings, &mut warning_codes);
    push_overlay_parse_warnings(&input, &mut warnings, &mut warning_codes);
    project_binding::push_project_binding_warning(&input, &mut warnings, &mut warning_codes);
    push_preset_warning(&input, &mut warnings, &mut warning_codes);
    push_rbac_permissive_default_warning(
        &mut warnings,
        &mut warning_codes,
        &input.state.principals(),
        input.state.mutation_allowed_in_runtime(),
    );

    warnings
}

fn push_tool_schema_warning(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    let Some(client_fingerprint) = input.reported_client_tool_schema_fingerprint else {
        return;
    };
    if client_fingerprint == input.current_tool_schema_fingerprint {
        return;
    }

    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        "tool_schema_cache_stale",
        "client-reported tool schema fingerprint does not match the active CodeLens tool surface; refresh tools/list or reconnect before trusting cached tool input schemas",
        true,
        crate::tool_schema_generation::TOOL_SCHEMA_REFRESH_ACTION,
        "tool_schema_cache",
        json!({
            "client_tool_schema_fingerprint": client_fingerprint,
            "server_tool_schema_fingerprint": input.current_tool_schema_fingerprint,
            "schema_version": crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION,
            "refresh": {
                "method": "tools/list",
                "params": { "full": true },
                "fallback": "reconnect_mcp_server"
            },
        }),
    );
}

fn push_watcher_warning(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    let watcher_error = input.state.watcher_error();
    if let Some(warning) = watcher_unavailable_warning(watcher_error.as_deref())
        && warning_codes.insert(WATCHER_UNAVAILABLE_CODE.to_owned())
    {
        warnings.push(warning);
    }
}

fn push_overlay_parse_warnings(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    if input.requested_host_context.is_some() && input.host_context.is_none() {
        push_prepare_harness_warning(
            warnings,
            warning_codes,
            "unknown_host_context",
            "prepare_harness_session received an unknown host_context hint and fell back to surface-default routing",
            false,
            "use_documented_host_context",
            "bootstrap_routing",
        );
    }
    if input.requested_task_overlay.is_some() && input.task_overlay.is_none() {
        push_prepare_harness_warning(
            warnings,
            warning_codes,
            "unknown_task_overlay",
            "prepare_harness_session received an unknown task_overlay hint and fell back to surface-default routing",
            false,
            "use_documented_task_overlay",
            "bootstrap_routing",
        );
    }
    if input.requested_agent_role.is_some() && input.agent_role.is_none() {
        push_prepare_harness_warning(
            warnings,
            warning_codes,
            "unknown_agent_role",
            "prepare_harness_session received an unknown agent_role hint and fell back to host/task overlay routing",
            false,
            "use_documented_agent_role",
            "bootstrap_routing",
        );
    }
}

fn push_preset_warning(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    if !input.preset_dropped_for_profile {
        return;
    }

    let profile_str = input.requested_profile.unwrap_or("?");
    let preset_str = input.requested_preset.unwrap_or("?");
    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        "preset_dropped_for_profile",
        &format!(
            "both `profile` and `preset` supplied; using profile=`{profile_str}` and dropping preset=`{preset_str}` (profile wins)",
        ),
        false,
        "drop_redundant_argument",
        "preset",
        json!({
            "winner_field": "profile",
            "winner_value": input.requested_profile,
            "dropped_field": "preset",
            "dropped_value": input.requested_preset,
        }),
    );
}
