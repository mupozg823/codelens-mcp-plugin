use crate::AppState;
use crate::protocol::BackendKind;
use crate::resource_context::{
    ResourceRequestContext, build_http_session_payload, build_visible_tool_context,
};
use crate::tool_defs::{
    HostContext, TaskOverlay, compile_surface_overlay, preferred_bootstrap_tools,
    tool_name_requests, tool_request_omissions,
};
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;
use std::collections::HashSet;

use super::activate::activate_project;
use super::prep_recovery::{
    RefreshSymbolIndexRemediation, prepare_harness_index_recovery,
    refresh_symbol_index_recommended_action_for_surface,
    refresh_symbol_index_remediation_for_surface,
};
use super::prep_warnings::{
    WATCHER_UNAVAILABLE_CODE, collect_prepare_harness_warnings, push_prepare_harness_warning,
    push_prepare_harness_warning_with_extras, push_rbac_permissive_default_warning,
    watcher_unavailable_warning,
};
use super::util::{client_tool_schema_fingerprint, is_anonymized_agent_project_name};

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

    let index_recovery = prepare_harness_index_recovery(state, arguments);

    let detail = arguments
        .get("detail")
        .and_then(|v| v.as_str())
        .unwrap_or("full");
    let request = ResourceRequestContext::from_request("codelens://tools/list", Some(arguments));
    let session = request.session.clone();
    let active_surface = state.execution_surface(&session);
    let token_budget = state.execution_token_budget(&session);
    let visible = build_visible_tool_context(state, &request);
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
    let host_context = requested_host_context.and_then(HostContext::from_str);
    let task_overlay = requested_task_overlay.and_then(TaskOverlay::from_str);
    let overlay_plan = compile_surface_overlay(active_surface, host_context, task_overlay);

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
    let warnings = collect_prepare_harness_warnings(
        &capabilities_payload,
        arguments
            .get("file_path")
            .and_then(|value| value.as_str())
            .is_some(),
    );
    let warnings = {
        let mut warnings = warnings;
        let mut warning_codes = warnings
            .iter()
            .filter_map(|warning| {
                warning
                    .get("code")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .collect::<HashSet<_>>();
        if let Some(client_fingerprint) = reported_client_tool_schema_fingerprint.as_deref()
            && client_fingerprint != current_tool_schema_fingerprint
        {
            push_prepare_harness_warning_with_extras(
                &mut warnings,
                &mut warning_codes,
                "tool_schema_cache_stale",
                "client-reported tool schema fingerprint does not match the active CodeLens tool surface; refresh tools/list or reconnect before trusting cached tool input schemas",
                true,
                crate::tool_schema_generation::TOOL_SCHEMA_REFRESH_ACTION,
                "tool_schema_cache",
                json!({
                    "client_tool_schema_fingerprint": client_fingerprint,
                    "server_tool_schema_fingerprint": current_tool_schema_fingerprint,
                    "schema_version": crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION,
                    "refresh": {
                        "method": "tools/list",
                        "params": { "full": true },
                        "fallback": "reconnect_mcp_server"
                    },
                }),
            );
        }
        match index_recovery
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
        {
            "failed" => {
                let stale_files = index_recovery
                    .get("before")
                    .and_then(|before| before.get("stale_files"))
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                push_prepare_harness_warning_with_extras(
                    &mut warnings,
                    &mut warning_codes,
                    "index_refresh_failed",
                    index_recovery
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("failed to refresh stale index during bootstrap"),
                    false,
                    refresh_symbol_index_recommended_action_for_surface(active_surface),
                    "symbol_index",
                    json!({
                        "remediation": refresh_symbol_index_remediation_for_surface(
                            active_surface,
                            RefreshSymbolIndexRemediation::Force
                        ),
                        "stale_files": stale_files,
                    }),
                )
            }
            "skipped" => {
                let stale_files = index_recovery
                    .get("before")
                    .and_then(|before| before.get("stale_files"))
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                let threshold = index_recovery
                    .get("threshold")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(stale_files);
                push_prepare_harness_warning_with_extras(
                    &mut warnings,
                    &mut warning_codes,
                    "index_refresh_skipped",
                    "stale index detected but auto-refresh threshold was exceeded",
                    false,
                    refresh_symbol_index_recommended_action_for_surface(active_surface),
                    "symbol_index",
                    json!({
                        "remediation": refresh_symbol_index_remediation_for_surface(
                            active_surface,
                            RefreshSymbolIndexRemediation::StaleOnly
                        ),
                        "auto_refresh_threshold": {
                            "max_stale_files": threshold,
                            "current_stale_files": stale_files,
                        },
                    }),
                )
            }
            _ => {}
        }
        // P4.1: a watcher that failed to start means the index silently
        // goes stale on every edit — surface it instead of degrading.
        let watcher_error = state.watcher_error();
        if let Some(warning) = watcher_unavailable_warning(watcher_error.as_deref())
            && warning_codes.insert(WATCHER_UNAVAILABLE_CODE.to_owned())
        {
            warnings.push(warning);
        }
        if requested_host_context.is_some() && host_context.is_none() {
            push_prepare_harness_warning(
                &mut warnings,
                &mut warning_codes,
                "unknown_host_context",
                "prepare_harness_session received an unknown host_context hint and fell back to surface-default routing",
                false,
                "use_documented_host_context",
                "bootstrap_routing",
            );
        }
        if requested_task_overlay.is_some() && task_overlay.is_none() {
            push_prepare_harness_warning(
                &mut warnings,
                &mut warning_codes,
                "unknown_task_overlay",
                "prepare_harness_session received an unknown task_overlay hint and fell back to surface-default routing",
                false,
                "use_documented_task_overlay",
                "bootstrap_routing",
            );
        }
        // Issue #186: detect when the active project resolved to an
        // anonymized agent identifier (e.g. `agent-a110134bd9c6e7440`)
        // instead of the daemon's CLI startup root. This happens when
        // a session-bound switch lands on an internal Claude/Codex
        // workspace path where the directory basename is itself a
        // hash. The active project is then "shared on-disk index"
        // material from a sibling daemon's view but the answer key is
        // a different tree, surfacing as `indexed_files: 0` +
        // false-positive `no_supported_files`. Surface the mismatch
        // so the caller can re-issue with `project=<real path>`.
        let active_project_name_for_check = activate_payload
            .get("project_name")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let active_project_is_anonymized =
            is_anonymized_agent_project_name(active_project_name_for_check);
        if active_project_is_anonymized {
            let daemon_default = state.default_project_scope();
            push_prepare_harness_warning_with_extras(
                &mut warnings,
                &mut warning_codes,
                "anonymized_project_name_detected",
                &format!(
                    "active project resolved to anonymized agent identifier `{active_project_name_for_check}`; this usually means a session-bound switch landed on an internal harness workspace and the on-disk index for the daemon's CLI default project is not loaded. Re-issue prepare_harness_session with `project=<absolute repo path>` to pin the intended project.",
                ),
                false,
                "activate_explicit_project",
                "active_project",
                json!({
                    "anonymized_project_name": active_project_name_for_check,
                    "daemon_default_project_root": daemon_default,
                    "remediation": {
                        "method": "tool_call",
                        "tool": "prepare_harness_session",
                        "args": { "project": daemon_default },
                    },
                }),
            );
        }
        if !explicit_project_request && !active_project_is_anonymized {
            let active_project_root = state.current_project_scope();
            let daemon_default_project_root = state.default_project_scope();
            if active_project_root != daemon_default_project_root {
                let suggested_project = daemon_default_project_root.clone();
                push_prepare_harness_warning_with_extras(
                    &mut warnings,
                    &mut warning_codes,
                    "active_project_differs_from_daemon_default",
                    "active CodeLens project differs from the daemon default project. If this is not the workspace you intend to inspect, re-issue prepare_harness_session or activate_project with an absolute project path; do not fall back to native tools solely because the active project is stale.",
                    false,
                    "verify_or_activate_explicit_project",
                    "active_project",
                    json!({
                        "active_project_root": active_project_root,
                        "daemon_default_project_root": daemon_default_project_root,
                        "native_fallback_recommended": false,
                        "remediation": {
                            "tool": "prepare_harness_session",
                            "args": {
                                "project": suggested_project.clone(),
                                "detail": "compact"
                            },
                            "alternative_tool": "activate_project",
                            "alternative_args": {
                                "project": suggested_project
                            }
                        }
                    }),
                );
            }
        }
        // Issue #224: when caller supplied both `profile` and `preset`,
        // surface a non-blocking warning naming the dropped value so the
        // next call can drop the redundant arg explicitly. Profile wins
        // because it carries the semantic role + executor bias.
        if preset_dropped_for_profile {
            let profile_str = requested_profile.as_deref().unwrap_or("?");
            let preset_str = requested_preset.as_deref().unwrap_or("?");
            push_prepare_harness_warning_with_extras(
                &mut warnings,
                &mut warning_codes,
                "preset_dropped_for_profile",
                &format!(
                    "both `profile` and `preset` supplied; using profile=`{profile_str}` and dropping preset=`{preset_str}` (profile wins)",
                ),
                false,
                "drop_redundant_argument",
                "preset",
                json!({
                    "winner_field": "profile",
                    "winner_value": requested_profile,
                    "dropped_field": "preset",
                    "dropped_value": requested_preset,
                }),
            );
        }
        // P3.1: mutation-capable runtime resolving principals to the
        // no-file permissive fallback — every principal gets Refactor
        // until an operator adds principals.toml or
        // CODELENS_AUTH_MODE=strict.
        push_rbac_permissive_default_warning(
            &mut warnings,
            &mut warning_codes,
            &state.principals(),
            state.mutation_allowed_in_runtime(),
        );
        warnings
    };

    let visible_tool_names = visible
        .tools
        .iter()
        .map(|tool| tool.name.to_owned())
        .collect::<Vec<_>>();
    let requested_entrypoints = arguments
        .get("preferred_entrypoints")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let overlay_preferred_entrypoints = overlay_plan
        .preferred_entrypoints
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    let preferred_entrypoints_source = if !requested_entrypoints.is_empty() {
        "provided"
    } else if !overlay_preferred_entrypoints.is_empty() {
        "overlay"
    } else {
        "surface_default"
    };
    let preferred_entrypoint_requests = if !requested_entrypoints.is_empty() {
        tool_name_requests(requested_entrypoints)
    } else if !overlay_preferred_entrypoints.is_empty() {
        tool_name_requests(overlay_preferred_entrypoints)
    } else {
        tool_name_requests(
            preferred_bootstrap_tools(active_surface)
                .unwrap_or(&[])
                .iter()
                .map(|tool| (*tool).to_owned())
                .collect::<Vec<_>>(),
        )
    };
    let preferred_entrypoints = preferred_entrypoint_requests
        .iter()
        .map(|request| request.tool.clone())
        .collect::<Vec<_>>();
    let preferred_entrypoints_visible = preferred_entrypoints
        .iter()
        .filter(|tool| visible_tool_names.iter().any(|name| name == *tool))
        .cloned()
        .collect::<Vec<_>>();
    let overlay_preferred_entrypoints_visible = overlay_plan
        .preferred_entrypoints
        .iter()
        .filter(|tool| visible_tool_names.iter().any(|name| name == *tool))
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    let overlay_emphasized_tools = overlay_plan
        .emphasized_tools
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    let overlay_emphasized_tools_visible = overlay_plan
        .emphasized_tools
        .iter()
        .filter(|tool| visible_tool_names.iter().any(|name| name == *tool))
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    let overlay_avoid_tools = overlay_plan
        .avoid_tools
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    let overlay_avoid_tools_visible = overlay_plan
        .avoid_tools
        .iter()
        .filter(|tool| visible_tool_names.iter().any(|name| name == *tool))
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    let preferred_entrypoints_with_executors = preferred_entrypoints_visible
        .iter()
        .map(|tool| {
            json!({
                "tool": tool,
                "preferred_executor": crate::tool_defs::tool_preferred_executor_label(tool),
            })
        })
        .collect::<Vec<_>>();
    let preferred_entrypoints_omitted = tool_request_omissions(
        &preferred_entrypoint_requests,
        &preferred_entrypoints_visible,
        active_surface,
        visible.deferred_loading_active,
    );
    let recommended_entrypoint = preferred_entrypoints_visible.first().cloned();
    let recommended_entrypoint_preferred_executor = recommended_entrypoint
        .as_deref()
        .map(crate::tool_defs::tool_preferred_executor_label);
    let mut visible_executor_counts = std::collections::BTreeMap::new();
    for tool in &visible.tools {
        *visible_executor_counts
            .entry(crate::tool_defs::tool_preferred_executor_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }

    let result = if detail == "full" {
        json!({
            "activated": true,
            "project": activate_payload,
            "active_surface": active_surface.as_label(),
            "token_budget": token_budget,
            "surface_generation": surface_generation,
            "config": config_payload,
            "index_recovery": index_recovery,
            "capabilities": capabilities_payload,
            "health_summary": health_summary,
            "warnings": warnings,
            "overlay": {
                "applied": overlay_plan.applied(),
                "host_context": overlay_plan.host_context.map(|value| value.as_str()),
                "task_overlay": overlay_plan.task_overlay.map(|value| value.as_str()),
                "preferred_executor_bias": overlay_plan.preferred_executor_bias,
                "preferred_entrypoints": overlay_plan.preferred_entrypoints,
                "preferred_entrypoints_visible": overlay_preferred_entrypoints_visible,
                "emphasized_tools": overlay_emphasized_tools,
                "emphasized_tools_visible": overlay_emphasized_tools_visible,
                "avoid_tools": overlay_avoid_tools,
                "avoid_tools_visible": overlay_avoid_tools_visible,
                "routing_notes": overlay_plan.routing_notes,
            },
            "http_session": build_http_session_payload(state, &request),
            "visible_tools": {
                "tool_count": visible.tools.len(),
                "tool_count_total": visible.total_tool_count,
                "tool_names": visible_tool_names,
                "preferred_executors": visible_executor_counts,
                "all_namespaces": visible.all_namespaces,
                "all_tiers": visible.all_tiers,
                "preferred_namespaces": visible.preferred_namespaces,
                "preferred_tiers": visible.preferred_tiers,
                "loaded_namespaces": visible.loaded_namespaces,
                "loaded_tiers": visible.loaded_tiers,
                "effective_namespaces": visible.effective_namespaces,
                "effective_tiers": visible.effective_tiers,
                "selected_namespace": visible.selected_namespace,
                "selected_tier": visible.selected_tier,
                "deferred_loading_active": visible.deferred_loading_active,
                "full_tool_exposure": visible.full_tool_exposure,
            },
            "routing": {
                "preferred_entrypoints": preferred_entrypoints,
                "preferred_entrypoints_source": preferred_entrypoints_source,
                "preferred_entrypoints_visible": preferred_entrypoints_visible,
                "preferred_entrypoints_omitted": preferred_entrypoints_omitted,
                "preferred_entrypoints_with_executors": preferred_entrypoints_with_executors,
                "recommended_entrypoint": recommended_entrypoint,
                "recommended_entrypoint_preferred_executor": recommended_entrypoint_preferred_executor,
            },
            "harness": {
                "effort_level": state.effort_level().as_str(),
                "compression_offset": state.effort_level().compression_threshold_offset(),
                "meta_max_result_size": true,
                "rapid_burst_detection": true,
                "schema_pre_validation": true,
                "doom_loop_threshold": 3,
                "preflight_ttl_seconds": state.preflight_ttl_seconds(),
            }
        })
    } else {
        let project_name = activate_payload
            .get("project_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let indexed_files = activate_payload
            .get("indexed_files")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let capabilities_available = capabilities_payload
            .get("available")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let tool_count = visible.tools.len();
        // Issue #199-B-1: compact mode trims `tool_names` to the first 5
        // and `preferred_entrypoints_visible` to whatever the routing
        // layer surfaces, but it never tells the caller *how much* was
        // dropped. The full-detail response carries `*_omitted_count`
        // markers next to every trimmed array; compact must do the
        // same so callers can budget their next call instead of
        // re-issuing `detail=full` just to learn the surface size.
        const COMPACT_TOOL_NAMES_LIMIT: usize = 5;
        let first_five_tools: Vec<_> = visible_tool_names
            .iter()
            .take(COMPACT_TOOL_NAMES_LIMIT)
            .cloned()
            .collect();
        let tool_names_omitted_count = visible_tool_names
            .len()
            .saturating_sub(first_five_tools.len());
        let preferred_entrypoints_visible_omitted_count = preferred_entrypoints
            .len()
            .saturating_sub(preferred_entrypoints_visible.len());
        let health_status = health_summary
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
            "surface_generation": surface_generation,
            "visible_tools": {
                "tool_count": tool_count,
                "tool_names": first_five_tools,
                "tool_names_omitted_count": tool_names_omitted_count,
            },
            "health_summary": {
                "status": health_status,
            },
            "warnings": warnings,
            "routing": {
                "recommended_entrypoint": recommended_entrypoint,
                "preferred_entrypoints_visible": preferred_entrypoints_visible,
                "preferred_entrypoints_omitted": preferred_entrypoints_omitted,
                "preferred_entrypoints_visible_omitted_count":
                    preferred_entrypoints_visible_omitted_count,
            },
        })
    };

    Ok((result, success_meta(BackendKind::Session, 1.0)))
}
