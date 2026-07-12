use super::AppState;
use crate::error::CodeLensError;
use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Holds project-specific resources that can be reused across rebinds.
pub(super) struct ProjectContext {
    pub(super) project: ProjectRoot,
    pub(super) symbol_index: Arc<SymbolIndex>,
    pub(super) graph_cache: Arc<GraphCache>,
    pub(super) lsp_pool: Arc<LspSessionPool>,
    pub(super) memories_dir: PathBuf,
    pub(super) analysis_dir: PathBuf,
    pub(super) audit_dir: PathBuf,
    /// Keeps the watcher alive so it continues to receive file-system events.
    pub(super) watcher: Option<FileWatcher>,
    /// `Some` only when a requested watcher failed to start — the index
    /// will NOT auto-update on edits while this is set. Always `None`
    /// for intentionally watcher-less (one-shot) constructions.
    pub(super) watcher_error: Option<String>,
}

impl ProjectContext {
    pub(super) fn shutdown_resources(&self) {
        if let Some(ref watcher) = self.watcher {
            watcher.stop();
        }
        self.lsp_pool.shutdown();
    }
}

#[derive(Default)]
pub(super) struct ProjectContextCache {
    pub(super) entries: HashMap<String, Arc<ProjectContext>>,
    pub(super) access_order: VecDeque<String>,
}

impl ProjectContextCache {
    pub(super) fn get(&mut self, scope: &str) -> Option<Arc<ProjectContext>> {
        let context = self.entries.get(scope).cloned()?;
        self.touch(scope);
        Some(context)
    }

    pub(super) fn insert(&mut self, scope: String, context: Arc<ProjectContext>) {
        self.entries.insert(scope.clone(), context);
        self.touch(&scope);
    }

    fn touch(&mut self, scope: &str) {
        self.access_order.retain(|entry| entry != scope);
        self.access_order.push_back(scope.to_owned());
    }

    pub(super) fn evict_until_within_limit(
        &mut self,
        limit: usize,
        protected_scopes: &[&str],
    ) -> Vec<Arc<ProjectContext>> {
        let mut evicted = Vec::new();
        while self.entries.len() > limit {
            let Some(oldest) = self.access_order.pop_front() else {
                break;
            };
            if protected_scopes.iter().any(|scope| *scope == oldest) {
                self.access_order.push_back(oldest);
                if self.access_order.iter().all(|scope| {
                    protected_scopes
                        .iter()
                        .any(|protected| protected == &scope.as_str())
                }) {
                    break;
                }
                continue;
            }
            if let Some(context) = self.entries.remove(&oldest) {
                evicted.push(context);
            }
        }
        evicted
    }

    /// Remove cached runtimes whose project root directory no longer exists on
    /// disk (e.g. a removed git worktree) and return them so the caller can
    /// observe/log the reap. Dropping the last `Arc` to a removed context
    /// releases the SQLite symbol-index handle the dead root was still holding
    /// open. Live roots are never touched.
    pub(super) fn reap_deleted_roots(&mut self) -> Vec<Arc<ProjectContext>> {
        let dead: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, context)| !context.project.as_path().exists())
            .map(|(scope, _)| scope.clone())
            .collect();
        let mut reaped = Vec::with_capacity(dead.len());
        for scope in dead {
            if let Some(context) = self.entries.remove(&scope) {
                self.access_order.retain(|entry| entry != &scope);
                reaped.push(context);
            }
        }
        reaped
    }
}

/// Reject a bind/activation whose canonical root is the process user's home
/// directory. Indexing the entire home tree pins the daemon and was the cause
/// of `prepare_harness_session(project=$HOME)` client-timeout hangs; a repo
/// *inside* home (`/Users/x/repo`) is unaffected — only the home root itself
/// is refused.
///
/// Pure over its inputs (`home` and `allow_home` are injected rather than read
/// from the environment) so the policy is unit-testable without touching
/// process state. `home == None` means the home directory could not be
/// determined — fail open rather than guess.
pub(super) fn ensure_project_root_not_home(
    candidate: &Path,
    home: Option<&Path>,
    allow_home: bool,
) -> Result<(), CodeLensError> {
    if allow_home {
        return Ok(());
    }
    let Some(home) = home else {
        return Ok(());
    };
    if canonical_or_owned(candidate) == canonical_or_owned(home) {
        return Err(CodeLensError::HomeRootRejected {
            root: candidate.display().to_string(),
        });
    }
    Ok(())
}

