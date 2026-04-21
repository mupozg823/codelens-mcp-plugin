//! Project and memory registry reporting.
//!
//! CodeLens exposes the currently supported memory scope and the active plus
//! registered secondary projects without requiring extra tool calls.

use crate::AppState;
use serde::Serialize;

/// Which tier a memory record belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Project,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
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

/// Enumerate currently supported memory scopes.
pub fn enumerate_memory_scopes(state: &AppState) -> Vec<MemoryScopeReport> {
    let project_dir = state.memories_dir();
    vec![MemoryScopeReport {
        scope: MemoryScope::Project.as_str(),
        path: Some(project_dir.to_string_lossy().into_owned()),
        exists: project_dir.is_dir(),
        mutation_wired: true,
    }]
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRegistryEntry {
    pub name: String,
    pub path: String,
    pub is_active: bool,
    pub has_project_memory: bool,
}

/// Snapshot of the active project + secondary projects, without requiring a
/// tool call.
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
    }
}
