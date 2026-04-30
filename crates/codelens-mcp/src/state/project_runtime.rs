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
        .map_or(true, |s| s.indexed_files == 0)
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
        .as_ref().map_or_else(|| state.default_analysis_dir.clone(), |override_ctx| override_ctx.analysis_dir.clone());
    state.artifact_store.set_analysis_dir(analysis_dir.clone());
    state.job_store.set_jobs_dir(analysis_dir.join("jobs"));
    state.artifact_store.clear();
    state.job_store.clear();
    state.clear_recent_preflights();
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
