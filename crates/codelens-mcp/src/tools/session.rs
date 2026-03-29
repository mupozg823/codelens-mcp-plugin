use super::{success_meta, AppState, ToolResult};
use crate::tools::memory::list_memory_names;
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
    Ok((
        json!({
            "activated": true,
            "project_name": project_name,
            "project_base_path": state.project.as_path().to_string_lossy(),
            "backend_id": "rust-core",
            "memory_count": memory_count,
            "serena_memories_dir": state.memories_dir.to_string_lossy(),
            "file_watcher": watcher_running
        }),
        success_meta("session", 1.0),
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
        success_meta("session", 1.0),
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
        success_meta("session", 1.0),
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
                success_meta("session", 1.0),
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
        success_meta("session", 1.0),
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
        success_meta("session", 1.0),
    ))
}

pub fn summarize_changes(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    Ok((
        json!({
            "instructions": "To summarize your changes:\n1. Use search_for_pattern to identify modified symbols\n2. Use get_symbols_overview to understand file structure\n3. Write a summary to memory using write_memory with name 'session_summary'",
            "project_name": state.project.as_path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        }),
        success_meta("session", 1.0),
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
        success_meta("session", 1.0),
    ))
}

pub fn get_watch_status(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    match &state.watcher {
        Some(watcher) => {
            let stats = watcher.stats();
            Ok((json!(stats), success_meta("watcher", 1.0)))
        }
        None => Ok((
            json!({"running": false, "events_processed": 0, "files_reindexed": 0, "note": "File watcher not started"}),
            success_meta("watcher", 1.0),
        )),
    }
}

pub fn think_noop(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    Ok((json!(""), success_meta("noop", 1.0)))
}

pub fn switch_modes(_state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let mode = arguments
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    Ok((
        json!({"status":"ok","mode":mode,"note":"Mode switching is a no-op in standalone mode"}),
        success_meta("noop", 1.0),
    ))
}

pub fn set_preset(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let preset_str = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced");
    let new_preset = crate::ToolPreset::from_str(preset_str);
    let old_preset = {
        let mut guard = state.preset.lock().unwrap();
        let old = *guard;
        *guard = new_preset;
        old
    };
    Ok((
        json!({
            "status": "ok",
            "previous_preset": format!("{old_preset:?}"),
            "current_preset": format!("{new_preset:?}"),
            "note": "Preset changed. Next tools/list call will reflect the new tool set."
        }),
        success_meta("session", 1.0),
    ))
}
