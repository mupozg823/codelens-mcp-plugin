use super::{success_meta, AppState, ToolResult};
use crate::protocol::BackendKind;
use crate::tool_defs::{default_budget_for_preset, default_budget_for_profile, ToolProfile, ToolPreset, ToolSurface};
use crate::tools::memory::list_memory_names;
use codelens_core::detect_frameworks;
use serde_json::json;

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
    let watcher_running = state
        .watcher
        .as_ref()
        .map(|w| w.stats().running)
        .unwrap_or(false);
    let frameworks = detect_frameworks(project.as_path());

    // Auto-set role surface based on project size
    let file_count = state
        .symbol_index()
        .stats()
        .map(|s| s.indexed_files)
        .unwrap_or(0);
    let (auto_surface, auto_budget, auto_label) = if file_count < 50 {
        (
            ToolSurface::Profile(ToolProfile::BuilderMinimal),
            default_budget_for_profile(ToolProfile::BuilderMinimal),
            "builder-minimal",
        )
    } else if file_count > 500 {
        (
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            default_budget_for_profile(ToolProfile::ReviewerGraph),
            "reviewer-graph",
        )
    } else {
        (
            ToolSurface::Profile(ToolProfile::PlannerReadonly),
            default_budget_for_profile(ToolProfile::PlannerReadonly),
            "planner-readonly",
        )
    };
    state.set_surface(auto_surface);
    state.set_token_budget(auto_budget);

    Ok((
        json!({
            "activated": true,
            "switched": switched.is_some(),
            "project_name": project_name,
            "project_base_path": project.as_path().to_string_lossy(),
            "backend_id": "rust-core",
            "memory_count": memory_count,
            "serena_memories_dir": memories_dir.to_string_lossy(),
            "file_watcher": watcher_running,
            "frameworks": frameworks,
            "auto_surface": auto_label,
            "auto_budget": auto_budget,
            "indexed_files": file_count
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
    let path = super::required_string(arguments, "path")?;
    match state.add_secondary_project(path) {
        Ok(name) => Ok((
            json!({ "added": true, "name": name, "path": path }),
            success_meta(BackendKind::Session, 1.0),
        )),
        Err(e) => Err(crate::error::CodeLensError::NotFound(e.to_string())),
    }
}

pub fn remove_queryable_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = super::required_string(arguments, "name")?;
    let removed = state.remove_secondary_project(name);
    Ok((
        json!({ "removed": removed, "name": name }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn query_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let project_name = super::required_string(arguments, "project_name")?;
    let symbol_name = super::required_string(arguments, "symbol_name")?;
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

pub fn get_watch_status(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let failure_count = state.symbol_index().db().index_failure_count().unwrap_or(0);
    match &state.watcher {
        Some(watcher) => {
            let mut stats = watcher.stats();
            stats.index_failures = Some(failure_count);
            Ok((json!(stats), success_meta(BackendKind::Config, 1.0)))
        }
        None => Ok((
            json!({"running": false, "events_processed": 0, "files_reindexed": 0, "index_failures": failure_count, "note": "File watcher not started"}),
            success_meta(BackendKind::Config, 1.0),
        )),
    }
}

pub fn get_capabilities(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());

    // Determine language from file path if provided
    let language = file_path
        .and_then(|fp| {
            std::path::Path::new(fp)
                .extension()
                .and_then(|e| e.to_str())
        })
        .map(|ext| ext.to_ascii_lowercase());

    // Check LSP availability
    let lsp_attached = file_path
        .and_then(|fp| crate::tools::default_lsp_command_for_path(fp))
        .map(|cmd| {
            std::process::Command::new("which")
                .arg(&cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    // Check embeddings
    #[cfg(feature = "semantic")]
    let embeddings_loaded = state.embedding.get().map(|e| e.is_some()).unwrap_or(false);
    #[cfg(not(feature = "semantic"))]
    let embeddings_loaded = false;

    // Check index freshness
    let index_stats = state.symbol_index().stats().ok();
    let index_fresh = index_stats
        .as_ref()
        .map(|s| s.stale_files == 0 && s.indexed_files > 0)
        .unwrap_or(false);

    // Build available/unavailable features
    let mut available = vec![
        "symbols",
        "imports",
        "calls",
        "rename",
        "search",
        "blast_radius",
        "dead_code",
    ];
    let mut unavailable: Vec<serde_json::Value> = Vec::new();

    if lsp_attached {
        available.extend_from_slice(&[
            "type_hierarchy",
            "diagnostics",
            "workspace_symbols",
            "rename_plan",
        ]);
    } else {
        unavailable
            .push(json!({"feature": "type_hierarchy_lsp", "reason": "no LSP server attached"}));
        unavailable.push(json!({"feature": "diagnostics", "reason": "no LSP server attached"}));
        // Native type hierarchy is still available
        available.push("type_hierarchy_native");
    }

    if embeddings_loaded {
        available.push("semantic_search");
    } else {
        unavailable.push(json!({"feature": "semantic_search", "reason": "embeddings not loaded — call index_embeddings first"}));
    }

    if !index_fresh {
        unavailable.push(json!({"feature": "cached_queries", "reason": "index may be stale — call refresh_symbol_index"}));
    }

    Ok((
        json!({
            "language": language,
            "lsp_attached": lsp_attached,
            "embeddings_loaded": embeddings_loaded,
            "index_fresh": index_fresh,
            "indexed_files": index_stats.as_ref().map(|s| s.indexed_files).unwrap_or(0),
            "available": available,
            "unavailable": unavailable,
        }),
        success_meta(BackendKind::Config, 0.95),
    ))
}

pub fn get_tool_metrics(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let snapshot = state.metrics().snapshot();
    let surfaces = state.metrics().surface_snapshot();
    let session = state.metrics().session_snapshot();
    let per_tool: Vec<serde_json::Value> = snapshot
        .into_iter()
        .map(|(name, m)| {
            json!({
                "tool": name,
                "calls": m.call_count,
                "success_count": m.success_count,
                "total_ms": m.total_ms,
                "max_ms": m.max_ms,
                "total_tokens": m.total_tokens,
                "avg_output_tokens": if m.call_count > 0 {
                    m.total_tokens / m.call_count as usize
                } else { 0 },
                "p95_latency_ms": crate::telemetry::percentile_95(&m.latency_samples),
                "success_rate": if m.call_count > 0 {
                    m.success_count as f64 / m.call_count as f64
                } else { 0.0 },
                "error_rate": if m.call_count > 0 {
                    m.error_count as f64 / m.call_count as f64
                } else { 0.0 },
                "errors": m.error_count,
                "last_called": m.last_called_at,
            })
        })
        .collect();
    let count = per_tool.len();
    let per_surface = surfaces
        .into_iter()
        .map(|(surface, metrics)| json!({
            "surface": surface,
            "calls": metrics.call_count,
            "success_count": metrics.success_count,
            "total_ms": metrics.total_ms,
            "total_tokens": metrics.total_tokens,
            "errors": metrics.error_count,
            "avg_tokens_per_call": if metrics.call_count > 0 {
                metrics.total_tokens / metrics.call_count as usize
            } else { 0 },
            "p95_latency_ms": crate::telemetry::percentile_95(&metrics.latency_samples),
            "surface_token_efficiency": if metrics.success_count > 0 {
                metrics.total_tokens as f64 / metrics.success_count as f64
            } else { 0.0 }
        }))
        .collect::<Vec<_>>();
    let handle_reads = session.analysis_summary_reads + session.analysis_section_reads;
    Ok((
        json!({
            "tools": per_tool.clone(),
            "per_tool": per_tool,
            "count": count,
            "surfaces": per_surface.clone(),
            "per_surface": per_surface,
            "session": {
                "total_calls": session.total_calls,
                "success_count": session.success_count,
                "total_ms": session.total_ms,
                "total_tokens": session.total_tokens,
                "error_count": session.error_count,
                "tools_list_tokens": session.tools_list_tokens,
                "analysis_summary_reads": session.analysis_summary_reads,
                "analysis_section_reads": session.analysis_section_reads,
                "retry_count": session.retry_count,
                "handle_reuse_count": session.handle_reuse_count,
                "repeated_low_level_chain_count": session.repeated_low_level_chain_count,
                "composite_calls": session.composite_calls,
                "low_level_calls": session.low_level_calls,
                "stdio_session_count": session.stdio_session_count,
                "http_session_count": session.http_session_count,
                "analysis_jobs_enqueued": session.analysis_jobs_enqueued,
                "analysis_jobs_started": session.analysis_jobs_started,
                "analysis_jobs_completed": session.analysis_jobs_completed,
                "analysis_jobs_failed": session.analysis_jobs_failed,
                "analysis_jobs_cancelled": session.analysis_jobs_cancelled,
                "analysis_queue_depth": session.analysis_queue_depth,
                "analysis_queue_max_depth": session.analysis_queue_max_depth,
                "active_analysis_workers": session.active_analysis_workers,
                "peak_active_analysis_workers": session.peak_active_analysis_workers,
                "analysis_worker_limit": session.analysis_worker_limit,
                "analysis_transport_mode": session.analysis_transport_mode.clone(),
                "avg_ms_per_call": if session.total_calls > 0 {
                    session.total_ms / session.total_calls
                } else { 0 },
                "avg_tool_output_tokens": if session.total_calls > 0 {
                    session.total_tokens / session.total_calls as usize
                } else { 0 },
                "p95_tool_latency_ms": crate::telemetry::percentile_95(&session.latency_samples),
                "timeline_length": session.timeline.len(),
            }
            ,
            "derived_kpis": {
                "composite_ratio": if session.total_calls > 0 {
                    session.composite_calls as f64 / session.total_calls as f64
                } else { 0.0 },
                "surface_token_efficiency": if session.success_count > 0 {
                    session.total_tokens as f64 / session.success_count as f64
                } else { 0.0 },
                "low_level_chain_reduction": if session.low_level_calls > 0 {
                    1.0 - (session.repeated_low_level_chain_count as f64 / session.low_level_calls as f64)
                } else { 1.0 },
                "handle_reuse_rate": if handle_reads > 0 {
                    session.handle_reuse_count as f64 / handle_reads as f64
                } else { 0.0 },
                "analysis_job_success_rate": if session.analysis_jobs_started > 0 {
                    session.analysis_jobs_completed as f64 / session.analysis_jobs_started as f64
                } else { 0.0 }
            }
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}

/// Export session telemetry as markdown — replaces collect-session.sh + Python.
pub fn export_session_markdown(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session_name = arguments
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("session");
    let snapshot = state.metrics().snapshot();
    let session = state.metrics().session_snapshot();

    let total_calls = session.total_calls.max(1);
    let mut tools: Vec<_> = snapshot.into_iter().collect();
    tools.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
    let count = tools.len();

    let mut md = String::with_capacity(2048);
    md.push_str(&format!("# Session Telemetry: {session_name}\n\n"));
    md.push_str("## Summary\n\n| Metric | Value |\n|---|---|\n");
    md.push_str(&format!("| Total calls | {} |\n", total_calls));
    md.push_str(&format!("| Total time | {}ms |\n", session.total_ms));
    md.push_str(&format!(
        "| Avg per call | {}ms |\n",
        if total_calls > 0 {
            session.total_ms / total_calls
        } else {
            0
        }
    ));
    md.push_str(&format!("| Total tokens | {} |\n", session.total_tokens));
    md.push_str(&format!("| Errors | {} |\n", session.error_count));
    md.push_str(&format!(
        "| Analysis summary reads | {} |\n",
        session.analysis_summary_reads
    ));
    md.push_str(&format!(
        "| Analysis section reads | {} |\n",
        session.analysis_section_reads
    ));
    md.push_str(&format!("| Unique tools | {count} |\n\n"));

    md.push_str("## Tool Usage\n\n| Tool | Calls | Total(ms) | Avg(ms) | Max(ms) | Err |\n|---|---|---|---|---|---|\n");
    for (name, m) in &tools {
        let avg = if m.call_count > 0 {
            m.total_ms as f64 / m.call_count as f64
        } else {
            0.0
        };
        md.push_str(&format!(
            "| {} | {} | {} | {:.1} | {} | {} |\n",
            name, m.call_count, m.total_ms, avg, m.max_ms, m.error_count
        ));
    }

    md.push_str("\n## Distribution\n\n```\n");
    for (name, m) in tools.iter().take(5) {
        let pct = m.call_count as f64 / total_calls as f64 * 100.0;
        let bar = "#".repeat((pct / 2.0) as usize);
        md.push_str(&format!(
            "  {:<30} {:3} ({:5.1}%) {}\n",
            name, m.call_count, pct, bar
        ));
    }
    md.push_str("```\n\n");
    md.push_str(&format!(
        "Tokens/call: {}\n",
        if total_calls > 0 {
            session.total_tokens / total_calls as usize
        } else {
            0
        }
    ));

    Ok((
        json!({
            "markdown": md,
            "session_name": session_name,
            "tool_count": count,
            "total_calls": total_calls,
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}

pub fn set_preset(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let preset_str = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced");
    let new_preset = ToolPreset::from_str(preset_str);
    let old_surface = state.surface().as_label().to_owned();
    state.set_surface(ToolSurface::Preset(new_preset));

    // Auto-set token budget per preset, or accept explicit override
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_preset(new_preset));
    state.set_token_budget(budget);

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_preset": format!("{new_preset:?}"),
            "active_surface": ToolSurface::Preset(new_preset).as_label(),
            "token_budget": budget,
            "note": "Preset changed. Next tools/list call will reflect the new tool set."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn set_profile(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let profile_str = arguments
        .get("profile")
        .and_then(|v| v.as_str())
        .unwrap_or("planner-readonly");
    let profile = ToolProfile::from_str(profile_str)
        .ok_or_else(|| crate::error::CodeLensError::Validation(format!("unknown profile `{profile_str}`")))?;
    let old_surface = state.surface().as_label().to_owned();
    state.set_surface(ToolSurface::Profile(profile));
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_profile(profile));
    state.set_token_budget(budget);

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_profile": profile.as_str(),
            "active_surface": ToolSurface::Profile(profile).as_label(),
            "token_budget": budget,
            "note": "Profile changed. Next tools/list call will reflect the role-specific tool surface."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}