/// Canonicalize `path`, falling back to the path as-given when it cannot be
/// resolved (e.g. it does not exist yet). Canonicalization collapses symlinks
/// and `.`/`..` so `/Users/x` and `/Users/x/` (or a `/var`→`/private/var`
/// symlink on macOS) compare equal.
fn canonical_or_owned(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Production entry point for the home-root guard: reads `$HOME` and the
/// `CODELENS_ALLOW_HOME_PROJECT` escape hatch from the environment and
/// delegates to [`ensure_project_root_not_home`].
pub(super) fn home_binding_guard(candidate: &Path) -> Result<(), CodeLensError> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let allow_home = crate::env_compat::env_var_bool("CODELENS_ALLOW_HOME_PROJECT") == Some(true);
    ensure_project_root_not_home(candidate, home.as_deref(), allow_home)
}

/// Request-scoped project binding. `Default` pins the daemon's startup
/// default project — a session explicitly bound to it must not observe
/// another session's `project_override`. `Context` pins a specific
/// project runtime resolved from the context cache.
pub(crate) enum RequestProjectBinding {
    Default,
    Context(Arc<ProjectContext>),
}

thread_local! {
    /// Per-request project binding. Set by `ensure_session_project` (dispatch,
    /// tools/list, resources) and by analysis workers for the duration of a
    /// job. Read by `active_project_context` ahead of the global override so
    /// concurrent sessions bound to different projects never mutate — or
    /// serialize on — shared daemon state (the pre-#357 design switched the
    /// global override under a daemon-wide mutex on every call).
    static REQUEST_PROJECT_BINDING: std::cell::RefCell<Option<RequestProjectBinding>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard for a request-scoped project binding. Restores the previous
/// binding on drop so nested binds (e.g. `activate_project` inside
/// `prepare_harness_session`) unwind correctly even on panic.
pub(crate) struct RequestProjectGuard {
    previous: Option<RequestProjectBinding>,
}

impl Drop for RequestProjectGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        REQUEST_PROJECT_BINDING.with(|cell| {
            *cell.borrow_mut() = previous;
        });
    }
}

pub(super) fn bind_request_project(binding: RequestProjectBinding) -> RequestProjectGuard {
    let previous = REQUEST_PROJECT_BINDING.with(|cell| cell.borrow_mut().replace(binding));
    RequestProjectGuard { previous }
}

/// Replace the current request's binding in place without creating a new
/// guard scope. Used by `activate_project` when a session re-binds mid-call:
/// the dispatch-level guard still restores the pre-request state on exit.
pub(super) fn rebind_request_project(binding: RequestProjectBinding) {
    REQUEST_PROJECT_BINDING.with(|cell| {
        *cell.borrow_mut() = Some(binding);
    });
}

pub(super) fn active_project_context(state: &AppState) -> Option<Arc<ProjectContext>> {
    let request_binding = REQUEST_PROJECT_BINDING.with(|cell| match &*cell.borrow() {
        None => None,
        Some(RequestProjectBinding::Default) => Some(None),
        Some(RequestProjectBinding::Context(context)) => Some(Some(Arc::clone(context))),
    });
    if let Some(resolved) = request_binding {
        return resolved;
    }
    state
        .project_override
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .cloned()
}

