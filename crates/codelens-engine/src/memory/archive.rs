use std::path::Path;

use anyhow::{Result, bail};

use super::audit::{AuditRecorder, MemoryAuditEvent};
use super::paths::{MemoryTier, collect_memory_files, resolve_memory_path};
use super::policy::{ARCHIVE_DIRNAME, MemoryPolicy};

pub fn archive_memory(memories_dir: &Path, name: &str) -> Result<()> {
    archive_memory_rec(memories_dir, name, None)
}

/// Archive a memory entry with optional audit recording.
pub fn archive_memory_rec(
    memories_dir: &Path,
    name: &str,
    recorder: Option<&dyn AuditRecorder>,
) -> Result<()> {
    let policy = MemoryPolicy::load(memories_dir);
    if policy.is_read_only(name) {
        bail!("memory '{name}' is read-only and cannot be archived");
    }
    let source = resolve_memory_path(memories_dir, name)?;
    if !source.is_file() {
        bail!("memory not found: {name}");
    }
    let archive_dir = memories_dir.join(ARCHIVE_DIRNAME);
    std::fs::create_dir_all(&archive_dir)?;
    let dest = archive_dir.join(source.file_name().expect("file name"));
    if dest.exists() {
        bail!("archive already contains an entry for: {name}");
    }
    let path_str = source.to_string_lossy().to_string();
    std::fs::rename(&source, &dest)?;
    if let Some(rec) = recorder {
        rec.record(&MemoryAuditEvent::Archived {
            tier: MemoryTier::Project,
            path: path_str,
        });
    }
    Ok(())
}

/// List archived memory entries.
pub fn list_archived(memories_dir: &Path) -> Result<Vec<String>> {
    let archive_dir = memories_dir.join(ARCHIVE_DIRNAME);
    if !archive_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    collect_memory_files(memories_dir, &archive_dir, &mut names);
    for name in &mut names {
        *name = format!("archived/{}", name);
    }
    names.sort();
    Ok(names)
}

/// Restore an archived memory entry.
/// Records an audit event via `recorder` when provided.
pub fn restore_archived(memories_dir: &Path, name: &str) -> Result<()> {
    restore_archived_rec(memories_dir, name, None)
}

/// Restore an archived memory entry with optional audit recording.
pub fn restore_archived_rec(
    memories_dir: &Path,
    name: &str,
    recorder: Option<&dyn AuditRecorder>,
) -> Result<()> {
    let short_name = name.trim_start_matches("archived/");
    let archive_dir = memories_dir.join(ARCHIVE_DIRNAME);
    let source = resolve_memory_path(&archive_dir, short_name)?;
    if !source.is_file() {
        bail!("archived memory not found: {name}");
    }
    let dest = resolve_memory_path(memories_dir, short_name)?;
    if dest.exists() {
        bail!("a memory with the name '{short_name}' already exists; delete it first");
    }
    let path_str = dest.to_string_lossy().to_string();
    std::fs::rename(&source, &dest)?;
    if let Some(rec) = recorder {
        rec.record(&MemoryAuditEvent::Restored {
            tier: MemoryTier::Project,
            path: path_str,
        });
    }
    Ok(())
}
