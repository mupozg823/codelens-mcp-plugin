use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, success_meta};
use codelens_engine::compute_dominant_language;
use codelens_engine::memory::list_memory_names;
use serde_json::json;

pub(crate) fn prepare_for_new_conversation(
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

pub(crate) fn summarize_changes(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    Ok((
        json!({
            "instructions": "To summarize your changes:\n1. Use search_for_pattern to identify modified symbols\n2. Use get_symbols_overview to understand file structure\n3. Write a summary to memory using write_memory with name 'session_summary'",
            "project_name": state.project().as_path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub(crate) fn auto_set_embed_hint_lang(project_path: &std::path::Path) {
    let auto_hint_gate_enabled = std::env::var("CODELENS_EMBED_HINT_AUTO")
        .ok()
        .map(|v| {
            let lowered = v.trim().to_ascii_lowercase();
            match lowered.as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => true,
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
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", lang);
    }
}
