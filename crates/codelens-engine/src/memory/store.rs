use std::path::Path;

use anyhow::{Context, Result, bail};

use super::audit::{AuditRecorder, MemoryAuditEvent};
use super::frontmatter::{MemoryMetadata, parse_frontmatter};
use super::paths::{MemoryTier, collect_memory_files, resolve_memory_path, resolve_memory_tier};
use super::policy::{MemoryPolicy, POLICY_FILE_BASENAME, POLICY_FILENAME};

pub fn list_memory_names(memories_dir: &Path, topic: Option<&str>) -> Vec<String> {
    let policy = MemoryPolicy::load(memories_dir);
    list_memory_names_with_policy(memories_dir, topic, &policy)
}

/// List memory names, filtering out entries matching the ignored policy.
pub fn list_memory_names_with_policy(
    memories_dir: &Path,
    topic: Option<&str>,
    policy: &MemoryPolicy,
) -> Vec<String> {
    let mut names = Vec::new();
    if !memories_dir.is_dir() {
        return names;
    }
    collect_memory_files(memories_dir, memories_dir, &mut names);
    names.sort();
    names.retain(|n| {
        // Policy file and archive dir are hidden
        !n.starts_with(POLICY_FILENAME) && !policy.is_ignored(n)
    });
    if let Some(t) = topic {
        let t = t.trim().trim_matches('/');
        if !t.is_empty() {
            names.retain(|n| n == t || n.starts_with(&format!("{t}/")));
        }
    }
    names
}

/// List memory names from all tiers, returning (name, tier) pairs.
/// Entries from the global tier are prefixed with `global/`.
pub fn list_all_memory_names(
    project_dir: &Path,
    global_dir: Option<&Path>,
    topic: Option<&str>,
) -> Vec<(String, MemoryTier)> {
    let project_memories = project_dir.join(".codelens").join("memories");
    let project_names = list_memory_names(&project_memories, topic);
    let mut result: Vec<(String, MemoryTier)> = project_names
        .into_iter()
        .map(|n| (n, MemoryTier::Project))
        .collect();

    if let Some(gdir) = global_dir {
        let global_names = list_memory_names(gdir, topic);
        for name in global_names {
            // Deduplicate: project tier takes precedence
            let prefixed = format!("global/{}", name);
            if !result.iter().any(|(n, _)| n == &name) {
                result.push((prefixed, MemoryTier::Global));
            }
        }
    }
    result
}

/// Read a memory file's content from the appropriate tier.
pub fn read_memory(memories_dir: &Path, name: &str) -> Result<String> {
    let path = resolve_memory_path(memories_dir, name)?;
    std::fs::read_to_string(&path).with_context(|| format!("memory not found: {name}"))
}

/// Read a memory file from a specific tier's directory.
pub fn read_memory_from_tier(
    project_dir: &Path,
    global_dir: Option<&Path>,
    name: &str,
) -> Result<(String, MemoryTier)> {
    let loc = resolve_memory_tier(name, project_dir, global_dir);
    let content = std::fs::read_to_string(&loc.path)
        .with_context(|| format!("memory not found: {}", name.trim_start_matches("global/")))?;
    Ok((content, loc.tier))
}

