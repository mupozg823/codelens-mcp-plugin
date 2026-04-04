//! Project memory file management — .codelens/memories/ persistence layer.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// List all memory file names under the memories directory.
/// Optionally filtered by topic prefix.
pub fn list_memory_names(memories_dir: &Path, topic: Option<&str>) -> Vec<String> {
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

fn collect_memory_files(base: &Path, dir: &Path, names: &mut Vec<String>) {
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

/// Resolve a memory name to a filesystem path, with validation.
pub fn resolve_memory_path(memories_dir: &Path, name: &str) -> Result<PathBuf> {
    let normalized = name
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .trim_end_matches(".md")
        .to_string();
    if normalized.is_empty() {
        bail!("memory name must not be empty");
    }
    if normalized.contains("..") {
        bail!("memory path must not contain '..': {name}");
    }
    Ok(memories_dir.join(format!("{normalized}.md")))
}

/// Read a memory file's content.
pub fn read_memory(memories_dir: &Path, name: &str) -> Result<String> {
    let path = resolve_memory_path(memories_dir, name)?;
    std::fs::read_to_string(&path).map_err(|_| anyhow::anyhow!("memory not found: {name}"))
}

/// Write content to a memory file (creates directories if needed).
pub fn write_memory(memories_dir: &Path, name: &str, content: &str) -> Result<()> {
    let path = resolve_memory_path(memories_dir, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}

/// Delete a memory file.
pub fn delete_memory(memories_dir: &Path, name: &str) -> Result<()> {
    let path = resolve_memory_path(memories_dir, name)?;
    if !path.is_file() {
        bail!("memory not found: {name}");
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

/// Rename a memory file.
pub fn rename_memory(memories_dir: &Path, old_name: &str, new_name: &str) -> Result<()> {
    let old_path = resolve_memory_path(memories_dir, old_name)?;
    let new_path = resolve_memory_path(memories_dir, new_name)?;
    if !old_path.is_file() {
        bail!("memory not found: {old_name}");
    }
    if new_path.exists() {
        bail!("target already exists: {new_name}");
    }
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&old_path, &new_path)?;
    Ok(())
}