pub(super) fn build_project_runtime_context(
    project: ProjectRoot,
    start_watcher: bool,
) -> anyhow::Result<ProjectContext> {
    let symbol_index = Arc::new(SymbolIndex::new(project.clone())?);
    if symbol_index
        .stats()
        .map(|s| s.indexed_files == 0)
        .unwrap_or(true)
    {
        let _ = symbol_index.refresh_all();
    }
    let graph_cache = Arc::new(GraphCache::new(30));
    let memories_dir = project.as_path().join(".codelens").join("memories");
    let analysis_dir = project.as_path().join(".codelens").join("analysis-cache");
    let audit_dir = project.as_path().join(".codelens").join("audit");
    let _ = fs::create_dir_all(&memories_dir);
    let _ = fs::create_dir_all(&analysis_dir);
    let _ = fs::create_dir_all(analysis_dir.join("jobs"));
    let _ = fs::create_dir_all(&audit_dir);
    let lsp_pool = Arc::new(LspSessionPool::new(project.clone()));
    // P1.3: opt-in LSP pre-warm — spawn the project's language servers in the
    // background so the latency-sensitive default reference path finds them
    // warm (closing e.g. the Python import/type-annotation recall gap without
    // ever paying a cold start on a request). Gated to the same full-runtime
    // constructions that start the watcher; one-shot CLI stays untouched.
    if start_watcher {
        maybe_prewarm_lsp_sessions(&symbol_index, &lsp_pool);
    }
    let mut watcher_error = None;
    let watcher = if start_watcher {
        match FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        ) {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                tracing::warn!(
                    project = %project.as_path().display(),
                    %error,
                    "file watcher failed to start; index will not auto-update on edits"
                );
                watcher_error = Some(error.to_string());
                None
            }
        }
    } else {
        None
    };
    Ok(ProjectContext {
        project,
        symbol_index,
        graph_cache,
        lsp_pool,
        memories_dir,
        analysis_dir,
        audit_dir,
        watcher,
        watcher_error,
    })
}

/// P1.3: decide which LSP servers to pre-warm.
///
/// `mode` is `CODELENS_LSP_PREWARM`:
/// - unset / empty / `off` — disabled (opt-in feature; spawning language
///   servers costs memory and must be a deployment decision).
/// - `auto` — derive from the index's per-extension file counts: the top
///   extensions (≥ `AUTO_MIN_FILES` files) that map to a default LSP server,
///   deduplicated by server command, capped at `AUTO_MAX_SERVERS`.
/// - anything else — comma-separated explicit server commands
///   (e.g. `pyright-langserver,rust-analyzer`), passed through verbatim with
///   their default args. Whitelisting happens at spawn time in the pool.
///
/// Pure — no I/O, no env reads — so the policy is unit-testable.
fn prewarm_commands(mode: &str, language_counts: &[(String, usize)]) -> Vec<String> {
    const AUTO_MIN_FILES: usize = 10;
    const AUTO_MAX_SERVERS: usize = 3;

    let mode = mode.trim();
    if mode.is_empty() || mode.eq_ignore_ascii_case("off") {
        return Vec::new();
    }
    let mut commands: Vec<String> = Vec::new();
    if mode.eq_ignore_ascii_case("auto") {
        for (extension, count) in language_counts {
            if *count < AUTO_MIN_FILES {
                continue;
            }
            if commands.len() >= AUTO_MAX_SERVERS {
                break;
            }
            let probe = format!("probe.{extension}");
            if let Some(command) = crate::tools::default_lsp_command_for_path(&probe)
                && !commands.contains(&command)
            {
                commands.push(command);
            }
        }
    } else {
        for raw in mode.split(',') {
            let command = raw.trim();
            if !command.is_empty() && !commands.iter().any(|existing| existing == command) {
                commands.push(command.to_owned());
            }
        }
    }
    commands
}

/// Read the pre-warm policy from the environment and warm the chosen servers
/// on a background thread. Never blocks the bind path; failures (missing
/// binary, non-whitelisted command) are logged and skipped — pre-warm is an
/// optimization, not a correctness dependency.
fn maybe_prewarm_lsp_sessions(symbol_index: &Arc<SymbolIndex>, lsp_pool: &Arc<LspSessionPool>) {
    let mode = std::env::var("CODELENS_LSP_PREWARM").unwrap_or_default();
    if mode.trim().is_empty() || mode.trim().eq_ignore_ascii_case("off") {
        return;
    }
    let language_counts = symbol_index.language_counts().unwrap_or_default();
    let commands = prewarm_commands(&mode, &language_counts);
    if commands.is_empty() {
        return;
    }
    let pool = Arc::clone(lsp_pool);
    std::thread::spawn(move || {
        for command in commands {
            let args = crate::tools::default_lsp_args_for_command(&command);
            match pool.prewarm_session(&command, &args) {
                Ok(()) => tracing::info!(server = %command, "lsp prewarm: session warm"),
                Err(error) => {
                    tracing::warn!(server = %command, %error, "lsp prewarm: skipped");
                }
            }
        }
    });
}

