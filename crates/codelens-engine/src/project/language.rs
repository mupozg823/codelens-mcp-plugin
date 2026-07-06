use super::exclusions::is_excluded_within;
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

/// Walk `root` and return the canonical extension tag of the dominant
/// source language by file count (e.g. `rs`, `py`, `ts`, `go`). Returns
/// `None` when the project contains fewer than 3 source files in total,
/// or when no single language holds a clear plurality.
pub fn compute_dominant_language(root: &Path) -> Option<String> {
    const WALK_CAP: usize = 16_384;
    const MIN_FILES: usize = 3;

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded_within(root, entry.path()))
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
