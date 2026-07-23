use std::sync::Arc;

use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, SymbolIndex};

use crate::error::CodeLensError;
use crate::runtime_types::WatcherFailureHealth;
use crate::sparse_symbol_cache::SparseSymbolCache;
use crate::state::AppState;

impl AppState {
    pub(crate) fn project_runtime_health_payload(&self) -> serde_json::Value {
        self.active_project_context()
            .unwrap_or_else(|| Arc::clone(&self.default_context))
            .runtime_health_payload()
    }

    /// Get the active project root. Clones the ProjectRoot (just a PathBuf).
    pub(crate) fn project(&self) -> codelens_engine::ProjectRoot {
        self.active_project_context()
            .map(|context| context.project.clone())
            .unwrap_or_else(|| self.default_context.project.clone())
    }

    /// `true` if the caller has explicitly activated a project (via
    /// `activate_project` or session-scoped routing). When `false`,
    /// `project()` falls back to the daemon's startup default — which
    /// is rarely the caller's actual cwd in HTTP/launchd setups.
    ///
    /// Workflows that surface project-scoped findings (rankings,
    /// blockers, prior analyses) should warn when this returns `false`,
    /// otherwise stale state from prior sessions may leak into the
    /// response. See issue #213.
    pub(crate) fn has_explicit_active_project(&self) -> bool {
        self.active_project_context().is_some()
    }

