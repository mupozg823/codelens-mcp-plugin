use crate::AppState;
use crate::protocol::BackendKind;
use crate::resource_context::{ResourceRequestContext, build_visible_tool_context};
use crate::tool_defs::{AgentRole, HostContext, TaskOverlay, compile_surface_overlay_for_agent};
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;

use super::activate::activate_project;
use super::host_environment::HostEnvironmentSnapshot;
use super::prep_recovery::prepare_harness_index_recovery;
use super::util::client_tool_schema_fingerprint;

mod response;
mod routing;
mod warnings;

pub fn prepare_harness_session(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    // Issue #224: previously this rejected callers that supplied both
    // `preset` and `profile`, but the schema documented both as
    // independent optional fields and routing docs frequently pair
    // them ("profile=builder-minimal" + "preset=minimal"). The
    // rejection burned a bootstrap round trip.
    //
    // New contract: `profile` wins (it carries the semantic role that
    // determines tool surface + executor bias), `preset` is dropped
    // when both are supplied, and the dropped value is surfaced via
    // `surface_resolution` + a non-blocking warning so the caller
    // can adjust on the next call.
    let requested_profile = arguments
        .get("profile")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let requested_preset = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let explicit_project_request = arguments.get("project").and_then(|v| v.as_str()).is_some();
    let preset_dropped_for_profile = requested_profile.is_some() && requested_preset.is_some();
    // Drop `preset` for the downstream `activate_project` call so
    // the existing single-knob path runs unchanged. Working on a
    // clone keeps the caller's original arguments intact for logging
    // / response echo.
    let adjusted_arguments_owner: Option<serde_json::Value> =
        preset_dropped_for_profile.then(|| {
            let mut cloned = arguments.clone();
            if let Some(map) = cloned.as_object_mut() {
                map.remove("preset");
            }
            cloned
        });
    let effective_arguments: &serde_json::Value =
        adjusted_arguments_owner.as_ref().unwrap_or(arguments);

    // Preserve existing surface when caller does not explicitly request a
    // profile/preset change. `activate_project` auto-selects a surface based
    // on file count, which overwrites a harness-chosen surface on every
    // bootstrap call. We snapshot before activation and restore unless the
    // user provided a new profile/preset.
    let prior_surface = *state.surface();
    let explicit_surface_request = requested_profile.is_some() || requested_preset.is_some();

    let (activate_payload, _) = activate_project(state, effective_arguments)?;

    // Restore surface if the caller did not explicitly ask for a new one.
    if !explicit_surface_request {
        state.set_surface(prior_surface);
    }

    // Apply effort_level if provided (before preset/profile for budget calculation)
    if let Some(effort_str) = arguments.get("effort_level").and_then(|v| v.as_str()) {
        let level = match effort_str {
            "low" => crate::client_profile::EffortLevel::Low,
            "medium" => crate::client_profile::EffortLevel::Medium,
            _ => crate::client_profile::EffortLevel::High,
        };
        state.set_effort_level(level);
    }

    if arguments.get("profile").and_then(|v| v.as_str()).is_some() {
        crate::tools::session::set_profile(state, arguments)?;
    } else if arguments.get("preset").and_then(|v| v.as_str()).is_some() {
        crate::tools::session::set_preset(state, arguments)?;
    } else if let Some(budget) = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
    {
        #[cfg(feature = "http")]
        {
            let session = crate::session_context::SessionRequestContext::from_json(arguments);
            if state.should_route_to_session(&session) {
                state.set_session_surface_and_budget(
                    &session.session_id,
                    state.execution_surface(&session),
                    budget,
                );
            } else {
                state.set_token_budget(budget);
            }
        }
        #[cfg(not(feature = "http"))]
        {
            state.set_token_budget(budget);
        }
    }

    // #357: the compact bootstrap listing hides most tools (symbol
    // navigation, references, impact) until the session expands it — but
    // standard MCP clients never send the `full`/`namespace` expansion
    // params, so the surface stayed collapsed forever. A successful
    // bootstrap is the declared end of the compact phase: flip full
    // exposure and notify so hosts re-fetch the complete surface.
    let bootstrap_session = crate::session_context::SessionRequestContext::from_json(arguments);
    #[cfg(feature = "http")]
    if state.should_route_to_session(&bootstrap_session) {
        state.enable_session_full_tool_exposure(&bootstrap_session.session_id);
        state.notify_tools_list_changed(&bootstrap_session);
    }
    if bootstrap_session.is_local() {
        state.enable_local_full_tool_exposure();
    }

    let index_recovery = prepare_harness_index_recovery(state, arguments);

    // Token economy (T3): the bootstrap default is now the ~1K-token compact
    // envelope. Callers that need the full ~3.6K payload (config, http_session,
    // overlay, index_recovery, full visible_tools/routing) must opt in with an
    // explicit `detail="full"` — backward compatible for those callers.
    let detail = arguments
        .get("detail")
        .and_then(|v| v.as_str())
        .unwrap_or("compact");
    // NOTE: the response's visible-tool context intentionally keeps the
    // compact bootstrap view (T3 token economy) — the full-exposure flip
    // above only affects SUBSEQUENT tools/list calls via session metadata.
    let request = ResourceRequestContext::from_request("codelens://tools/list", Some(arguments));
    let session = request.session.clone();
    let active_surface = state.execution_surface(&session);
    let token_budget = state.execution_token_budget(&session);
    let visible = build_visible_tool_context(state, &request);
    let host_environment = HostEnvironmentSnapshot::from_arguments(
        arguments,
        &request.session,
        request.client_profile,
    );
    let surface_generation =
        crate::tool_schema_generation::surface_generation_payload(&visible.tools);
    let current_tool_schema_fingerprint =
        crate::tool_schema_generation::tool_schema_fingerprint(&visible.tools);
    let reported_client_tool_schema_fingerprint =
        client_tool_schema_fingerprint(arguments).map(str::to_owned);
    let requested_host_context = arguments
        .get("host_context")
        .and_then(|value| value.as_str());
    let requested_task_overlay = arguments
        .get("task_overlay")
        .and_then(|value| value.as_str());
    let requested_agent_role = arguments.get("agent_role").and_then(|value| value.as_str());
    let host_context = requested_host_context.and_then(HostContext::from_str);
    let task_overlay = requested_task_overlay.and_then(TaskOverlay::from_str);
    let agent_role = requested_agent_role.and_then(AgentRole::from_str);
    let overlay_plan =
        compile_surface_overlay_for_agent(active_surface, host_context, task_overlay, agent_role);
    let skill_hints = if host_context == Some(HostContext::Codex)
        || matches!(
            request.client_profile,
            crate::client_profile::ClientProfile::Codex
        ) {
        let task_hint = arguments
            .get("task")
            .or_else(|| arguments.get("objective"))
            .and_then(|value| value.as_str());
        let file_path_hint = arguments.get("file_path").and_then(|value| value.as_str());
        let skill_roots = host_environment.skill_root_paths();
        Some(if skill_roots.is_empty() {
            crate::skill_catalog::codex_prepare_skill_hints(task_hint, file_path_hint)
        } else {
            crate::skill_catalog::codex_prepare_skill_hints_for_roots(
                task_hint,
                file_path_hint,
                &skill_roots,
            )
        })
    } else {
        None
    };

    let config_payload = if detail == "full" {
        let (payload, _) = crate::tools::filesystem::get_current_config(state, arguments)?;
        payload
    } else {
        json!({
            "runtime": "rust-core",
            "project_root": state.project().as_path().display().to_string(),
            "surface": active_surface.as_label(),
            "token_budget": token_budget,
            "tool_count": crate::tool_defs::visible_tools(active_surface).len(),
            "client_profile": request.client_profile.as_str(),
        })
    };
    let capabilities_arguments = match arguments.get("file_path").and_then(|v| v.as_str()) {
        Some(file_path) => json!({ "file_path": file_path }),
        None => json!({}),
    };
    let (capabilities_payload, _) =
        crate::tools::session::get_capabilities(state, &capabilities_arguments)?;
    let health_summary = capabilities_payload
        .get("health_summary")
        .cloned()
        .unwrap_or_else(|| json!({"status": "ok", "warning_count": 0, "warnings": []}));
    let warnings = warnings::prepare_harness_warnings(warnings::PrepareHarnessWarningInput {
        capabilities_payload: &capabilities_payload,
        arguments,
        reported_client_tool_schema_fingerprint: reported_client_tool_schema_fingerprint.as_deref(),
        current_tool_schema_fingerprint: &current_tool_schema_fingerprint,
        index_recovery: &index_recovery,
        active_surface,
        state,
        requested_host_context,
        host_context,
        requested_task_overlay,
        task_overlay,
        requested_agent_role,
        agent_role,
        explicit_project_request,
        activate_payload: &activate_payload,
        preset_dropped_for_profile,
        requested_profile: requested_profile.as_deref(),
        requested_preset: requested_preset.as_deref(),
    });

    let routing = routing::prepare_harness_routing(routing::PrepareHarnessRoutingInput {
        arguments,
        active_surface,
        visible: &visible,
        host_context,
        task_overlay,
        agent_role,
        overlay_preferred_entrypoints: &overlay_plan.preferred_entrypoints,
        overlay_emphasized_tools: &overlay_plan.emphasized_tools,
        overlay_avoid_tools: &overlay_plan.avoid_tools,
        overlay_preferred_executor_bias: overlay_plan.preferred_executor_bias,
        overlay_routing_notes: &overlay_plan.routing_notes,
    });
    let result = response::prepare_harness_response(response::PrepareHarnessResponseInput {
        detail,
        state,
        request: &request,
        visible: &visible,
        activate_payload: &activate_payload,
        active_surface,
        token_budget,
        surface_generation: &surface_generation,
        config_payload: &config_payload,
        index_recovery: &index_recovery,
        capabilities_payload: &capabilities_payload,
        health_summary: &health_summary,
        warnings: &warnings,
        skill_hints: &skill_hints,
        host_environment: &host_environment,
        routing: &routing,
    });

    Ok((result, success_meta(BackendKind::Session, 1.0)))
}
