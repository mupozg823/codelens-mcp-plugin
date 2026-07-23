use std::sync::Arc;

use codelens_engine::{ProjectRoot, SymbolIndex};

use crate::state::AppState;

/// A read-only project registered for cross-project queries.
pub(crate) struct SecondaryProject {
    pub project: ProjectRoot,
    pub index: Arc<SymbolIndex>,
    /// Pins the same runtime authority used by primary/session project routing.
    _context: Arc<super::project_runtime::ProjectContext>,
}

impl AppState {
    /// Register a secondary project for cross-project queries.
    pub(crate) fn add_secondary_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let scope = project.as_path().to_string_lossy().to_string();
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        {
            let map = self
                .secondary_projects
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some((existing_name, _)) = map
                .iter()
                .find(|(_, secondary)| secondary.project.as_path() == project.as_path())
            {
                return Ok(existing_name.clone());
            }
            if let Some(existing) = map.get(&name) {
                anyhow::bail!(
                    "secondary project name collision: `{name}` already refers to `{}` instead of `{scope}`",
                    existing.project.as_path().display()
                );
            }
        }

        let context = self
            .project_context_for_scope(path)?
            .unwrap_or_else(|| Arc::clone(&self.default_context));
        let index = Arc::clone(&context.symbol_index);
        let mut map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some((existing_name, _)) = map
            .iter()
            .find(|(_, secondary)| secondary.project.as_path() == project.as_path())
        {
            return Ok(existing_name.clone());
        }
        if let Some(existing) = map.get(&name) {
            anyhow::bail!(
                "secondary project name collision: `{name}` already refers to `{}` instead of `{scope}`",
                existing.project.as_path().display()
            );
        }
        map.insert(
            name.clone(),
            SecondaryProject {
                project,
                index,
                _context: context,
            },
        );
        Ok(name)
    }

    /// Remove a secondary project.
    pub(crate) fn remove_secondary_project(&self, name: &str) -> bool {
        let mut map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.remove(name).is_some()
    }

    /// Get a snapshot of secondary project names and paths.
    pub(crate) fn list_secondary_projects(&self) -> Vec<(String, String)> {
        let map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.iter()
            .map(|(name, sp)| {
                (
                    name.clone(),
                    sp.project.as_path().to_string_lossy().to_string(),
                )
            })
            .collect()
    }

    /// Query symbols in a secondary project by name.
    pub(crate) fn query_secondary_project(
        &self,
        project_name: &str,
        symbol_name: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<codelens_engine::SymbolInfo>> {
        let map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let sp = map
            .get(project_name)
            .ok_or_else(|| anyhow::anyhow!("project '{}' not registered", project_name))?;
        sp.index
            .find_symbol(symbol_name, None, false, false, max_results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;

    #[test]
    fn registering_same_secondary_project_twice_reuses_existing_runtime() {
        let default_project = temp_project_root("secondary-default");
        let secondary_project = temp_project_root("secondary-reuse");
        std::fs::write(
            secondary_project.as_path().join("lib.rs"),
            "pub fn secondary_symbol() {}\n",
        )
        .unwrap();
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);

        let first_name = state
            .add_secondary_project(secondary_project.as_path().to_str().unwrap())
            .unwrap();
        let first_index = {
            let projects = state.secondary_projects.lock().unwrap();
            Arc::clone(&projects.get(&first_name).unwrap().index)
        };

        let second_name = state
            .add_secondary_project(secondary_project.as_path().to_str().unwrap())
            .expect("re-registering the same project should be idempotent");
        let second_index = {
            let projects = state.secondary_projects.lock().unwrap();
            Arc::clone(&projects.get(&second_name).unwrap().index)
        };

        assert_eq!(first_name, second_name);
        assert!(Arc::ptr_eq(&first_index, &second_index));
    }

    #[test]
    fn registering_default_project_as_secondary_reuses_default_runtime() {
        let default_project = temp_project_root("secondary-default-reuse");
        let state = AppState::new_minimal(default_project.clone(), ToolPreset::Balanced);
        let default_index = state.symbol_index();

        let name = state
            .add_secondary_project(default_project.as_path().to_str().unwrap())
            .expect("default runtime should be reusable as a secondary project");
        let secondary_index = {
            let projects = state.secondary_projects.lock().unwrap();
            Arc::clone(&projects.get(&name).unwrap().index)
        };

        assert!(Arc::ptr_eq(&default_index, &secondary_index));
    }

    #[test]
    fn secondary_then_primary_switch_reuses_one_runtime() {
        let default_project = temp_project_root("secondary-then-primary-default");
        let shared_project = temp_project_root("secondary-then-primary-shared");
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);

        let name = state
            .add_secondary_project(shared_project.as_path().to_str().unwrap())
            .unwrap();
        let secondary_index = {
            let projects = state.secondary_projects.lock().unwrap();
            Arc::clone(&projects.get(&name).unwrap().index)
        };
        state
            .switch_project(shared_project.as_path().to_str().unwrap())
            .expect("primary activation must reuse the secondary runtime");

        assert!(Arc::ptr_eq(&secondary_index, &state.symbol_index()));
    }

    #[test]
    fn primary_then_secondary_registration_reuses_one_runtime() {
        let default_project = temp_project_root("primary-then-secondary-default");
        let shared_project = temp_project_root("primary-then-secondary-shared");
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);
        state
            .switch_project(shared_project.as_path().to_str().unwrap())
            .unwrap();
        let primary_index = state.symbol_index();

        let name = state
            .add_secondary_project(shared_project.as_path().to_str().unwrap())
            .expect("secondary registration must reuse the primary runtime");
        let secondary_index = {
            let projects = state.secondary_projects.lock().unwrap();
            Arc::clone(&projects.get(&name).unwrap().index)
        };

        assert!(Arc::ptr_eq(&primary_index, &secondary_index));
    }
}
