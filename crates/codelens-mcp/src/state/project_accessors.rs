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

    /// Whether `path` passes the root checks a binding applies (marker
    /// detection, the home and markerless policies, the home guard) without
    /// building anything.
    #[cfg(feature = "http")]
    pub(crate) fn project_scope_resolves(&self, path: &str) -> bool {
        codelens_engine::ProjectRoot::new(path).is_ok_and(|project| {
            super::project_runtime::home_binding_guard(project.as_path()).is_ok()
        })
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
    ///
    /// The build runs on a background thread, one per scope. A request waits
    /// for it up to the bind budget (`CODELENS_BIND_BUDGET_SECS`, default 20)
    /// and otherwise returns a retryable `IndexNotReady`; the build keeps
    /// going, and the next request after it finishes installs the runtime.
    fn resolve_cached_project_context(
        &self,
        project: codelens_engine::ProjectRoot,
        scope: &str,
    ) -> anyhow::Result<Arc<super::project_runtime::ProjectContext>> {
        let deadline = std::time::Instant::now() + bind_budget();
        self.install_finished_builds();
        let ticket = {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(scope) {
                return Ok(cached);
            }
            match cache.in_flight.get(scope) {
                Some(ticket) => Arc::clone(ticket),
                None => {
                    let ticket = Arc::new(super::project_runtime::InFlightBuild::new());
                    cache
                        .in_flight
                        .insert(scope.to_owned(), Arc::clone(&ticket));
                    #[cfg(test)]
                    cache.record_build_attempt(scope);
                    spawn_project_build(project, Arc::clone(&ticket))?;
                    ticket
                }
            }
        };

        if !ticket.wait_until(deadline) {
            return Err(still_building(scope, &ticket));
        }

        // Exactly one caller takes the outcome; the others wait for the
        // runtime it installs.
        let outcome = {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(scope) {
                return Ok(cached);
            }
            if cache
                .in_flight
                .get(scope)
                .is_some_and(|current| Arc::ptr_eq(current, &ticket))
            {
                cache.in_flight.remove(scope);
            }
            ticket.take()
        };
        let built = match outcome {
            Some(Ok(context)) => Arc::new(context),
            Some(Err(error)) => {
                ticket.record_failure(&error);
                return Err(error);
            }
            None => return self.await_installed(scope, &ticket, deadline),
        };
        Ok(self.install_built_context(scope, built))
    }

    /// Install builds that finished after every waiting request gave up.
    /// Without this, a runtime nobody asked for again stayed outside the
    /// cache, holding its writer lease and watcher, beyond LRU eviction.
    fn install_finished_builds(&self) {
        let finished = {
            let mut cache = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let scopes = cache
                .in_flight
                .iter()
                .filter(|(_, ticket)| ticket.is_finished())
                .map(|(scope, _)| scope.clone())
                .collect::<Vec<_>>();
            scopes
                .into_iter()
                .filter_map(|scope| cache.in_flight.remove(&scope).map(|ticket| (scope, ticket)))
                .collect::<Vec<_>>()
        };
        for (scope, ticket) in finished {
            match ticket.take() {
                Some(Ok(context)) => {
                    self.install_built_context(&scope, Arc::new(context));
                }
                Some(Err(error)) => {
                    ticket.record_failure(&error);
                    tracing::warn!(
                        project = %scope,
                        error = %format!("{error:#}"),
                        "background project runtime build failed; the next request retries it"
                    );
                }
                None => {}
            }
        }
    }

    /// Another caller took the finished build and is installing it.
    fn await_installed(
        &self,
        scope: &str,
        ticket: &super::project_runtime::InFlightBuild,
        deadline: std::time::Instant,
    ) -> anyhow::Result<Arc<super::project_runtime::ProjectContext>> {
        loop {
            if let Some(cached) = self
                .project_context_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(scope)
            {
                return Ok(cached);
            }
            if let Some(failure) = ticket.failure() {
                return Err(anyhow::Error::new(
                    crate::error::CodeLensError::IndexNotReady(format!(
                        "the project runtime build for `{scope}` failed ({failure}); the next \
                         request starts a new build"
                    )),
                ));
            }
            if std::time::Instant::now() >= deadline {
                return Err(still_building(scope, ticket));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn install_built_context(
        &self,
        scope: &str,
        built: Arc<super::project_runtime::ProjectContext>,
    ) -> Arc<super::project_runtime::ProjectContext> {
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
            return cached;
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
        built
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

/// Default ceiling on how long a request waits for its project's runtime
/// build. Override with `CODELENS_BIND_BUDGET_SECS`. Well under the 60 s at
/// which Claude Code gave up on `prepare_harness_session` (23 timeouts in two
/// weeks), so the client always gets an answer it can act on.
const DEFAULT_BIND_BUDGET_SECS: u64 = 20;

thread_local! {
    static SKIP_BIND_WAIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While alive, binds on this thread do not wait for an in-flight build:
/// a cached runtime still binds, anything else returns `IndexNotReady` at
/// once (and a missing build is still started).
pub(crate) struct NoBindWait(());

impl NoBindWait {
    pub(crate) fn enter() -> Self {
        SKIP_BIND_WAIT.with(|cell| cell.set(true));
        Self(())
    }
}

impl Drop for NoBindWait {
    fn drop(&mut self) {
        SKIP_BIND_WAIT.with(|cell| cell.set(false));
    }
}

fn bind_budget() -> std::time::Duration {
    if SKIP_BIND_WAIT.with(std::cell::Cell::get) {
        return std::time::Duration::ZERO;
    }
    #[cfg(test)]
    if let Some((budget, _)) = TEST_BIND_OVERRIDE.with(std::cell::Cell::get) {
        return budget;
    }
    std::time::Duration::from_secs(
        std::env::var("CODELENS_BIND_BUDGET_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_BIND_BUDGET_SECS),
    )
}

fn still_building(scope: &str, ticket: &super::project_runtime::InFlightBuild) -> anyhow::Error {
    anyhow::Error::new(crate::error::CodeLensError::IndexNotReady(format!(
        "project runtime for `{scope}` is still being built ({}s so far); it continues in \
         the background, retry in a few seconds",
        ticket.started.elapsed().as_secs()
    )))
}

#[cfg(test)]
thread_local! {
    /// Test hook: `(bind budget, build delay)` for requests on this thread
    /// and the builds they start. The delay stands in for a stalled
    /// filesystem (the sandboxd case). Thread-local, so parallel tests that
    /// build runtimes are unaffected.
    pub(crate) static TEST_BIND_OVERRIDE: std::cell::Cell<Option<(std::time::Duration, u64)>> =
        const { std::cell::Cell::new(None) };
}

fn spawn_project_build(
    project: codelens_engine::ProjectRoot,
    ticket: Arc<super::project_runtime::InFlightBuild>,
) -> anyhow::Result<()> {
    #[cfg(test)]
    let delay_ms = TEST_BIND_OVERRIDE
        .with(std::cell::Cell::get)
        .map_or(0, |(_, delay)| delay);
    std::thread::Builder::new()
        .name("codelens-project-build".to_owned())
        .spawn(move || {
            #[cfg(test)]
            if delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            let outcome = AppState::build_project_runtime_context(project, true);
            ticket.complete(outcome);
        })
        .map(drop)
        .map_err(anyhow::Error::new)
}

#[cfg(test)]
mod bind_budget_tests {
    use super::TEST_BIND_OVERRIDE;
    use crate::AppState;
    use crate::tool_defs::ToolPreset;
    use std::sync::Arc;

    /// Zero bind budget and a slow build for requests on this thread.
    struct SlowBuild;

    impl SlowBuild {
        fn with_delay(delay_ms: u64) -> Self {
            TEST_BIND_OVERRIDE.with(|cell| cell.set(Some((std::time::Duration::ZERO, delay_ms))));
            Self
        }
    }

    impl Drop for SlowBuild {
        fn drop(&mut self) {
            TEST_BIND_OVERRIDE.with(|cell| cell.set(None));
        }
    }

    fn temp_project(label: &str) -> codelens_engine::ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-bind-budget-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "pub fn budget_probe() {}\n").unwrap();
        codelens_engine::ProjectRoot::new_exact(&dir).unwrap()
    }

    fn is_index_not_ready(error: &anyhow::Error) -> bool {
        matches!(
            error.downcast_ref::<crate::error::CodeLensError>(),
            Some(crate::error::CodeLensError::IndexNotReady(_))
        )
    }

    #[test]
    fn a_slow_build_returns_at_the_budget_and_installs_on_a_later_call() {
        let _slow = SlowBuild::with_delay(400);
        let state = Arc::new(AppState::new_minimal(
            temp_project("default"),
            ToolPreset::Balanced,
        ));
        let project = temp_project("slow");
        let path = project.as_path().to_string_lossy().to_string();

        let first = state.bind_request_project_scope(&path).map(drop);
        let follower = state.bind_request_project_scope(&path).map(drop);
        assert!(
            first.as_ref().is_err_and(is_index_not_ready),
            "the leader must return at the budget: {first:?}"
        );
        assert!(
            follower.as_ref().is_err_and(is_index_not_ready),
            "a follower must join the same build: {follower:?}"
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let installed = loop {
            match state.bind_request_project_scope(&path) {
                Ok(_binding) => break state.symbol_index(),
                Err(error) if is_index_not_ready(&error) => {
                    assert!(std::time::Instant::now() < deadline, "build never finished");
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("unexpected error: {error:#}"),
            }
        };
        let _again = state.bind_request_project_scope(&path).unwrap();
        assert!(Arc::ptr_eq(&installed, &state.symbol_index()));
        let attempts = state
            .project_context_cache
            .lock()
            .unwrap()
            .build_attempt_count(project.as_path().to_string_lossy().as_ref());
        assert_eq!(
            attempts, 1,
            "callers during the build must not start another"
        );
    }

    #[test]
    fn waiters_on_a_failed_build_return_without_waiting_out_the_budget() {
        let state = Arc::new(AppState::new_minimal(
            temp_project("default-fail"),
            ToolPreset::Balanced,
        ));
        let project = temp_project("busy");
        // Another writer holds the project, so the build fails.
        let _held = super::super::project_runtime_lease::ProjectRuntimeLease::try_acquire(&project)
            .expect("hold the writer lease");
        let path = project.as_path().to_string_lossy().to_string();
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let started = std::time::Instant::now();
        let handles = (0..4)
            .map(|_| {
                let state = Arc::clone(&state);
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    TEST_BIND_OVERRIDE
                        .with(|cell| cell.set(Some((std::time::Duration::from_secs(10), 200))));
                    barrier.wait();
                    state.bind_request_project_scope(&path).map(drop)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("bind thread panicked"))
            .collect::<Vec<_>>();

        assert!(results.iter().all(Result::is_err), "{results:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "waiters must not sit out the 10 s budget after the build failed"
        );
    }

    #[test]
    fn an_explicit_rebind_does_not_wait_on_the_previous_project() {
        let state = Arc::new(AppState::new_minimal(
            temp_project("default-nowait"),
            ToolPreset::Balanced,
        ));
        let project = temp_project("nowait");
        let path = project.as_path().to_string_lossy().to_string();
        TEST_BIND_OVERRIDE.with(|cell| cell.set(Some((std::time::Duration::from_secs(10), 1_500))));
        let started = std::time::Instant::now();

        let result = {
            let _no_wait = super::NoBindWait::enter();
            state.bind_request_project_scope(&path).map(drop)
        };

        TEST_BIND_OVERRIDE.with(|cell| cell.set(None));
        assert!(result.as_ref().is_err_and(is_index_not_ready), "{result:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(
            state
                .project_context_cache
                .lock()
                .unwrap()
                .in_flight
                .contains_key(&path),
            "the build must still start"
        );
    }

    #[test]
    fn a_build_nobody_came_back_for_is_installed_by_the_next_bind() {
        let state = Arc::new(AppState::new_minimal(
            temp_project("default-sweep"),
            ToolPreset::Balanced,
        ));
        let abandoned = temp_project("abandoned");
        let abandoned_scope = abandoned.as_path().to_string_lossy().to_string();
        {
            let _slow = SlowBuild::with_delay(100);
            let gave_up = state.bind_request_project_scope(&abandoned_scope).map(drop);
            assert!(
                gave_up.as_ref().is_err_and(is_index_not_ready),
                "{gave_up:?}"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(600));

        let other = temp_project("other");
        let _binding = state
            .bind_request_project_scope(other.as_path().to_string_lossy().as_ref())
            .unwrap();

        let cache = state.project_context_cache.lock().unwrap();
        assert!(cache.entries.contains_key(&abandoned_scope));
        assert!(!cache.in_flight.contains_key(&abandoned_scope));
    }
}
