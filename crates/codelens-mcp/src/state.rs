#[cfg(feature = "semantic")]
use codelens_core::EmbeddingEngine;
use codelens_core::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::sync::{Arc, Mutex};

use crate::telemetry::ToolMetricsRegistry;
use crate::tool_defs::ToolPreset;

// ── Application state ──────────────────────────────────────────────────

pub(crate) struct AppState {
    pub(crate) project: ProjectRoot,
    symbol_index: Arc<SymbolIndex>,
    lsp_pool: LspSessionPool,
    pub(crate) graph_cache: Arc<GraphCache>,
    preset: Mutex<ToolPreset>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    pub(crate) token_budget: std::sync::atomic::AtomicUsize,
    pub(crate) memories_dir: std::path::PathBuf,
    pub(crate) metrics: ToolMetricsRegistry,
    pub(crate) watcher: Option<FileWatcher>,
    #[cfg(feature = "semantic")]
    pub(crate) embedding: std::sync::OnceLock<Option<EmbeddingEngine>>,
    #[cfg(feature = "http")]
    pub(crate) session_store: Option<crate::server::session::SessionStore>,
}

impl AppState {
    /// Access the symbol index. SymbolIndex is internally synchronized
    /// (reader/writer split), so no external lock is needed.
    pub(crate) fn symbol_index(&self) -> &SymbolIndex {
        &self.symbol_index
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
            project,
            symbol_index,
            lsp_pool,
            graph_cache,
            preset: Mutex::new(preset),
            token_budget: std::sync::atomic::AtomicUsize::new(4000),
            memories_dir,
            metrics: ToolMetricsRegistry::new(),
            watcher,
            #[cfg(feature = "semantic")]
            embedding: std::sync::OnceLock::new(),
            #[cfg(feature = "http")]
            session_store: None,
        }
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
