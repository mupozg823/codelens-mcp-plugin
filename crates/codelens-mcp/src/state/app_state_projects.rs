use super::*;
use std::sync::Arc;

impl AppState {
    /// Get the active project root. Clones the ProjectRoot (just a PathBuf).
    pub(crate) fn project(&self) -> ProjectRoot {
        self.active_project_context()
            .map(|context| context.project.clone())
            .unwrap_or_else(|| self.project_runtime.default_project.clone())
    }

    /// Get the active symbol index.
    pub(crate) fn symbol_index(&self) -> Arc<SymbolIndex> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.symbol_index))
            .unwrap_or_else(|| Arc::clone(&self.project_runtime.default_symbol_index))
    }

    /// Get the active graph cache.
    pub(crate) fn graph_cache(&self) -> Arc<GraphCache> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.graph_cache))
            .unwrap_or_else(|| Arc::clone(&self.project_runtime.default_graph_cache))
    }

    /// Get the active memories directory.
    pub(crate) fn memories_dir(&self) -> PathBuf {
        self.active_project_context()
            .map(|context| context.memories_dir.clone())
            .unwrap_or_else(|| self.project_runtime.default_memories_dir.clone())
    }

    /// Get the active analysis cache directory.
    pub(crate) fn analysis_dir(&self) -> PathBuf {
        self.active_project_context()
            .map(|context| context.analysis_dir.clone())
            .unwrap_or_else(|| self.project_runtime.default_analysis_dir.clone())
    }

    #[allow(dead_code)]
    pub(crate) fn artifact_store(&self) -> &AnalysisArtifactStore {
        &self.analysis_runtime.artifact_store
    }

    pub(crate) fn audit_dir(&self) -> PathBuf {
        self.active_project_context()
            .map(|context| context.audit_dir.clone())
            .unwrap_or_else(|| self.project_runtime.default_audit_dir.clone())
    }

    pub(crate) fn watcher_stats(&self) -> Option<codelens_engine::WatcherStats> {
        self.active_project_context()
            .as_ref()
            .and_then(|context| context.watcher.as_ref().map(FileWatcher::stats))
            .or_else(|| {
                self.project_runtime
                    .default_watcher
                    .as_ref()
                    .map(FileWatcher::stats)
            })
    }

    pub(crate) fn watcher_running(&self) -> bool {
        self.watcher_stats()
            .map(|stats| stats.running)
            .unwrap_or(false)
    }

    /// Switch the active project at runtime. Creates a new index and graph cache.
    pub(crate) fn switch_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let scope = project.as_path().to_string_lossy().to_string();
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        if scope == self.default_project_scope() {
            self.activate_project_context(None);
            return Ok(name);
        }

        if let Some(current) = self.active_project_context()
            && current.project.as_path() == project.as_path()
        {
            return Ok(name);
        }

        let context = {
            let mut cache = self
                .project_runtime
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(&scope) {
                cached
            } else {
                let built = Arc::new(Self::build_project_runtime_context(project, true)?);
                cache.insert(scope.clone(), Arc::clone(&built));
                let active_scope = self.current_project_scope();
                let protected = [self.default_project_scope(), active_scope, scope.clone()];
                let protected_refs = protected.iter().map(String::as_str).collect::<Vec<_>>();
                let _evicted =
                    cache.evict_until_within_limit(PROJECT_CONTEXT_CACHE_LIMIT, &protected_refs);
                built
            }
        };
        self.activate_project_context(Some(context));
        Ok(name)
    }

    #[allow(dead_code)]
    pub(crate) fn reset_project(&self) {
        self.activate_project_context(None);
    }

    #[allow(dead_code)]
    pub(crate) fn is_default_project(&self) -> bool {
        self.active_project_context().is_none()
    }

    /// Access the LSP session pool. Pool uses internal per-session locking.
    pub(crate) fn lsp_pool(&self) -> Arc<LspSessionPool> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.lsp_pool))
            .unwrap_or_else(|| Arc::clone(&self.project_runtime.default_lsp_pool))
    }

    /// Register a secondary project for cross-project queries.
    pub(crate) fn add_secondary_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        let index = Arc::new(SymbolIndex::new(project.clone()));
        index.refresh_all()?;
        let mut map = self
            .project_runtime
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.insert(name.clone(), SecondaryProject { project, index });
        Ok(name)
    }

    pub(crate) fn remove_secondary_project(&self, name: &str) -> bool {
        let mut map = self
            .project_runtime
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.remove(name).is_some()
    }

    pub(crate) fn list_secondary_projects(&self) -> Vec<(String, String)> {
        let map = self
            .project_runtime
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

    pub(crate) fn query_secondary_project(
        &self,
        project_name: &str,
        symbol_name: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<codelens_engine::SymbolInfo>> {
        let map = self
            .project_runtime
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
