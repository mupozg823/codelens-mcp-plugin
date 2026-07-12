use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface, default_budget_for_profile};
use crate::tool_runtime::{ToolResult, success_meta};
use codelens_engine::detect_frameworks;
use codelens_engine::memory::list_memory_names;
use serde_json::json;

use super::embed_hint::auto_set_embed_hint_lang;

pub fn activate_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    #[cfg(feature = "http")]
    let route_to_session = state.should_route_to_session(&session);
    #[cfg(not(feature = "http"))]
    let route_to_session = false;

    // If a project path is provided, switch the active project.
    // #357: for HTTP sessions this must NOT mutate the daemon-global
    // override (that clobbered every other session's project and cleared
    // shared artifact/job/preflight state). Instead: validate + warm the
    // context cache and re-point the CURRENT request's binding; the
    // durable per-session binding is recorded below.
    let switched = if let Some(path) = arguments.get("project").and_then(|v| v.as_str()) {
        if route_to_session {
            match state.rebind_request_project_scope(path) {
                Ok(()) => Some(
                    std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.to_owned()),
                ),
                Err(e) => return Err(switch_project_error(e)),
            }
        } else {
            match state.switch_project(path) {
                Ok(name) => Some(name),
                Err(e) => return Err(switch_project_error(e)),
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
                "builder",
            )
        } else if file_count > 500 {
            (
                ToolSurface::Profile(ToolProfile::ReviewerGraph),
                default_budget_for_profile(ToolProfile::ReviewerGraph).max(client.default_budget()),
                "review",
            )
        } else {
            (
                ToolSurface::Profile(ToolProfile::PlannerReadonly),
                default_budget_for_profile(ToolProfile::PlannerReadonly)
                    .max(client.default_budget()),
                "readonly",
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

    let embedding_ready = semantic_embedding_ready(state);

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

/// Preserve a structured `CodeLensError` (e.g. `HomeRootRejected`) raised deep
/// in the bind path instead of flattening every failure into `NotFound`, so
/// the caller still receives the machine-readable recovery hint.
fn switch_project_error(error: anyhow::Error) -> crate::error::CodeLensError {
    match error.downcast::<crate::error::CodeLensError>() {
        Ok(structured) => structured,
        Err(other) => {
            crate::error::CodeLensError::NotFound(format!("failed to switch project: {other}"))
        }
    }
}

#[cfg(feature = "semantic")]
fn semantic_embedding_ready(state: &AppState) -> bool {
    if !codelens_engine::embedding_model_assets_available() {
        return false;
    }
    if let Some(engine) = state.embedding_ref().as_ref() {
        return engine.index_info().indexed_symbols > 0;
    }
    codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
        .ok()
        .flatten()
        .map(|info| info.indexed_symbols > 0)
        .unwrap_or(false)
}

#[cfg(not(feature = "semantic"))]
fn semantic_embedding_ready(_state: &AppState) -> bool {
    false
}
