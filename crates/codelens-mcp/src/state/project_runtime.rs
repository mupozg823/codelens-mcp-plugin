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

#[derive(Default)]
pub(super) struct ProjectContextCache {
    entries: HashMap<String, Arc<ProjectRuntimeContext>>,
    access_order: VecDeque<String>,
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
    #[cfg(feature = "semantic")]
    state.reset_embedding();
    #[cfg(feature = "scip-backend")]
    state.reset_scip();
    state.artifact_store.cleanup_stale_dirs(AppState::now_ms());
    let scope = state.current_project_scope();
    state
        .job_store
        .cleanup_stale_files(AppState::now_ms(), Some(&scope));
}

pub(super) fn project(state: &AppState) -> ProjectRoot {
    active_project_context(state)
        .map(|context| context.project.clone())
        .unwrap_or_else(|| state.default_project.clone())
}

pub(super) fn symbol_index(state: &AppState) -> Arc<SymbolIndex> {
    active_project_context(state)
        .map(|context| Arc::clone(&context.symbol_index))
        .unwrap_or_else(|| Arc::clone(&state.default_symbol_index))
}

pub(super) fn graph_cache(state: &AppState) -> Arc<GraphCache> {
    active_project_context(state)
        .map(|context| Arc::clone(&context.graph_cache))
        .unwrap_or_else(|| Arc::clone(&state.default_graph_cache))
}

pub(super) fn memories_dir(state: &AppState) -> PathBuf {
    active_project_context(state)
        .map(|context| context.memories_dir.clone())
        .unwrap_or_else(|| state.default_memories_dir.clone())
}

pub(super) fn analysis_dir(state: &AppState) -> PathBuf {
    active_project_context(state)
        .map(|context| context.analysis_dir.clone())
        .unwrap_or_else(|| state.default_analysis_dir.clone())
}

pub(super) fn audit_dir(state: &AppState) -> PathBuf {
    active_project_context(state)
        .map(|context| context.audit_dir.clone())
        .unwrap_or_else(|| state.default_audit_dir.clone())
}

pub(super) fn watcher_stats(state: &AppState) -> Option<codelens_engine::WatcherStats> {
    active_project_context(state)
        .as_ref()
        .and_then(|context| context.watcher.as_ref().map(FileWatcher::stats))
        .or_else(|| state.default_watcher.as_ref().map(FileWatcher::stats))
}

pub(super) fn watcher_running(state: &AppState) -> bool {
    watcher_stats(state)
        .map(|stats| stats.running)
        .unwrap_or(false)
}

pub(super) fn lsp_pool(state: &AppState) -> Arc<LspSessionPool> {
    active_project_context(state)
        .map(|context| Arc::clone(&context.lsp_pool))
        .unwrap_or_else(|| Arc::clone(&state.default_lsp_pool))
}

pub(super) fn switch_project(state: &AppState, path: &str) -> anyhow::Result<String> {
    let project = ProjectRoot::new(path)?;
    let scope = project.as_path().to_string_lossy().to_string();
    let name = project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    if scope == default_project_scope(state) {
        activate_project_context(state, None);
        return Ok(name);
    }

    if let Some(current) = active_project_context(state)
        && current.project.as_path() == project.as_path()
    {
        return Ok(name);
    }

    let context = {
        let mut cache = state
            .project_context_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = cache.get(&scope) {
            cached
        } else {
            let built = Arc::new(build_project_runtime_context(project, true)?);
            cache.insert(scope.clone(), Arc::clone(&built));
            let active_scope = state.current_project_scope();
            let protected = [default_project_scope(state), active_scope, scope.clone()];
            let protected_refs = protected.iter().map(String::as_str).collect::<Vec<_>>();
            let _evicted =
                cache.evict_until_within_limit(super::PROJECT_CONTEXT_CACHE_LIMIT, &protected_refs);
            built
        }
    };
    activate_project_context(state, Some(context));
    Ok(name)
}

pub(super) fn reset_project(state: &AppState) {
    activate_project_context(state, None);
}

pub(super) fn is_default_project(state: &AppState) -> bool {
    active_project_context(state).is_none()
}

fn default_project_scope(state: &AppState) -> String {
    state
        .default_project
        .as_path()
        .to_string_lossy()
        .to_string()
}
