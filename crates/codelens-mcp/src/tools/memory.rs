use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use serde_json::json;

pub fn list_memories(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let topic = arguments.get("topic").and_then(|v| v.as_str());
    let names = list_memory_names(&state.memories_dir, topic);
    Ok((
        json!({
            "topic": topic,
            "count": names.len(),
            "memories": names.iter().map(|n| json!({"name": n, "path": format!(".serena/memories/{n}.md")})).collect::<Vec<_>>()
        }),
        success_meta("filesystem", 1.0),
    ))
}

pub fn read_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let path = resolve_memory_path(&state.memories_dir, name)?;
    let content = std::fs::read_to_string(&path)
        .map_err(|_| CodeLensError::NotFound(format!("Memory: {name}")))?;
    Ok((
        json!({"memory_name": name, "content": content}),
        success_meta("filesystem", 1.0),
    ))
}

pub fn write_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let content = required_string(arguments, "content")?;
    let path = resolve_memory_path(&state.memories_dir, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok((
        json!({"status":"ok","memory_name": name}),
        success_meta("filesystem", 1.0),
    ))
}

pub fn delete_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let path = resolve_memory_path(&state.memories_dir, name)?;
    if !path.is_file() {
        return Err(CodeLensError::NotFound(format!("Memory: {name}")));
    }
    std::fs::remove_file(&path)?;
    Ok((
        json!({"status":"ok","memory_name": name}),
        success_meta("filesystem", 1.0),
    ))
}

pub fn edit_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let content = required_string(arguments, "content")?;
    let path = resolve_memory_path(&state.memories_dir, name)?;
    if !path.is_file() {
        return Err(CodeLensError::NotFound(format!("Memory: {name}")));
    }
    std::fs::write(&path, content)?;
    Ok((
        json!({"status":"ok","memory_name": name}),
        success_meta("filesystem", 1.0),
    ))
}

pub fn rename_memory(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let old_name = required_string(arguments, "old_name")?;
    let new_name = required_string(arguments, "new_name")?;
    let old_path = resolve_memory_path(&state.memories_dir, old_name)?;
    let new_path = resolve_memory_path(&state.memories_dir, new_name)?;
    if !old_path.is_file() {
        return Err(CodeLensError::NotFound(format!("Memory: {old_name}")));
    }
    if new_path.exists() {
        return Err(CodeLensError::Validation(format!(
            "target already exists: {new_name}"
        )));
    }
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&old_path, &new_path)?;
    Ok((
        json!({"status":"ok","old_name": old_name,"new_name": new_name}),
        success_meta("filesystem", 1.0),
    ))
}

// ── Helpers (moved from main.rs) ─────────────────────────────────────────

pub fn list_memory_names(memories_dir: &std::path::Path, topic: Option<&str>) -> Vec<String> {
    if !memories_dir.is_dir() {
        return Vec::new();
    }
    let mut names = Vec::new();
    collect_memory_files(memories_dir, memories_dir, &mut names);
    names.sort();
    if let Some(t) = topic {
        let t = t.trim().trim_matches('/');
        if !t.is_empty() {
            names.retain(|n| n == t || n.starts_with(&format!("{t}/")));
        }
    }
    names
}

fn collect_memory_files(base: &std::path::Path, dir: &std::path::Path, names: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_memory_files(base, &path, names);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(rel) = path.strip_prefix(base) {
                let name = rel
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches(".md")
                    .to_string();
                names.push(name);
            }
        }
    }
}

pub fn resolve_memory_path(
    memories_dir: &std::path::Path,
    name: &str,
) -> Result<std::path::PathBuf, CodeLensError> {
    let normalized = name
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .trim_end_matches(".md")
        .to_string();
    if normalized.is_empty() {
        return Err(CodeLensError::Validation(
            "memory name must not be empty".into(),
        ));
    }
    if normalized.contains("..") {
        return Err(CodeLensError::Validation(format!(
            "memory path must not contain '..': {name}"
        )));
    }
    Ok(memories_dir.join(format!("{normalized}.md")))
}
