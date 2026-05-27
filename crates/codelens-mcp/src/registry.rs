//! Project + memory registry scaffold (P3 passive half, refactored).
//!
//! Memory tier logic and `global_memory_dir` live in `codelens_engine::memory`.
//! This module provides the project-registry surface and thin wrappers
//! (`MemoryScope`) that map engine types to the MCP reporting contract.

use crate::AppState;
use codelens_engine::memory::{self, MemoryTier};
use serde::Serialize;

/// Which tier a memory record belongs to — mirrors the engine's `MemoryTier`
/// for JSON serialization compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Project,
    Global,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

impl From<MemoryTier> for MemoryScope {
    fn from(tier: MemoryTier) -> Self {
        match tier {
            MemoryTier::Project => Self::Project,
            MemoryTier::Global => Self::Global,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryScopeReport {
    pub scope: &'static str,
    pub path: Option<String>,
    pub exists: bool,
    pub mutation_wired: bool,
}

/// Enumerate memory scopes with their current paths and wiring status.
/// Both project and global scopes now have full mutation wiring.
pub fn enumerate_memory_scopes(state: &AppState) -> Vec<MemoryScopeReport> {
    let project_dir = state.memories_dir();
    let global_dir = memory::global_memory_dir();
    vec![
        MemoryScopeReport {
            scope: MemoryScope::Project.as_str(),
            path: Some(project_dir.to_string_lossy().into_owned()),
            exists: project_dir.is_dir(),
            mutation_wired: true,
        },
        MemoryScopeReport {
            scope: MemoryScope::Global.as_str(),
            path: global_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            exists: global_dir.as_ref().is_some_and(|p| p.is_dir()),
            mutation_wired: true,
        },
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRegistryEntry {
    pub name: String,
    pub path: String,
    pub is_active: bool,
    pub has_project_memory: bool,
}

/// Snapshot of the active project + secondary projects, without requiring a
/// tool call. Lets resource consumers render a registry view alongside
/// memory scopes and backend capabilities.
pub fn enumerate_projects(state: &AppState) -> Vec<ProjectRegistryEntry> {
    let active = state.project();
    let active_name = active
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let active_path = active.as_path().to_string_lossy().into_owned();
    let has_memories = state.memories_dir().is_dir();

    let mut entries = vec![ProjectRegistryEntry {
        name: active_name,
        path: active_path,
        is_active: true,
        has_project_memory: has_memories,
    }];

    for (name, path) in state.list_secondary_projects() {
        entries.push(ProjectRegistryEntry {
            name,
            path,
            is_active: false,
            has_project_memory: false,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_scope_strings_are_stable() {
        assert_eq!(MemoryScope::Project.as_str(), "project");
        assert_eq!(MemoryScope::Global.as_str(), "global");
    }

    #[test]
    fn global_memory_dir_uses_home() {
        // Only assert the structural invariant (ends with
        // `.codelens/memories`) so the test remains CI-independent
        // regardless of the host's actual HOME value.
        if let Some(path) = memory::global_memory_dir() {
            let as_string = path.to_string_lossy();
            assert!(
                as_string.ends_with(".codelens/memories"),
                "global memory dir should live under .codelens/memories, got {as_string}"
            );
        }
    }
}
