use crate::protocol::BackendKind;
use crate::tool_defs::{default_budget_for_profile, ToolPreset, ToolProfile, ToolSurface};
use crate::tools::{success_meta, AppState, ToolResult};
use codelens_core::detect_frameworks;
use codelens_core::memory::list_memory_names;
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
            "indexed_files": file_count,
            "embedding_ready": state.embedding_ref().is_some()
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
    let path = crate::tools::required_string(arguments, "path")?;
    match state.add_secondary_project(path) {
        Ok(name) => Ok((
            json!({ "added": true, "name": name, "path": path }),
            success_meta(BackendKind::Session, 1.0),
        )),
        Err(e) => Err(crate::error::CodeLensError::NotFound(e.to_string())),
    }
}

pub fn remove_queryable_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = crate::tools::required_string(arguments, "name")?;
    let removed = state.remove_secondary_project(name);
    Ok((
        json!({ "removed": removed, "name": name }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn query_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let project_name = crate::tools::required_string(arguments, "project_name")?;
    let symbol_name = crate::tools::required_string(arguments, "symbol_name")?;
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
