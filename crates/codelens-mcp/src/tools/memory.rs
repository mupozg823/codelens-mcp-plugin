use super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use serde_json::json;

pub fn list_memories(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let topic = arguments.get("topic").and_then(|v| v.as_str());
    let names = codelens_engine::memory::list_memory_names(&state.memories_dir(), topic);
    Ok((
        json!({
            "topic": topic,
            "count": names.len(),
            "memories": names.iter().map(|n| json!({"name": n, "path": format!(".codelens/memories/{n}.md")})).collect::<Vec<_>>()
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn read_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let content = codelens_engine::memory::read_memory(&state.memories_dir(), name)
        .map_err(|_| CodeLensError::NotFound(format!("Memory: {name}")))?;
    Ok((
        json!({"memory_name": name, "content": content}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn write_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let content = required_string(arguments, "content")?;
    codelens_engine::memory::write_memory(&state.memories_dir(), name, content)?;
    Ok((
        json!({"status":"ok","memory_name": name}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn delete_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    codelens_engine::memory::delete_memory(&state.memories_dir(), name)
        .map_err(|_| CodeLensError::NotFound(format!("Memory: {name}")))?;
    Ok((
        json!({"status":"ok","memory_name": name}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn rename_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name_old = required_string(arguments, "old_name")?;
    let name_new = required_string(arguments, "new_name")?;
    codelens_engine::memory::rename_memory(&state.memories_dir(), name_old, name_new)?;
    Ok((
        json!({"status":"ok","old_name": name_old,"new_name": name_new}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}
