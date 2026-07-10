//! Memory tools — tiered project/global memory with policy enforcement.
//!
//! Wraps the engine's `memory` module with MCP tool handlers that support:
//! - **Project tier**: `.codelens/memories/` (default, always available)
//! - **Global tier**: `$HOME/.codelens/memories/` (accessed via `scope` param
//!   or `global/` name prefix)
//! - **Policy enforcement**: read-only entries reject writes/deletes;
//!   ignored entries are hidden from listing
//! - **Archive**: move entries to `.archive/` instead of deleting them

use super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::memory::{self, MemoryPolicy, MemoryTier};
use serde_json::{Value, json};

fn scoped_memory_dir(
    state: &AppState,
    arguments: &Value,
) -> Result<(std::path::PathBuf, MemoryTier), CodeLensError> {
    match arguments
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("project")
    {
        "project" => Ok((state.memories_dir(), MemoryTier::Project)),
        "global" => memory::global_memory_dir()
            .map(|dir| (dir, MemoryTier::Global))
            .ok_or_else(|| CodeLensError::NotFound("global memory dir not available".into())),
        scope => Err(CodeLensError::Validation(format!(
            "unsupported memory scope: {scope}"
        ))),
    }
}

// ── List memories ────────────────────────────────────────────────────────────

pub fn list_memories(state: &AppState, arguments: &Value) -> ToolResult {
    let topic = arguments.get("topic").and_then(|v| v.as_str());
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("project");

    let global_dir = memory::global_memory_dir();

    match scope {
        "global" => {
            if let Some(gdir) = global_dir.as_ref() {
                let names = memory::list_memory_names(gdir, topic);
                Ok((
                    json!({
                        "scope": "global",
                        "topic": topic,
                        "count": names.len(),
                        "memories": names.iter().map(|n| json!({
                            "name": n,
                            "path": format!("global/{}",  n),
                            "scope": "global",
                            "tier": "global"
                        })).collect::<Vec<_>>()
                    }),
                    success_meta(BackendKind::Memory, 1.0),
                ))
            } else {
                Ok((
                    json!({"scope": "global", "topic": topic, "count": 0, "memories": [], "error": "global memory dir not available"}),
                    success_meta(BackendKind::Memory, 0.5),
                ))
            }
        }
        "both" => {
            let all = memory::list_all_memory_names(
                state.project().as_path(),
                global_dir.as_deref(),
                topic,
            );
            Ok((
                json!({
                    "scope": "both",
                    "topic": topic,
                    "count": all.len(),
                    "memories": all.iter().map(|(name, tier)| json!({
                        "name": name,
                        "scope": tier.as_str(),
                        "tier": tier.as_str(),
                        "path": format!(".codelens/memories/{}.md",  name.trim_start_matches("global/"))
                    })).collect::<Vec<_>>()
                }),
                success_meta(BackendKind::Memory, 1.0),
            ))
        }
        _ => {
            // "project" (default)
            let names = memory::list_memory_names(&state.memories_dir(), topic);
            Ok((
                json!({
                    "scope": "project",
                    "topic": topic,
                    "count": names.len(),
                    "memories": names.iter().map(|n| json!({
                        "name": n,
                        "path": format!(".codelens/memories/{n}.md"),
                        "scope": "project",
                        "tier": "project"
                    })).collect::<Vec<_>>()
                }),
                success_meta(BackendKind::Memory, 1.0),
            ))
        }
    }
}

// ── Read memory ──────────────────────────────────────────────────────────────

pub fn read_memory(state: &AppState, arguments: &Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");

    let global_dir = memory::global_memory_dir();

    if scope == "auto" {
        let effective_name = name.trim_start_matches("global/");
        match memory::read_memory_from_tier(state.project().as_path(), global_dir.as_deref(), name)
        {
            Ok((content, tier)) => Ok((
                json!({
                    "memory_name": effective_name,
                    "content": content,
                    "scope": tier.as_str(),
                    "tier": tier.as_str()
                }),
                success_meta(BackendKind::Memory, 1.0),
            )),
            Err(_) => Err(CodeLensError::NotFound(format!("Memory: {effective_name}"))),
        }
    } else {
        let dir = match scope {
            "global" => global_dir
                .ok_or_else(|| CodeLensError::NotFound("global memory dir not available".into()))?,
            _ => state.memories_dir(),
        };
        let content = memory::read_memory(&dir, name)
            .map_err(|_| CodeLensError::NotFound(format!("Memory: {name}")))?;
        Ok((
            json!({"memory_name": name, "content": content, "scope": scope, "tier": scope}),
            success_meta(BackendKind::Memory, 1.0),
        ))
    }
}

