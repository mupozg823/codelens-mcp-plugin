use anyhow::Result;
use std::path::{Path, PathBuf};

pub const EXCLUDED_DIRS: &[&str] = &[
    // VCS & IDE
    ".git",
    ".idea",
    ".vscode",
    ".cursor",
    ".claude",
    ".claire",
    // Build output
    ".gradle",
    "build",
    "dist",
    "out",
    "node_modules",
    "vendor",
    "__pycache__",
    "target",
    ".next",
    // Virtual environments
    ".venv",
    "venv",
    ".tox",
    "env",
    // Caches (common polluters — can contain 40K+ symbols from deps)
    ".cache",
    ".ruff_cache",
    ".pytest_cache",
    ".mypy_cache",
    ".fastembed_cache",
    // Editor extensions (e.g. Antigravity/Windsurf bundled JS)
    ".antigravity",
    ".windsurf",
    // Cloud & external mounts
    "Library",
    // CodeLens runtime
    ".codelens",
];

/// Returns `true` if any component of `path` matches an excluded directory.
pub fn is_excluded(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        EXCLUDED_DIRS.contains(&value.as_ref())
    })
}

/// Walk `root` collecting files that pass `filter`, skipping excluded dirs.
pub fn collect_files(root: &Path, filter: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    use walkdir::WalkDir;

    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if entry.file_type().is_file() && filter(entry.path()) {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

/// Walk `root` and return the canonical extension tag of the dominant
/// source language by file count (e.g. `rs`, `py`, `ts`, `go`). Returns
/// `None` when the project contains fewer than 3 source files in total,
/// or when no single language holds a clear plurality.
pub fn compute_dominant_language(root: &Path) -> Option<String> {
    use std::collections::HashMap;
    use walkdir::WalkDir;

    const WALK_CAP: usize = 16_384;
    const MIN_FILES: usize = 3;

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(ext) = entry.path().extension() else {
            continue;
        };
        let Some(ext_str) = ext.to_str() else {
            continue;
        };
        let ext_lower = ext_str.to_ascii_lowercase();
        if crate::lang_registry::for_extension(&ext_lower).is_none() {
            continue;
        }
        *counts.entry(ext_lower).or_insert(0) += 1;
        total += 1;
        if total >= WALK_CAP {
            break;
        }
    }

    if total < MIN_FILES {
        return None;
    }

    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(ext, _)| ext)
}
