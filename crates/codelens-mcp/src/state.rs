#[cfg(feature = "semantic")]
use codelens_core::EmbeddingEngine;
use codelens_core::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::ToolPreset;
use std::collections::VecDeque;

// ── Application state ──────────────────────────────────────────────────

/// Holds project-specific resources that can be swapped at runtime.
struct ProjectOverride {
    project: ProjectRoot,
    symbol_index: Arc<SymbolIndex>,
    graph_cache: Arc<GraphCache>,
    memories_dir: std::path::PathBuf,
}

pub(crate) struct AppState {
    // Default project (set at startup, immutable)
    default_project: ProjectRoot,
    default_symbol_index: Arc<SymbolIndex>,
    default_graph_cache: Arc<GraphCache>,
    default_memories_dir: std::path::PathBuf,
    // Runtime project override (set by activate_project)
    project_override: std::sync::RwLock<Option<ProjectOverride>>,
    lsp_pool: LspSessionPool,
    preset: Mutex<ToolPreset>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    pub(crate) token_budget: std::sync::atomic::AtomicUsize,
    pub(crate) metrics: ToolMetricsRegistry,
    /// Recent tool call names for context-aware suggestions (max 5).
    recent_tools: Mutex<VecDeque<String>>,
    pub(crate) watcher: Option<FileWatcher>,
    #[cfg(feature = "semantic")]
    pub(crate) embedding: std::sync::OnceLock<Option<EmbeddingEngine>>,
    /// Secondary (read-only) project indexes for cross-project queries.
    pub(crate) secondary_projects: Mutex<HashMap<String, SecondaryProject>>,
    #[cfg(feature = "http")]
    pub(crate) session_store: Option<crate::server::session::SessionStore>,
}

/// A read-only project registered for cross-project queries.
pub(crate) struct SecondaryProject {
    pub project: ProjectRoot,
    pub index: Arc<SymbolIndex>,
}

impl AppState {
    // ── Active project accessors (check override, fallback to default) ──

    /// Get the active project root. Clones the ProjectRoot (just a PathBuf).
    pub(crate) fn project(&self) -> ProjectRoot {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => o.project.clone(),
            None => self.default_project.clone(),
        }
    }

    /// Get the active symbol index.
    pub(crate) fn symbol_index(&self) -> Arc<SymbolIndex> {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => Arc::clone(&o.symbol_index),
            None => Arc::clone(&self.default_symbol_index),
        }
    }

    /// Get the active graph cache.
    pub(crate) fn graph_cache(&self) -> Arc<GraphCache> {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => Arc::clone(&o.graph_cache),
            None => Arc::clone(&self.default_graph_cache),
        }
    }

    /// Get the active memories directory.
    pub(crate) fn memories_dir(&self) -> std::path::PathBuf {
        let guard = self
            .project_override
            .read()
            .unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(o) => o.memories_dir.clone(),
            None => self.default_memories_dir.clone(),
        }
    }

    /// Switch the active project at runtime. Creates a new index and graph cache.
    pub(crate) fn switch_project(&self, path: &str) -> anyhow::Result<String> {
        let project = ProjectRoot::new(path)?;
        let name = project
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
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
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(ProjectOverride {
            project,
            symbol_index,
            graph_cache,
            memories_dir,
        });
        Ok(name)
    }

    /// Reset to the default project.
    #[allow(dead_code)]
    pub(crate) fn reset_project(&self) {
        *self
            .project_override
            .write()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Check if running on the default project.
    #[allow(dead_code)]
    pub(crate) fn is_default_project(&self) -> bool {
        self.project_override
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .is_none()
    }

    /// Access the LSP session pool. Pool uses internal per-session locking.
    pub(crate) fn lsp_pool(&self) -> &LspSessionPool {
        &self.lsp_pool
    }

    /// Acquire preset lock with poison recovery.
    pub(crate) fn preset(&self) -> std::sync::MutexGuard<'_, ToolPreset> {
        self.preset
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Access the tool metrics registry.
    pub(crate) fn metrics(&self) -> &ToolMetricsRegistry {
        &self.metrics
    }

    /// Record a tool call in the recent tools ring buffer.
    pub(crate) fn push_recent_tool(&self, name: &str) {
        let mut q = self.recent_tools.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() >= 5 {
            q.pop_front();
        }
        q.push_back(name.to_owned());
    }

    /// Get the recent tool call names (up to 5).
    pub(crate) fn recent_tools(&self) -> Vec<String> {
        self.recent_tools
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// Current global token budget.
    pub(crate) fn token_budget(&self) -> usize {
        self.token_budget.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Set global token budget.
    pub(crate) fn set_token_budget(&self, budget: usize) {
        self.token_budget
            .store(budget, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let symbol_index = Arc::new(SymbolIndex::new(project.clone()));
        // Auto-index on startup if DB is empty — ensures zero-config first use.
        if symbol_index
            .stats()
            .map(|s| s.indexed_files == 0)
            .unwrap_or(true)
        {
            let _ = symbol_index.refresh_all();
        }
        let lsp_pool = LspSessionPool::new(project.clone());
        let graph_cache = Arc::new(GraphCache::new(30));
        let memories_dir = project.as_path().join(".codelens").join("memories");

        let watcher = FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        )
        .ok();

        Self {
            default_project: project,
            default_symbol_index: symbol_index,
            lsp_pool,
            default_graph_cache: graph_cache,
            default_memories_dir: memories_dir,
            project_override: std::sync::RwLock::new(None),
            preset: Mutex::new(preset),
            token_budget: std::sync::atomic::AtomicUsize::new(4000),
            metrics: ToolMetricsRegistry::new(),
            recent_tools: Mutex::new(VecDeque::with_capacity(5)),
            watcher,
            secondary_projects: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
        }
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
        // Ensure it's indexed
        index.refresh_all()?;
        let mut map = self
            .secondary_projects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.insert(name.clone(), SecondaryProject { project, index });
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
    ) -> anyhow::Result<Vec<codelens_core::SymbolInfo>> {
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

    /// Initialize the session store for HTTP mode.
    #[cfg(feature = "http")]
    pub(crate) fn with_session_store(mut self) -> Self {
        self.session_store = Some(crate::server::session::SessionStore::new(
            std::time::Duration::from_secs(30 * 60), // 30 minutes
        ));
        self
    }
}
