use crate::AppState;
use crate::protocol::BackendKind;
use crate::resource_context::{
    ResourceRequestContext, build_http_session_payload, build_visible_tool_context,
};
use crate::tool_defs::{
    HostContext, TaskOverlay, ToolPreset, ToolProfile, ToolSurface, compile_surface_overlay,
    default_budget_for_profile, preferred_bootstrap_tools,
};
use crate::tool_runtime::{ToolResult, required_string, success_meta};
use codelens_engine::memory::list_memory_names;
use codelens_engine::{compute_dominant_language, detect_frameworks};
use serde_json::{Value, json};
use std::collections::HashSet;

const DEFAULT_AUTO_REFRESH_STALE_THRESHOLD: usize = 32;

fn push_prepare_harness_warning(
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    code: &str,
    message: &str,
    restart_recommended: bool,
    recommended_action: &str,
    action_target: &str,
) {
    if warning_codes.insert(code.to_owned()) {
        warnings.push(json!({
            "code": code,
            "message": message,
            "restart_recommended": restart_recommended,
            "recommended_action": recommended_action,
            "action_target": action_target,
        }));
    }
}

fn append_prepare_harness_warning_from_guidance(
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    guidance: &Value,
    fallback_code: &str,
    fallback_message: &str,
    fallback_action: &str,
    fallback_target: &str,
) {
    let code = guidance
        .get("reason_code")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_code);
    let message = guidance
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_message);
    let recommended_action = guidance
        .get("recommended_action")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_action);
    let action_target = guidance
        .get("action_target")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_target);
    push_prepare_harness_warning(
        warnings,
        warning_codes,
        code,
        message,
        action_target == "daemon" || code == "stale_daemon_binary",
        recommended_action,
        action_target,
    );
}

fn collect_prepare_harness_warnings(
    capabilities_payload: &Value,
    include_diagnostics_warning: bool,
) -> Vec<Value> {
    let mut warnings = Vec::new();
    let mut warning_codes = HashSet::new();

    if let Some(items) = capabilities_payload
        .get("health_summary")
        .and_then(|value| value.get("warnings"))
        .and_then(|value| value.as_array())
    {
        for warning in items {
            let code = warning
                .get("code")
                .and_then(|value| value.as_str())
                .unwrap_or("health_warning");
            let message = warning
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("health warning");
            let recommended_action = warning
                .get("recommended_action")
                .and_then(|value| value.as_str())
                .unwrap_or("inspect_health_status");
            let action_target = warning
                .get("action_target")
                .and_then(|value| value.as_str())
                .unwrap_or("project");
            push_prepare_harness_warning(
                &mut warnings,
                &mut warning_codes,
                code,
                message,
                action_target == "daemon" || code == "stale_daemon_binary",
                recommended_action,
                action_target,
            );
        }
    }

    if let Some(guidance) = capabilities_payload
        .get("semantic_search_guidance")
        .filter(|value| {
            !value
                .get("available")
                .and_then(|available| available.as_bool())
                .unwrap_or(false)
        })
    {
        append_prepare_harness_warning_from_guidance(
            &mut warnings,
            &mut warning_codes,
            guidance,
            "semantic_search_unavailable",
            "semantic_search is unavailable",
            "inspect_semantic_configuration",
            "semantic_search",
        );
    }

    if include_diagnostics_warning
        && let Some(guidance) = capabilities_payload
            .get("diagnostics_guidance")
            .filter(|value| {
                !value
                    .get("available")
                    .and_then(|available| available.as_bool())
                    .unwrap_or(false)
            })
    {
        append_prepare_harness_warning_from_guidance(
            &mut warnings,
            &mut warning_codes,
            guidance,
            "diagnostics_unavailable",
            "diagnostics are unavailable",
            "inspect_lsp_configuration",
            "diagnostics",
        );
    }

    warnings
}

fn index_stats_payload(stats: &codelens_engine::IndexStats) -> Value {
    json!({
        "indexed_files": stats.indexed_files,
        "supported_files": stats.supported_files,
        "stale_files": stats.stale_files,
    })
}