pub(super) fn activate_project_context(state: &AppState, context: Option<Arc<ProjectContext>>) {
    *state
        .project_override
        .write()
        .unwrap_or_else(|p| p.into_inner()) = context.clone();
    let analysis_dir = context
        .as_ref()
        .map(|override_ctx| override_ctx.analysis_dir.clone())
        .unwrap_or_else(|| state.default_context.analysis_dir.clone());
    state.artifact_store.set_analysis_dir(analysis_dir.clone());
    state.job_store.set_jobs_dir(analysis_dir.join("jobs"));
    state.artifact_store.clear();
    state.job_store.clear();
    state.clear_recent_preflights();
    state.clear_orchestration_approvals();
    #[cfg(feature = "semantic")]
    state.reset_embedding();
    state
        .artifact_store
        .cleanup_stale_dirs(crate::util::now_ms());
    let scope = state.current_project_scope();
    state
        .job_store
        .cleanup_stale_files(crate::util::now_ms(), Some(&scope));
}

#[cfg(test)]
mod request_binding_tests {
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;

    #[test]
    fn nested_request_bindings_restore_on_drop() {
        let default_project = temp_project_root("binding-default");
        let project_a = temp_project_root("binding-a");
        let state = crate::AppState::new_minimal(default_project.clone(), ToolPreset::Balanced);

        let default_scope = default_project.as_path().to_string_lossy().to_string();
        assert_eq!(state.current_project_scope(), default_scope);

        {
            let _outer = state
                .bind_request_project_scope(project_a.as_path().to_str().unwrap())
                .unwrap();
            assert_eq!(
                state.current_project_scope(),
                project_a.as_path().to_string_lossy().to_string()
            );
            {
                // Nested bind back to the default project must pin Default —
                // not fall through to any global override.
                let _inner = state.bind_request_project_scope(&default_scope).unwrap();
                assert_eq!(state.current_project_scope(), default_scope);
            }
            // Inner guard dropped → outer binding restored.
            assert_eq!(
                state.current_project_scope(),
                project_a.as_path().to_string_lossy().to_string()
            );
        }
        // All guards dropped → unbound thread falls back to the daemon default.
        assert_eq!(state.current_project_scope(), default_scope);
    }

    #[test]
    fn request_binding_shields_thread_from_global_override() {
        let default_project = temp_project_root("shield-default");
        let project_a = temp_project_root("shield-a");
        let project_b = temp_project_root("shield-b");
        let state = crate::AppState::new_minimal(default_project, ToolPreset::Balanced);

        let _bound = state
            .bind_request_project_scope(project_a.as_path().to_str().unwrap())
            .unwrap();
        // Another session explicitly switching the global override must not
        // affect a request bound to its own project.
        state
            .switch_project(project_b.as_path().to_str().unwrap())
            .unwrap();
        assert_eq!(
            state.current_project_scope(),
            project_a.as_path().to_string_lossy().to_string()
        );
    }
}

#[cfg(test)]
mod prewarm_tests {
    use super::prewarm_commands;

    fn counts(entries: &[(&str, usize)]) -> Vec<(String, usize)> {
        entries
            .iter()
            .map(|(ext, count)| ((*ext).to_owned(), *count))
            .collect()
    }

    #[test]
    fn off_and_empty_modes_prewarm_nothing() {
        let language_counts = counts(&[("py", 100)]);
        assert!(prewarm_commands("", &language_counts).is_empty());
        assert!(prewarm_commands("off", &language_counts).is_empty());
        assert!(prewarm_commands("  OFF  ", &language_counts).is_empty());
    }