    /// Get the active symbol index.
    pub(crate) fn symbol_index(&self) -> Arc<SymbolIndex> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.symbol_index))
            .unwrap_or_else(|| Arc::clone(&self.default_context.symbol_index))
    }

    pub(crate) fn sparse_symbol_cache(&self) -> Arc<SparseSymbolCache> {
        Arc::clone(&self.sparse_symbol_cache)
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
            .unwrap_or_else(|| Arc::clone(&self.default_context.graph_cache))
    }

    /// Get the active memories directory.
    pub(crate) fn memories_dir(&self) -> std::path::PathBuf {
        self.active_project_context()
            .map(|context| context.memories_dir.clone())
            .unwrap_or_else(|| self.default_context.memories_dir.clone())
    }

    /// Get the active analysis cache directory (request-scoped project
    /// context first, then the daemon default).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn analysis_dir(&self) -> std::path::PathBuf {
        self.active_project_context()
            .map(|context| context.analysis_dir.clone())
            .unwrap_or_else(|| self.default_context.analysis_dir.clone())
    }

    pub(crate) fn audit_dir(&self) -> std::path::PathBuf {
        self.active_project_context()
            .map(|context| context.audit_dir.clone())
            .unwrap_or_else(|| self.default_context.audit_dir.clone())
    }

    pub(crate) fn watcher_stats(&self) -> Option<codelens_engine::WatcherStats> {
        self.active_project_context()
            .as_ref()
            .and_then(|context| context.watcher.as_ref().map(FileWatcher::stats))
            .or_else(|| {
                self.default_context
                    .watcher
                    .as_ref()
                    .map(FileWatcher::stats)
            })
    }

    /// Start-failure error of the active context's watcher, if any.
    /// `None` means the watcher is running or was intentionally not
    /// started. An explicit active context is authoritative — it never
    /// falls through to the daemon default's error.
    pub(crate) fn watcher_error(&self) -> Option<String> {
        self.active_project_context()
            .map(|context| context.watcher_error.clone())
            .unwrap_or_else(|| self.default_context.watcher_error.clone())
    }

    pub(crate) fn watcher_running(&self) -> bool {
        self.watcher_stats()
            .map(|stats| stats.running)
            .unwrap_or(false)
    }

    /// Resolve a project runtime context for `path` without mutating the
    /// daemon-global override. Returns `None` when `path` IS the daemon's
    /// default project (callers use the default resources directly).
    /// Get-or-build through the LRU context cache; evicted contexts have
    /// their resources shut down.
    pub(super) fn project_context_for_scope(
        &self,
        path: &str,
    ) -> anyhow::Result<Option<Arc<super::project_runtime::ProjectContext>>> {
        let project = codelens_engine::ProjectRoot::new(path)?;
        super::project_runtime::home_binding_guard(project.as_path())
            .map_err(anyhow::Error::new)?;
        self.reap_deleted_project_runtimes();
        let scope = project.as_path().to_string_lossy().to_string();
        if scope == self.default_project_scope() {
            return Ok(None);
        }
        let context = self.resolve_cached_project_context(project, &scope)?;
        Ok(Some(context))
    }

    /// Bind the CURRENT REQUEST (thread) to `path`, returning an RAII guard
    /// that restores the previous binding on drop. Never touches the global
    /// `project_override`, so concurrent sessions on different projects
    /// neither serialize nor clobber each other.
    pub(crate) fn bind_request_project_scope(
        &self,
        path: &str,
    ) -> anyhow::Result<super::project_runtime::RequestProjectGuard> {
        use super::project_runtime::RequestProjectBinding;
        let binding = match self.project_context_for_scope(path)? {
            None => RequestProjectBinding::Default,
            Some(context) => RequestProjectBinding::Context(context),
        };
        Ok(super::project_runtime::bind_request_project(binding))
    }

    /// Re-point the current request's binding at `path` in place (no new
    /// guard scope). Used when a session re-binds mid-call — the outer
    /// dispatch guard still restores the pre-request state on exit.
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    pub(crate) fn rebind_request_project_scope(&self, path: &str) -> anyhow::Result<()> {
        use super::project_runtime::RequestProjectBinding;
        let binding = match self.project_context_for_scope(path)? {
            None => RequestProjectBinding::Default,
            Some(context) => RequestProjectBinding::Context(context),
        };
        super::project_runtime::rebind_request_project(binding);
        Ok(())
    }

    /// Switch the active project at runtime. Creates a new index and graph cache.
    pub(crate) fn switch_project(&self, path: &str) -> anyhow::Result<String> {
        let project = codelens_engine::ProjectRoot::new(path)?;
        super::project_runtime::home_binding_guard(project.as_path())
            .map_err(anyhow::Error::new)?;
        self.reap_deleted_project_runtimes();
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

        let context = self.resolve_cached_project_context(project, &scope)?;

        self.activate_project_context(Some(context));
        Ok(name)
    }

    /// Get-or-build a non-default project context through the LRU cache.
    /// Evicted entries are retired before the active-session guard is released.
    fn resolve_cached_project_context(
        &self,
        project: codelens_engine::ProjectRoot,
        scope: &str,
    ) -> anyhow::Result<Arc<super::project_runtime::ProjectContext>> {
        let build_lock = {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(scope) {
                return Ok(cached);
            }
            cache.build_lock(scope)
        };

        // Only one thread may construct a runtime for this scope. Followers
        // wait without holding the cache mutex, then reuse the leader's entry.
        let _build_guard = build_lock.lock().unwrap_or_else(|p| p.into_inner());
        {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(scope) {
                return Ok(cached);
            }
            #[cfg(test)]
            cache.record_build_attempt(scope);
        }

        let built = Arc::new(Self::build_project_runtime_context(project, true)?);

        // Acquire after the potentially long build, immediately before cache
        // insertion. SessionStore project-path mutations require the matching
        // sessions write lock, so this read guard makes bind-vs-evict atomic.
        // Keep it through both cache selection and runtime retirement.
        #[cfg(feature = "http")]
        let active_session_paths = self
            .session_store
            .as_ref()
            .map(|store| store.active_project_paths_guard());

        let active_scope = self.current_project_scope();
        let default_scope = self.default_project_scope();

        let mut cache = self
            .project_context_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        if let Some(cached) = cache.get(scope) {
            drop(cache);
            built.shutdown_resources();
            return Ok(cached);
        }

        cache.insert(scope.to_owned(), Arc::clone(&built));
        let mut protected = vec![default_scope, active_scope, scope.to_owned()];
        #[cfg(feature = "http")]
        if let Some(paths) = active_session_paths.as_ref() {
            protected.extend(
                paths
                    .iter()
                    .filter_map(|path| codelens_engine::ProjectRoot::new(path).ok())
                    .map(|project| project.as_path().to_string_lossy().into_owned()),
            );
        }
        protected.sort();
        protected.dedup();
        let protected_refs = protected.iter().map(String::as_str).collect::<Vec<_>>();
        let evicted = cache
            .evict_until_within_limit(crate::state::PROJECT_CONTEXT_CACHE_LIMIT, &protected_refs);
        drop(cache);

        for context in evicted {
            #[cfg(feature = "scip-backend")]
            self.drop_scip_backend_for_project(context.project.as_path());
            context.shutdown_resources();
        }
        #[cfg(feature = "http")]
        drop(active_session_paths);
        Ok(built)
    }

    /// Sweep the per-project runtime registry and drop cached contexts whose
    /// root directory no longer exists (e.g. a removed git worktree). Removing
    /// the map entry lets the SQLite symbol-index handle the dead root was
    /// pinning close once any in-flight request still holding an `Arc` also
    /// finishes — active Arcs expire naturally, this only unlinks the map
    /// entry. Runs at project activation/binding; cost is one `Path::exists`
    /// per cached entry (cache is capped at `PROJECT_CONTEXT_CACHE_LIMIT`).
    fn reap_deleted_project_runtimes(&self) {
        let reaped = {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            cache.reap_deleted_roots()
        };
        for context in &reaped {
            tracing::info!(
                project = %context.project.as_path().display(),
                "reaped project runtime whose root directory no longer exists"
            );
        }
        // `reaped` drops here: for a runtime no live request still references,
        // this releases the last Arc and closes its SQLite handle.
    }

    /// Access the LSP session pool. Pool uses internal per-session locking.
    pub(crate) fn lsp_pool(&self) -> Arc<LspSessionPool> {
        self.active_project_context()
            .map(|context| Arc::clone(&context.lsp_pool))
            .unwrap_or_else(|| Arc::clone(&self.default_context.lsp_pool))
    }
}