// ── Write memory ─────────────────────────────────────────────────────────────

pub fn write_memory(state: &AppState, arguments: &Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let content = required_string(arguments, "content")?;
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("project");

    let global_dir = memory::global_memory_dir();

    let tier = match scope {
        "global" => {
            let gdir = global_dir.as_ref();
            if let Some(gdir) = gdir {
                memory::write_memory_tiered(
                    state.project().as_path(),
                    Some(gdir),
                    &format!("global/{name}"),
                    content,
                )?
            } else {
                return Err(CodeLensError::NotFound(
                    "global memory dir not available".into(),
                ));
            }
        }
        _ => {
            memory::write_memory(&state.memories_dir(), name, content)?;
            MemoryTier::Project
        }
    };

    Ok((
        json!({"status": "ok", "memory_name": name, "scope": tier.as_str(), "tier": tier.as_str()}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

// ── Delete memory ────────────────────────────────────────────────────────────

pub fn delete_memory(state: &AppState, arguments: &Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("project");

    let global_dir = memory::global_memory_dir();

    let tier = match scope {
        "global" => {
            if let Some(gdir) = global_dir.as_ref() {
                memory::delete_memory_tiered(
                    state.project().as_path(),
                    Some(gdir),
                    &format!("global/{name}"),
                )?
            } else {
                return Err(CodeLensError::NotFound(
                    "global memory dir not available".into(),
                ));
            }
        }
        _ => {
            memory::delete_memory(&state.memories_dir(), name)?;
            MemoryTier::Project
        }
    };

    Ok((
        json!({"status": "ok", "memory_name": name, "scope": tier.as_str(), "tier": tier.as_str()}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

// ── Rename memory ────────────────────────────────────────────────────────────

pub fn rename_memory(state: &AppState, arguments: &Value) -> ToolResult {
    let old_name = required_string(arguments, "old_name")?;
    let new_name = required_string(arguments, "new_name")?;
    let (dir, tier) = scoped_memory_dir(state, arguments)?;
    memory::rename_memory(&dir, old_name, new_name)?;
    Ok((
        json!({"status": "ok", "old_name": old_name, "new_name": new_name, "scope": tier.as_str(), "tier": tier.as_str()}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

// ── Archive memory (new) ─────────────────────────────────────────────────────

pub fn archive_memory(state: &AppState, arguments: &Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let (dir, tier) = scoped_memory_dir(state, arguments)?;
    memory::archive_memory(&dir, name)?;
    Ok((
        json!({"status": "archived", "memory_name": name, "scope": tier.as_str(), "tier": tier.as_str()}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

// ── Restore from archive (new) ───────────────────────────────────────────────

pub fn restore_memory(state: &AppState, arguments: &Value) -> ToolResult {
    let name = required_string(arguments, "memory_name")?;
    let (dir, tier) = scoped_memory_dir(state, arguments)?;
    memory::restore_archived(&dir, name)?;
    Ok((
        json!({"status": "restored", "memory_name": name, "scope": tier.as_str(), "tier": tier.as_str()}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

// ── List archived (new) ──────────────────────────────────────────────────────

pub fn list_archived(state: &AppState, arguments: &Value) -> ToolResult {
    let (dir, tier) = scoped_memory_dir(state, arguments)?;
    let archived = memory::list_archived(&dir)?;
    Ok((
        json!({"scope": tier.as_str(), "tier": tier.as_str(), "count": archived.len(), "memories": archived}),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

// ── Read policy (new) ────────────────────────────────────────────────────────

pub fn read_policy(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("project");
    let dir = match scope {
        "global" => match memory::global_memory_dir() {
            Some(d) => d,
            None => {
                return Ok((
                    json!({"scope": "global", "policy": "", "exists": false}),
                    success_meta(BackendKind::Memory, 0.5),
                ));
            }
        },
        _ => state.memories_dir(),
    };
    let policy = MemoryPolicy::load(&dir);
    let raw_content = memory::read_policy(&dir).unwrap_or_default();
    Ok((
        json!({
            "scope": scope,
            "policy_raw": raw_content,
            "read_only_patterns": policy.read_only,
            "ignored_patterns": policy.ignored,
            "exists": dir.join(".policy").is_file()
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}