    #[test]
    fn auto_maps_dominant_extensions_to_servers_and_dedupes() {
        // ts+tsx map to the same server — must appear once. `h` has no
        // default LSP mapping and is skipped without consuming a slot.
        let language_counts = counts(&[("ts", 300), ("h", 200), ("tsx", 150), ("py", 90)]);
        let commands = prewarm_commands("auto", &language_counts);
        assert!(
            !commands.is_empty(),
            "dominant mapped languages must yield servers"
        );
        let unique: std::collections::HashSet<_> = commands.iter().collect();
        assert_eq!(unique.len(), commands.len(), "no duplicate servers");
        assert!(commands.len() <= 3, "auto is capped at 3 servers");
    }

    #[test]
    fn auto_ignores_trace_languages_below_min_files() {
        // 3 stray Python files must not spawn a pyright for the whole daemon.
        let language_counts = counts(&[("py", 3)]);
        assert!(prewarm_commands("auto", &language_counts).is_empty());
    }

    #[test]
    fn explicit_list_passes_through_verbatim_deduped() {
        let commands = prewarm_commands(
            "pyright-langserver, rust-analyzer,pyright-langserver, ",
            &[],
        );
        assert_eq!(commands, vec!["pyright-langserver", "rust-analyzer"]);
    }
}

#[cfg(test)]
mod home_guard_tests {
    use super::{ensure_project_root_not_home, home_binding_guard};
    use crate::error::CodeLensError;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-home-guard-{label}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_exact_home_root() {
        let home = temp_dir("home");
        let err = ensure_project_root_not_home(&home, Some(&home), false)
            .expect_err("home root must be rejected");
        match err {
            CodeLensError::HomeRootRejected { root } => {
                assert!(root.contains("codelens-home-guard-home"), "{root}");
            }
            other => panic!("expected HomeRootRejected, got {other:?}"),
        }
    }

    #[test]
    fn allows_subrepo_of_home() {
        let home = temp_dir("home-sub");
        let repo = home.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        assert!(
            ensure_project_root_not_home(&repo, Some(&home), false).is_ok(),
            "a repo inside home must pass"
        );
    }

    #[test]
    fn escape_hatch_flag_allows_home_root() {
        let home = temp_dir("home-flag");
        assert!(
            ensure_project_root_not_home(&home, Some(&home), true).is_ok(),
            "allow_home must bypass the rejection"
        );
    }

    #[test]
    fn missing_home_fails_open() {
        let candidate = temp_dir("home-none");
        assert!(
            ensure_project_root_not_home(&candidate, None, false).is_ok(),
            "an undeterminable home directory must not reject any bind"
        );
    }

    #[test]
    fn env_escape_hatch_toggles_home_binding_guard() {
        let _lock = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let home = temp_dir("home-env");
        let prev_home = std::env::var_os("HOME");
        let prev_allow = std::env::var_os("CODELENS_ALLOW_HOME_PROJECT");
        let prev_allow_symbiote = std::env::var_os("SYMBIOTE_ALLOW_HOME_PROJECT");
        // SAFETY: env mutation is serialized under TEST_ENV_LOCK and restored below.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::remove_var("CODELENS_ALLOW_HOME_PROJECT");
            std::env::remove_var("SYMBIOTE_ALLOW_HOME_PROJECT");
        }
        assert!(
            home_binding_guard(&home).is_err(),
            "home root must be rejected when the escape hatch is unset"
        );
        // SAFETY: see above.
        unsafe {
            std::env::set_var("CODELENS_ALLOW_HOME_PROJECT", "1");
        }
        assert!(
            home_binding_guard(&home).is_ok(),
            "escape hatch env must allow the home root"
        );
        // SAFETY: restore the prior process environment.
        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match prev_allow {
                Some(v) => std::env::set_var("CODELENS_ALLOW_HOME_PROJECT", v),
                None => std::env::remove_var("CODELENS_ALLOW_HOME_PROJECT"),
            }
            match prev_allow_symbiote {
                Some(v) => std::env::set_var("SYMBIOTE_ALLOW_HOME_PROJECT", v),
                None => std::env::remove_var("SYMBIOTE_ALLOW_HOME_PROJECT"),
            }
        }
    }
}
