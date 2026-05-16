use super::AppState;
use anyhow::Context;
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
        symbol_index
            .refresh_all()
            .context("failed to refresh empty symbol index during runtime startup")?;
    }
    let graph_cache = Arc::new(GraphCache::new(30));
    let memories_dir = project.as_path().join(".codelens").join("memories");
    let analysis_dir = project.as_path().join(".codelens").join("analysis-cache");
    let audit_dir = project.as_path().join(".codelens").join("audit");
    ensure_runtime_dir(&memories_dir)?;
    ensure_runtime_dir(&analysis_dir)?;
    ensure_runtime_dir(&analysis_dir.join("jobs"))?;
    ensure_runtime_dir(&audit_dir)?;
    let lsp_pool = Arc::new(LspSessionPool::new(project.clone()));
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
                    error = %error,
                    "file watcher failed to start; runtime continues with manual refresh"
                );
                None
            }
        }
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

fn ensure_runtime_dir(path: &std::path::Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create runtime directory `{}`", path.display()))
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
mod tests {
    use super::*;

    #[test]
    fn build_project_runtime_context_fails_when_runtime_dirs_cannot_be_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(".codelens"), "not a directory").expect("write blocker");
        let project = ProjectRoot::new(dir.path().to_str().expect("utf8 path")).expect("project");

        let err = match build_project_runtime_context(project, false) {
            Ok(_) => panic!("runtime context should not ignore runtime directory setup failure"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("runtime directory"),
            "error should identify runtime directory setup, got: {err:#}"
        );
    }
}