fn prepare_harness_index_recovery(state: &AppState, arguments: &Value) -> Value {
    let enabled = arguments
        .get("auto_refresh_stale")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let threshold = arguments
        .get("auto_refresh_stale_threshold")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_AUTO_REFRESH_STALE_THRESHOLD);

    let before = match state.symbol_index().stats() {
        Ok(stats) => stats,
        Err(error) => {
            return json!({
                "enabled": enabled,
                "threshold": threshold,
                "status": "unavailable",
                "reason": "stats_unavailable",
                "error": error.to_string(),
            });
        }
    };

    if !enabled {
        return json!({
            "enabled": false,
            "threshold": threshold,
            "status": "disabled",
            "before": index_stats_payload(&before),
        });
    }

    if before.stale_files == 0 {
        return json!({
            "enabled": true,
            "threshold": threshold,
            "status": "not_needed",
            "before": index_stats_payload(&before),
            "after": index_stats_payload(&before),
        });
    }

    if before.stale_files > threshold {
        return json!({
            "enabled": true,
            "threshold": threshold,
            "status": "skipped",
            "reason": "stale_threshold_exceeded",
            "before": index_stats_payload(&before),
        });
    }

    match state.symbol_index().refresh_all() {
        Ok(after) => {
            state.graph_cache().invalidate();
            json!({
                "enabled": true,
                "threshold": threshold,
                "status": "refreshed",
                "reason": "stale_detected",
                "before": index_stats_payload(&before),
                "after": index_stats_payload(&after),
            })
        }
        Err(error) => json!({
            "enabled": true,
            "threshold": threshold,
            "status": "failed",
            "reason": "refresh_failed",
            "error": error.to_string(),
            "before": index_stats_payload(&before),
        }),
    }
}

pub fn activate_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    // If a project path is provided, switch the active project
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

    // v1.5 Phase 2j MCP follow-up: auto-set `CODELENS_EMBED_HINT_AUTO_LANG`
    // based on the project's dominant source language. Shared with the
    // startup path (`main.rs`) so one-shot CLI and stdio MCP share the
    // same detection + gating.
    auto_set_embed_hint_lang(project.as_path());

    // Auto-set role surface based on project size + client profile
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
    // For Claude Code clients, keep Balanced preset (all tools accessible).
    // Profile auto-selection only applies to Codex/generic clients.
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

    #[cfg(feature = "semantic")]
    let embedding_ready = state.embedding_ref().is_some();
    #[cfg(not(feature = "semantic"))]
    let embedding_ready = false;

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

