use super::AppState;
use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// Holds project-specific resources that can be reused across rebinds.
pub(super) struct ProjectRuntimeContext {
    pub(super) project: ProjectRoot,
    pub(super) symbol_index: Arc<SymbolIndex>,
    pub(super) graph_cache: Arc<GraphCache>,
    pub(super) lsp_pool: Arc<LspSessionPool>,
    pub(super) memories_dir: PathBuf,
    pub(super) analysis_dir: PathBuf,
    pub(super) audit_dir: PathBuf,
    /// Keeps the watcher alive so it continues to receive file-system events.
    pub(super) watcher: Option<FileWatcher>,
}

impl ProjectRuntimeContext {
    pub(super) fn shutdown_resources(&self) {
        if let Some(ref watcher) = self.watcher {
            watcher.stop();
        }
        self.lsp_pool.shutdown();
    }
}

#[derive(Default)]
pub(super) struct ProjectContextCache {
    pub(super) entries: HashMap<String, Arc<ProjectRuntimeContext>>,
    pub(super) access_order: VecDeque<String>,
}

impl ProjectContextCache {
    pub(super) fn get(&mut self, scope: &str) -> Option<Arc<ProjectRuntimeContext>> {
        let context = self.entries.get(scope).cloned()?;
        self.touch(scope);
        Some(context)
    }

    pub(super) fn insert(&mut self, scope: String, context: Arc<ProjectRuntimeContext>) {
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
    ) -> Vec<Arc<ProjectRuntimeContext>> {
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
}

pub(super) fn active_project_context(state: &AppState) -> Option<Arc<ProjectRuntimeContext>> {
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
) -> anyhow::Result<ProjectRuntimeContext> {
    let symbol_index = Arc::new(SymbolIndex::new(project.clone()));
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
    let watcher = if start_watcher {
        FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        )
        .ok()
    } else {
        None
    };
    Ok(ProjectRuntimeContext {
        project,
        symbol_index,
        graph_cache,
        lsp_pool,
        memories_dir,
        analysis_dir,
        audit_dir,
        watcher,
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

pub(super) fn activate_project_context(
    state: &AppState,
    context: Option<Arc<ProjectRuntimeContext>>,
) {
    *state
        .project_override
        .write()
        .unwrap_or_else(|p| p.into_inner()) = context.clone();
    let analysis_dir = context
        .as_ref()
        .map(|override_ctx| override_ctx.analysis_dir.clone())
        .unwrap_or_else(|| state.default_analysis_dir.clone());
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
