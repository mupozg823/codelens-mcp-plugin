use super::auto_set_embed_hint_lang;
use crate::AppState;
use crate::protocol::BackendKind;
use crate::resources::context::{
    ResourceRequestContext, build_coordination_payload, build_http_session_payload,
    build_visible_tool_context,
};
use crate::tool_defs::{
    HostContext, TaskOverlay, ToolPreset, ToolProfile, ToolSurface, compile_surface_overlay,
    default_budget_for_profile, preferred_bootstrap_tools,
};
use crate::tool_runtime::{ToolResult, success_meta};
use codelens_engine::detect_frameworks;
use codelens_engine::memory::list_memory_names;
use serde_json::json;
use std::collections::HashSet;

mod index_recovery;
mod lsp_auto_attach;
mod warnings;

use index_recovery::prepare_harness_index_recovery;
use lsp_auto_attach::auto_attach_lsp_prewarm;
use warnings::{collect_prepare_harness_warnings, push_prepare_harness_warning};

fn canonical_surface_tool_names(surface: ToolSurface, names: &[&str]) -> Vec<String> {
    crate::tool_defs::canonical_surface_tool_names(surface, names)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

fn visible_tool_subset(names: &[String], visible_tool_names: &HashSet<&str>) -> Vec<String> {
    names
        .iter()
        .filter(|tool| visible_tool_names.contains(tool.as_str()))
        .cloned()
        .collect()
}

pub(crate) fn activate_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let switched = if let Some(path) = arguments.get("project").and_then(|v| v.as_str()) {
        match state.switch_project(path) {
            Ok(name) => Some(name),
            Err(e) => {
                return Err(crate::error::CodeLensError::NotFound(format!(
                    "failed to switch project: {e}"
                )));
            }
        }
    } else {
        None
    };

    let project = state.project();
    let project_name = project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let memories_dir = state.memories_dir();
    let memory_count = list_memory_names(&memories_dir, None).len();
    let watcher_running = state.watcher_running();
    let frameworks = detect_frameworks(project.as_path());
    let project_base_path = project.as_path().to_string_lossy().to_string();

    auto_set_embed_hint_lang(project.as_path());

    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let client = session
        .client_name
        .as_deref()
        .map(|name| crate::client_profile::ClientProfile::detect(Some(name)))
        .unwrap_or_else(|| state.client_profile());
    let file_count = state
        .symbol_index()
        .stats()
        .map(|s| s.indexed_files)
        .unwrap_or(0);
    let (auto_surface, auto_budget, auto_label) =
        if matches!(client, crate::client_profile::ClientProfile::Claude) {
            (
                ToolSurface::Preset(ToolPreset::Balanced),
                client.default_budget(),
                "balanced",
            )
        } else if file_count < 50 {
            (
                ToolSurface::Profile(ToolProfile::BuilderMinimal),
                default_budget_for_profile(ToolProfile::BuilderMinimal)
                    .max(client.default_budget()),
                "builder-minimal",
            )
        } else if file_count > 500 {
            (
                ToolSurface::Profile(ToolProfile::ReviewerGraph),
                default_budget_for_profile(ToolProfile::ReviewerGraph).max(client.default_budget()),
                "reviewer-graph",
            )
        } else {
            (
                ToolSurface::Profile(ToolProfile::PlannerReadonly),
                default_budget_for_profile(ToolProfile::PlannerReadonly)
                    .max(client.default_budget()),
                "planner-readonly",
            )
        };
    #[cfg(feature = "http")]
    if state.should_route_to_session(&session) {
        state.set_session_surface_and_budget(&session.session_id, auto_surface, auto_budget);
        state.bind_project_to_session(&session.session_id, &project_base_path);
    } else {
        state.set_surface(auto_surface);
        state.set_token_budget(auto_budget);
    }
    #[cfg(not(feature = "http"))]
    {
        state.set_surface(auto_surface);
        state.set_token_budget(auto_budget);
    }

    let embedding_ready = state.embedding_status().ready();

    Ok((
        json!({
            "activated": true,
            "switched": switched.is_some(),
            "project_name": project_name,
            "project_base_path": project_base_path,
            "backend_id": "rust-core",
            "memory_count": memory_count,
            "serena_memories_dir": memories_dir.to_string_lossy(),
            "file_watcher": watcher_running,
            "frameworks": frameworks,
            "auto_surface": auto_label,
            "auto_budget": auto_budget,
            "indexed_files": file_count,
            "embedding_ready": embedding_ready
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub(crate) fn prepare_harness_session(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    if arguments.get("preset").and_then(|v| v.as_str()).is_some()
        && arguments.get("profile").and_then(|v| v.as_str()).is_some()
    {
        return Err(crate::error::CodeLensError::Validation(
            "prepare_harness_session accepts either `preset` or `profile`, not both".to_owned(),
        ));
    }

    let (activate_payload, _) = activate_project(state, arguments)?;

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
    let lsp_auto_attach = auto_attach_lsp_prewarm(state);

    let detail = arguments
        .get("detail")
        .and_then(|v| v.as_str())
        .unwrap_or("compact");
    let request = ResourceRequestContext::from_request("codelens://tools/list", Some(arguments));
    let session = request.session.clone();
    let active_surface = state.execution_surface(&session);
    let token_budget = state.execution_token_budget(&session);
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
    let (capabilities_payload, _) = crate::tools::session::get_capabilities(state, arguments)?;
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
        match index_recovery
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
        {
            "failed" => push_prepare_harness_warning(
                &mut warnings,
                &mut warning_codes,
                "index_refresh_failed",
                index_recovery
                    .get("error")
                    .and_then(|value| value.as_str())
                    .unwrap_or("failed to refresh stale index during bootstrap"),
                false,
                "refresh_symbol_index",
                "symbol_index",
            ),
            "skipped" => push_prepare_harness_warning(
                &mut warnings,
                &mut warning_codes,
                "index_refresh_skipped",
                "stale index detected but auto-refresh threshold was exceeded",
                false,
                "refresh_symbol_index",
                "symbol_index",
            ),
            _ => {}
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
        warnings
    };

    let visible = build_visible_tool_context(state, &request);
    let visible_tool_names = visible
        .tools
        .iter()
        .map(|tool| tool.name.to_owned())
        .collect::<Vec<_>>();
    let visible_tool_name_set = visible
        .tools
        .iter()
        .map(|tool| tool.name)
        .collect::<HashSet<_>>();
    let requested_entrypoints = arguments
        .get("preferred_entrypoints")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .filter_map(|tool| {
                    crate::tool_defs::canonical_surface_tool_name(active_surface, tool)
                })
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let overlay_preferred_entrypoints =
        canonical_surface_tool_names(active_surface, &overlay_plan.preferred_entrypoints);
    let preferred_entrypoints_source = if !requested_entrypoints.is_empty() {
        "provided"
    } else if !overlay_preferred_entrypoints.is_empty() {
        "overlay"
    } else {
        "surface_default"
    };
    let overlay_preferred_entrypoints_visible =
        visible_tool_subset(&overlay_preferred_entrypoints, &visible_tool_name_set);
    let preferred_entrypoints = if !requested_entrypoints.is_empty() {
        requested_entrypoints
    } else if !overlay_preferred_entrypoints.is_empty() {
        overlay_preferred_entrypoints
    } else {
        canonical_surface_tool_names(
            active_surface,
            preferred_bootstrap_tools(active_surface).unwrap_or(&[]),
        )
    };
    let preferred_entrypoints_visible =
        visible_tool_subset(&preferred_entrypoints, &visible_tool_name_set);
    let overlay_emphasized_tools =
        canonical_surface_tool_names(active_surface, &overlay_plan.emphasized_tools);
    let overlay_emphasized_tools_visible =
        visible_tool_subset(&overlay_emphasized_tools, &visible_tool_name_set);
    let overlay_avoid_tools =
        canonical_surface_tool_names(active_surface, &overlay_plan.avoid_tools);
    let overlay_avoid_tools_visible =
        visible_tool_subset(&overlay_avoid_tools, &visible_tool_name_set);
    let preferred_entrypoints_with_executors = preferred_entrypoints_visible
        .iter()
        .map(|tool| {
            json!({
                "tool": tool,
                "preferred_executor": crate::tool_defs::tool_preferred_executor_label(tool),
            })
        })
        .collect::<Vec<_>>();
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

    Ok((
        json!({
            "activated": true,
            "project": activate_payload,
            "active_surface": active_surface.as_label(),
            "token_budget": token_budget,
            "config": config_payload,
            "index_recovery": index_recovery,
            "lsp_auto_attach": lsp_auto_attach,
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
            "coordination": build_coordination_payload(state, &request),
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
            },
            "shared_analysis_pool": state.shared_analysis_pool_snapshot(20),
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}
