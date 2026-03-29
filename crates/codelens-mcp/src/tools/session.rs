use super::{success_meta, AppState, ToolResult};
use crate::protocol::BackendKind;
use crate::tools::memory::list_memory_names;
use codelens_core::detect_frameworks;
use serde_json::json;

pub fn activate_project(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let project_name = state
        .project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let memory_count = list_memory_names(&state.memories_dir, None).len();
    let watcher_running = state
        .watcher
        .as_ref()
        .map(|w| w.stats().running)
        .unwrap_or(false);
    let frameworks = detect_frameworks(state.project.as_path());

    // Auto-set preset based on project size
    let file_count = state
        .symbol_index()
        .stats()
        .map(|s| s.indexed_files)
        .unwrap_or(0);
    let (auto_preset, auto_budget) = if file_count < 50 {
        ("Minimal", 2000usize)
    } else if file_count > 500 {
        ("Full", 8000)
    } else {
        ("Balanced", 4000)
    };
    {
        let mut guard = state.preset();
        *guard = crate::tool_defs::ToolPreset::from_str(auto_preset);
    }
    state.set_token_budget(auto_budget);

    Ok((
        json!({
            "activated": true,
            "project_name": project_name,
            "project_base_path": state.project.as_path().to_string_lossy(),
            "backend_id": "rust-core",
            "memory_count": memory_count,
            "serena_memories_dir": state.memories_dir.to_string_lossy(),
            "file_watcher": watcher_running,
            "frameworks": frameworks,
            "auto_preset": auto_preset,
            "auto_budget": auto_budget,
            "indexed_files": file_count
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn check_onboarding_performed(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let required = [
        "project_overview",
        "style_and_conventions",
        "suggested_commands",
        "task_completion",
    ];
    let present = list_memory_names(&state.memories_dir, None);
    let missing: Vec<_> = required
        .iter()
        .filter(|r| !present.contains(&r.to_string()))
        .copied()
        .collect();
    Ok((
        json!({
            "onboarding_performed": missing.is_empty(),
            "required_memories": required,
            "present_memories": present,
            "missing_memories": missing,
            "serena_memories_dir": state.memories_dir.to_string_lossy(),
            "backend_id": "rust-core"
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn initial_instructions(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let project_name = state
        .project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let memories = list_memory_names(&state.memories_dir, None);
    Ok((
        json!({
            "project_name": project_name,
            "project_base_path": state.project.as_path().to_string_lossy(),
            "compatible_context": "standalone",
            "backend_id": "rust-core",
            "known_memories": memories,
            "recommended_tools": [
                "activate_project","get_current_config","check_onboarding_performed",
                "list_memories","read_memory","write_memory",
                "get_symbols_overview","find_symbol","find_referencing_symbols",
                "search_for_pattern","get_type_hierarchy"
            ]
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
        let existing = list_memory_names(&state.memories_dir, None);
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
    std::fs::create_dir_all(&state.memories_dir)?;
    let project_name = state
        .project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let defaults = [
        (
            "project_overview",
            format!(
                "# Project: {project_name}\nBase path: {}\n",
                state.project.as_path().display()
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
        let path = state.memories_dir.join(format!("{name}.md"));
        if !path.exists() {
            std::fs::write(&path, content)?;
        }
    }
    let created = list_memory_names(&state.memories_dir, None);
    Ok((
        json!({"status":"onboarded","project_name": project_name,"memories_created": created}),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn prepare_for_new_conversation(
    state: &AppState,
    _arguments: &serde_json::Value,
) -> ToolResult {
    let project_name = state
        .project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok((
        json!({
            "status":"ready",
            "project_name": project_name,
            "project_base_path": state.project.as_path().to_string_lossy(),
            "backend_id": "rust-core",
            "memory_count": list_memory_names(&state.memories_dir, None).len()
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn summarize_changes(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    Ok((
        json!({
            "instructions": "To summarize your changes:\n1. Use search_for_pattern to identify modified symbols\n2. Use get_symbols_overview to understand file structure\n3. Write a summary to memory using write_memory with name 'session_summary'",
            "project_name": state.project.as_path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn list_queryable_projects(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let project_name = state
        .project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let has_memories = state.memories_dir.is_dir();
    Ok((
        json!({
            "projects": [{"name": project_name, "path": state.project.as_path().to_string_lossy(), "is_active": true, "has_memories": has_memories}],
            "count": 1
        }),
        success_meta(BackendKind::Session, 1.0),
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

pub fn think_noop(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    Ok((json!(""), success_meta(BackendKind::Noop, 1.0)))
}

pub fn switch_modes(_state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let mode = arguments
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    Ok((
        json!({"status":"ok","mode":mode,"note":"Mode switching is a no-op in standalone mode"}),
        success_meta(BackendKind::Noop, 1.0),
    ))
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
    let session = state.metrics().session_snapshot();
    let data: Vec<serde_json::Value> = snapshot
        .into_iter()
        .map(|(name, m)| {
            json!({
                "tool": name,
                "calls": m.call_count,
                "total_ms": m.total_ms,
                "max_ms": m.max_ms,
                "errors": m.error_count,
                "last_called": m.last_called_at,
            })
        })
        .collect();
    let count = data.len();
    Ok((
        json!({
            "tools": data,
            "count": count,
            "session": {
                "total_calls": session.total_calls,
                "total_ms": session.total_ms,
                "total_tokens": session.total_tokens,
                "error_count": session.error_count,
                "avg_ms_per_call": if session.total_calls > 0 {
                    session.total_ms / session.total_calls
                } else { 0 },
                "timeline_length": session.timeline.len(),
            }
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}

pub fn set_preset(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let preset_str = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced");
    let new_preset = crate::ToolPreset::from_str(preset_str);
    let old_preset = {
        let mut guard = state.preset();
        let old = *guard;
        *guard = new_preset;
        old
    };

    // Auto-set token budget per preset, or accept explicit override
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(match new_preset {
            crate::ToolPreset::Full => 8000,
            crate::ToolPreset::Balanced => 4000,
            crate::ToolPreset::Minimal => 2000,
        });
    state.set_token_budget(budget);

    Ok((
        json!({
            "status": "ok",
            "previous_preset": format!("{old_preset:?}"),
            "current_preset": format!("{new_preset:?}"),
            "token_budget": budget,
            "note": "Preset changed. Next tools/list call will reflect the new tool set."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}