/// Read a memory file with full metadata: frontmatter links, stale detection,
/// and tier information.  This is the rich-read counterpart of
/// `read_memory_from_tier` for MCP responses that include metadata.
pub fn read_memory_with_metadata(
    project_dir: &Path,
    global_dir: Option<&Path>,
    name: &str,
) -> Result<(String, MemoryMetadata)> {
    let loc = resolve_memory_tier(name, project_dir, global_dir);
    let effective_name = name.trim_start_matches("global/");
    let content = std::fs::read_to_string(&loc.path)
        .with_context(|| format!("memory not found: {effective_name}"))?;
    let policy = MemoryPolicy::load(&loc.dir);
    let modified_secs = std::fs::metadata(&loc.path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let stale = modified_secs
        .map(|m| policy.is_stale(effective_name, m))
        .unwrap_or(false);
    let fm = parse_frontmatter(&content);
    Ok((
        content,
        MemoryMetadata {
            tier: loc.tier,
            stale,
            last_modified_secs: modified_secs,
            linked_symbols: fm
                .as_ref()
                .map(|f| f.linked_symbols.clone())
                .unwrap_or_default(),
            linked_files: fm
                .as_ref()
                .map(|f| f.linked_files.clone())
                .unwrap_or_default(),
            linked_analyses: fm
                .as_ref()
                .map(|f| f.linked_analyses.clone())
                .unwrap_or_default(),
        },
    ))
}

/// Write content to a memory file in the project tier.
/// Creates directories if needed.  Rejects writes to read-only entries.
pub fn write_memory(memories_dir: &Path, name: &str, content: &str) -> Result<()> {
    // Policy file is always writable
    if name == POLICY_FILENAME {
        return write_policy(memories_dir, content);
    }
    let policy = MemoryPolicy::load(memories_dir);
    if policy.is_read_only(name) {
        bail!("memory '{name}' is read-only (matches policy pattern)");
    }
    let path = resolve_memory_path(memories_dir, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}

/// Write content to a memory file, resolving the tier automatically.
/// Records an audit event via `recorder` when provided.
pub fn write_memory_tiered(
    project_dir: &Path,
    global_dir: Option<&Path>,
    name: &str,
    content: &str,
) -> Result<MemoryTier> {
    write_memory_tiered_rec(project_dir, global_dir, name, content, None)
}

/// Write content to a memory file with optional audit recording.
pub fn write_memory_tiered_rec(
    project_dir: &Path,
    global_dir: Option<&Path>,
    name: &str,
    content: &str,
    recorder: Option<&dyn AuditRecorder>,
) -> Result<MemoryTier> {
    // Explicit global prefix
    let (effective_name, force_tier) = if let Some(stripped) = name.strip_prefix("global/") {
        (stripped.trim_start_matches('/'), Some(MemoryTier::Global))
    } else {
        (name, None)
    };

    if effective_name == POLICY_FILENAME {
        let dir = match force_tier {
            Some(MemoryTier::Global) => global_dir
                .ok_or_else(|| anyhow::anyhow!("global memory directory not available"))?,
            _ => &project_dir.join(".codelens").join("memories"),
        };
        write_policy(dir, content)?;
        return Ok(force_tier.unwrap_or(MemoryTier::Project));
    }

    let loc = resolve_memory_tier(name, project_dir, global_dir);
    let tier_dir = &loc.dir;
    let policy = MemoryPolicy::load(tier_dir);
    if policy.is_read_only(effective_name) {
        bail!("memory '{name}' is read-only (matches policy pattern)");
    }
    let path = resolve_memory_path(tier_dir, effective_name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    std::fs::write(&path, content)?;
    if let Some(rec) = recorder {
        let event = if is_new {
            MemoryAuditEvent::Created {
                tier: loc.tier,
                path: path.to_string_lossy().to_string(),
            }
        } else {
            MemoryAuditEvent::Updated {
                tier: loc.tier,
                path: path.to_string_lossy().to_string(),
            }
        };
        rec.record(&event);
    }
    Ok(loc.tier)
}

/// Delete a memory file.  Rejects deletes on read-only entries.
pub fn delete_memory(memories_dir: &Path, name: &str) -> Result<()> {
    if name == POLICY_FILENAME {
        bail!("cannot delete the policy file; write an empty policy instead");
    }
    let policy = MemoryPolicy::load(memories_dir);
    if policy.is_read_only(name) {
        bail!("memory '{name}' is read-only and cannot be deleted");
    }
    let path = resolve_memory_path(memories_dir, name)?;
    if !path.is_file() {
        bail!("memory not found: {name}");
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

/// Delete a memory file from the appropriate tier.
/// Records an audit event via `recorder` when provided.
pub fn delete_memory_tiered(
    project_dir: &Path,
    global_dir: Option<&Path>,
    name: &str,
) -> Result<MemoryTier> {
    delete_memory_tiered_rec(project_dir, global_dir, name, None)
}

/// Delete a memory file from the appropriate tier with optional audit recording.
pub fn delete_memory_tiered_rec(
    project_dir: &Path,
    global_dir: Option<&Path>,
    name: &str,
    recorder: Option<&dyn AuditRecorder>,
) -> Result<MemoryTier> {
    let effective_name = name.trim_start_matches("global/");
    let loc = resolve_memory_tier(name, project_dir, global_dir);
    if effective_name == POLICY_FILENAME {
        bail!("cannot delete the policy file; write an empty policy instead");
    }
    let policy = MemoryPolicy::load(&loc.dir);
    if policy.is_read_only(effective_name) {
        bail!("memory '{name}' is read-only and cannot be deleted");
    }
    if !loc.path.is_file() {
        bail!("memory not found: {}", effective_name);
    }
    let path_str = loc.path.to_string_lossy().to_string();
    std::fs::remove_file(&loc.path)?;
    if let Some(rec) = recorder {
        rec.record(&MemoryAuditEvent::Deleted {
            tier: loc.tier,
            path: path_str,
        });
    }
    Ok(loc.tier)
}

/// Rename a memory file.  Rejects if source is read-only or target exists.
pub fn rename_memory(memories_dir: &Path, old_name: &str, new_name: &str) -> Result<()> {
    let policy = MemoryPolicy::load(memories_dir);
    if policy.is_read_only(old_name) {
        bail!("memory '{old_name}' is read-only and cannot be renamed");
    }
    if policy.is_read_only(new_name) {
        bail!("target name '{new_name}' is read-only and cannot be overwritten");
    }
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

fn write_policy(memories_dir: &Path, content: &str) -> Result<()> {
    if let Some(parent) = memories_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(memories_dir)?;
    std::fs::write(memories_dir.join(POLICY_FILE_BASENAME), content)?;
    Ok(())
}

/// Read the current policy content for a memories directory.
pub fn read_policy(memories_dir: &Path) -> Result<String> {
    let path = memories_dir.join(POLICY_FILE_BASENAME);
    if path.is_file() {
        std::fs::read_to_string(&path).with_context(|| "failed to read memory policy")
    } else {
        Ok(String::new())
    }
}
