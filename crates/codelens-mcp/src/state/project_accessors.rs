use std::sync::Arc;

use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, SymbolIndex};

use crate::error::CodeLensError;
use crate::runtime_types::WatcherFailureHealth;
use crate::state::AppState;

impl AppState {
    /// Get the active project root. Clones the ProjectRoot (just a PathBuf).
    pub(crate) fn project(&self) -> codelens_engine::ProjectRoot {
        self.active_project_context()
            .map(|context| context.project.clone())
            .unwrap_or_else(|| self.default_project.clone())
    }

    /// Get the active symbol index.
    pub(crate) fn symbol_index(&self) -> Arc<SymbolIndex> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.symbol_index))
            .unwrap_or_else(|| Arc::clone(&self.default_symbol_index))
    }

    pub(crate) fn watcher_failure_health(&self) -> WatcherFailureHealth {
        crate::state::watcher_health::watcher_failure_health(self)
    }

    pub(crate) fn prune_index_failures(&self) -> Result<WatcherFailureHealth, CodeLensError> {
        crate::state::watcher_health::prune_index_failures(self)
    }

    /// Get the active graph cache.
    pub(crate) fn graph_cache(&self) -> Arc<GraphCache> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.graph_cache))
            .unwrap_or_else(|| Arc::clone(&self.default_graph_cache))
    }

    /// Get the active memories directory.
    pub(crate) fn memories_dir(&self) -> std::path::PathBuf {
        self.active_project_context()
            .map(|context| context.memories_dir.clone())
            .unwrap_or_else(|| self.default_memories_dir.clone())
    }

    /// Get the active analysis cache directory.
    pub(crate) fn analysis_dir(&self) -> std::path::PathBuf {
        self.active_project_context()
            .map(|context| context.analysis_dir.clone())
            .unwrap_or_else(|| self.default_analysis_dir.clone())
    }

    pub(crate) fn audit_dir(&self) -> std::path::PathBuf {
        self.active_project_context()
            .map(|context| context.audit_dir.clone())
            .unwrap_or_else(|| self.default_audit_dir.clone())
    }

    pub(crate) fn watcher_stats(&self) -> Option<codelens_engine::WatcherStats> {
        self.active_project_context()
            .as_ref()
            .and_then(|context| context.watcher.as_ref().map(FileWatcher::stats))
            .or_else(|| self.default_watcher.as_ref().map(FileWatcher::stats))
    }

    pub(crate) fn watcher_running(&self) -> bool {
        self.watcher_stats()
            .map(|stats| stats.running)
            .unwrap_or(false)
    }

    /// Switch the active project at runtime. Creates a new index and graph cache.
    pub(crate) fn switch_project(&self, path: &str) -> anyhow::Result<String> {
        let project = codelens_engine::ProjectRoot::new(path)?;
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

        match self.active_project_context() {
            Some(current) if current.project.as_path() == project.as_path() => return Ok(name),
            _ => {}
        }

        let context = {
            let mut cache = self
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
                let _evicted = cache.evict_until_within_limit(
                    crate::state::PROJECT_CONTEXT_CACHE_LIMIT,
                    &protected_refs,
                );
                built
            }
        };
        self.activate_project_context(Some(context));
        Ok(name)
    }

    /// Access the LSP session pool. Pool uses internal per-session locking.
    pub(crate) fn lsp_pool(&self) -> Arc<LspSessionPool> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.lsp_pool))
            .unwrap_or_else(|| Arc::clone(&self.default_lsp_pool))
    }
}
