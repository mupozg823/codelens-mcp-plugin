use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::policy::POLICY_FILENAME;

/// Memory tier determines the storage root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryTier {
    /// Project-scoped: `<project>/.codelens/memories/`
    Project,
    /// User-wide: `$HOME/.codelens/memories/`
    Global,
}

impl MemoryTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

/// A resolved memory location that carries its tier and absolute path.
#[derive(Debug, Clone)]
pub struct MemoryLocation {
    pub tier: MemoryTier,
    pub dir: PathBuf,
    pub path: PathBuf,
}

/// Resolve which tier a memory name lives in.
pub fn resolve_memory_tier(
    name: &str,
    project_dir: &Path,
    global_dir: Option<&Path>,
) -> MemoryLocation {
    if let Some(stripped) = name.strip_prefix("global/") {
        let stripped = stripped.trim_start_matches('/');
        if let Some(gdir) = global_dir {
            let path = resolve_memory_path(gdir, stripped)
                .unwrap_or_else(|_| gdir.join(format!("{stripped}.md")));
            return MemoryLocation {
                tier: MemoryTier::Global,
                dir: gdir.to_path_buf(),
                path,
            };
        }
    }

    let project_memories = project_dir.join(".codelens").join("memories");
    let project_path = resolve_memory_path(&project_memories, name);
    if let Ok(path) = &project_path
        && path.is_file()
    {
        return MemoryLocation {
            tier: MemoryTier::Project,
            dir: project_memories,
            path: path.clone(),
        };
    }

    if let Some(gdir) = global_dir {
        let global_path = resolve_memory_path(gdir, name);
        if let Ok(path) = &global_path
            && path.is_file()
        {
            return MemoryLocation {
                tier: MemoryTier::Global,
                dir: gdir.to_path_buf(),
                path: path.clone(),
            };
        }
    }

    let fallback_dir = project_dir.join(".codelens").join("memories");
    MemoryLocation {
        tier: MemoryTier::Project,
        dir: fallback_dir.clone(),
        path: project_path.unwrap_or_else(|_| fallback_dir.join(format!("{name}.md"))),
    }
}

/// Return the global memory directory path: `$HOME/.codelens/memories`.
pub fn global_memory_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".codelens").join("memories"))
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

pub(crate) fn collect_memory_files(base: &Path, dir: &Path, names: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(fname) = path.file_name().and_then(|n| n.to_str())
                && fname.starts_with('.')
            {
                continue;
            }
            collect_memory_files(base, &path, names);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md")
            && let Ok(rel) = path.strip_prefix(base)
        {
            let name = rel
                .to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches(".md")
                .to_string();
            if name != POLICY_FILENAME {
                names.push(name);
            }
        }
    }
}