pub fn prepare_harness_session(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    if arguments.get("preset").and_then(|v| v.as_str()).is_some()
        && arguments.get("profile").and_then(|v| v.as_str()).is_some()
    {
        return Err(crate::error::CodeLensError::Validation(
            "prepare_harness_session accepts either `preset` or `profile`, not both".to_owned(),
        ));
    }

    let (activate_payload, _) = activate_project(state, arguments)?;

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
    let preferred_entrypoints = if !requested_entrypoints.is_empty() {
        requested_entrypoints
    } else if !overlay_preferred_entrypoints.is_empty() {
        overlay_preferred_entrypoints
    } else {
        preferred_bootstrap_tools(active_surface)
            .unwrap_or(&[])
            .iter()
            .map(|tool| (*tool).to_owned())
            .collect::<Vec<_>>()
    };
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
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn onboarding(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let force = arguments
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !force {
        let existing = list_memory_names(&state.memories_dir(), None);
        let required = [
            "project_overview",
            "style_and_conventions",
            "suggested_commands",
            "task_completion",
        ];
        if required.iter().all(|r| existing.contains(&r.to_string())) {
            return Ok((
                json!({"status":"already_onboarded","existing_memories": existing}),
                success_meta(BackendKind::Session, 1.0),
            ));
        }
    }
    let memories_dir = state.memories_dir();
    std::fs::create_dir_all(&memories_dir)?;
    let project = state.project();
    let project_name = project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let defaults = [
        (
            "project_overview",
            format!(
                "# Project: {project_name}\nBase path: {}\n",
                project.as_path().display()
            ),
        ),
        (
            "style_and_conventions",
            "# Style & Conventions\nTo be filled during onboarding.".to_string(),
        ),
        (
            "suggested_commands",
            "# Suggested Commands\n- cargo build\n- cargo test".to_string(),
        ),
        (
            "task_completion",
            "# Task Completion Checklist\n- Build passes\n- Tests pass\n- No regressions"
                .to_string(),
        ),
    ];
    for (name, content) in &defaults {
        let path = memories_dir.join(format!("{name}.md"));
        if !path.exists() {
            std::fs::write(&path, content)?;
        }
    }
    let created = list_memory_names(&state.memories_dir(), None);
    Ok((
        json!({"status":"onboarded","project_name": project_name,"memories_created": created}),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn prepare_for_new_conversation(
    state: &AppState,
    _arguments: &serde_json::Value,
) -> ToolResult {
    let project = state.project();
    let project_name = project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok((
        json!({
            "status":"ready",
            "project_name": project_name,
            "project_base_path": project.as_path().to_string_lossy(),
            "backend_id": "rust-core",
            "memory_count": list_memory_names(&state.memories_dir(), None).len()
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn summarize_changes(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    Ok((
        json!({
            "instructions": "To summarize your changes:\n1. Use search_for_pattern to identify modified symbols\n2. Use get_symbols_overview to understand file structure\n3. Write a summary to memory using write_memory with name 'session_summary'",
            "project_name": state.project().as_path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn list_queryable_projects(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let project = state.project();
    let project_name = project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let has_memories = state.memories_dir().is_dir();

    let mut projects = vec![json!({
        "name": project_name,
        "path": project.as_path().to_string_lossy(),
        "is_active": true,
        "has_memories": has_memories
    })];

    for (name, path) in state.list_secondary_projects() {
        projects.push(json!({
            "name": name,
            "path": path,
            "is_active": false,
            "has_memories": false
        }));
    }

    let count = projects.len();
    Ok((
        json!({ "projects": projects, "count": count }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn add_queryable_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    match state.add_secondary_project(path) {
        Ok(name) => Ok((
            json!({ "added": true, "name": name, "path": path }),
            success_meta(BackendKind::Session, 1.0),
        )),
        Err(e) => Err(crate::error::CodeLensError::NotFound(e.to_string())),
    }
}

pub fn remove_queryable_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "name")?;
    let removed = state.remove_secondary_project(name);
    Ok((
        json!({ "removed": removed, "name": name }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn query_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let project_name = required_string(arguments, "project_name")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let symbols = state
        .query_secondary_project(project_name, symbol_name, max_results)
        .map_err(|e| crate::error::CodeLensError::NotFound(e.to_string()))?;

    Ok((
        json!({
            "project": project_name,
            "symbols": symbols,
            "count": symbols.len()
        }),
        success_meta(BackendKind::TreeSitter, 0.90),
    ))
}

/// v1.5 Phase 2j MCP follow-up: auto-detect and export the dominant source
/// language for the given project so the engine's `auto_hint_should_enable`
/// gate can consult `language_supports_nl_stack` on the next embedding call.
///
/// Applied at two entry points:
///   1. Startup in `main.rs` — covers one-shot CLI (`--cmd`) and stdio MCP.
///   2. `activate_project` — covers MCP-driven project switches.
///
/// Only fires when:
///   (1) auto mode is explicitly enabled via `CODELENS_EMBED_HINT_AUTO=1`
///       (default-OFF policy held — no automatic behaviour change),
///   (2) the user has not already set `CODELENS_EMBED_HINT_AUTO_LANG`
///       themselves (explicit > auto, same rule as the three per-gate
///       env vars).
///
/// The detection walk is capped at 16k files inside
/// `compute_dominant_language` so even large monorepos pay a bounded cost.
/// When the walk yields no confident answer (fewer than 3 source files, or
/// no known-extension files at all), we leave the env var unset and the
/// engine falls through to the conservative default (stack OFF).
pub fn auto_set_embed_hint_lang(project_path: &std::path::Path) {
    // v1.6.0 flip (§8.14): default-ON semantics. Unset env means "auto
    // mode ON", explicit `CODELENS_EMBED_HINT_AUTO=0`/`false`/`no`/`off`
    // is the opt-out. Must stay in lock-step with the engine's
    // `auto_hint_mode_enabled()` in `crates/codelens-engine/src/embedding/mod.rs`.
    let auto_hint_gate_enabled = std::env::var("CODELENS_EMBED_HINT_AUTO")
        .ok()
        .map(|v| {
            let lowered = v.trim().to_ascii_lowercase();
            match lowered.as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => true, // unknown value → fall through to default-on
            }
        })
        .unwrap_or(true);
    let user_forced_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").is_ok();
    if !auto_hint_gate_enabled || user_forced_lang {
        return;
    }
    let Some(lang) = compute_dominant_language(project_path) else {
        return;
    };
    // Export to the process environment so the engine's gate functions
    // (`nl_tokens_enabled`, `api_calls_enabled`, `sparse_weighting_enabled`)
    // read it on the next call. Process-scoped — startup sets it once, and
    // `activate_project` re-writes it on project switch (handled via
    // `user_forced_lang` short-circuit: if we switch projects we'd have to
    // clear the var first, which is an acceptable follow-up limitation).
    //
    // SAFETY: `set_var` is unsafe on modern Rust because env-var mutation
    // is not thread-safe. Both call sites (startup main + single-threaded
    // MCP request handler) run before the engine has spawned worker
    // threads that read env, so the concurrent-read hazard does not apply.
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", lang);
    }
}
