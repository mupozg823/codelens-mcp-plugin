//! Embedding engine and SCIP backend accessors for `AppState`.
//!
//! Phase P3 also parks the single source of truth for "can the semantic
//! lane actually serve queries right now" here, so every handler that
//! used to ask `engine.is_indexed()` / `embedding_ref().is_some()` on
//! its own goes through `AppState::embedding_status()` instead. Unified
//! semantics means `get_ranked_context.retrieval.semantic_ready` and
//! `review_architecture.data.semantic.loaded` always agree.

#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
#[cfg(feature = "scip-backend")]
use std::sync::Arc;

use super::AppState;

/// Unified embedding readiness snapshot. `ready()` is the only predicate
/// downstream handlers should use to decide whether to advertise the
/// semantic lane.
#[derive(Debug, Clone)]
pub(crate) struct EmbeddingStatus {
    /// Engine is instantiated in the process' memory (not necessarily
    /// populated — an engine with an empty store is `loaded=true` but
    /// not `ready`).
    pub loaded: bool,
    /// Number of symbols currently indexed in the live engine. 0 when
    /// the engine is not loaded or its store is empty.
    pub indexed_symbols: usize,
}

impl EmbeddingStatus {
    /// Whether the semantic lane can contribute real scores on this
    /// call. `loaded && indexed_symbols > 0`.
    pub fn ready(&self) -> bool {
        self.loaded && self.indexed_symbols > 0
    }
}

impl AppState {
    /// Get or initialize embedding engine for the current project.
    /// Fast path (read lock) if already initialized; slow path (write lock) for first init.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_engine(
        &self,
    ) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        // Fast path: already initialized
        {
            let guard = self
                .project_runtime
                .embedding
                .read()
                .unwrap_or_else(|p| p.into_inner());
            if guard.is_some() {
                return guard;
            }
        }
        // Slow path: initialize under write lock
        {
            let mut wguard = self
                .project_runtime
                .embedding
                .write()
                .unwrap_or_else(|p| p.into_inner());
            if wguard.is_none() {
                let project = self.project();
                *wguard = EmbeddingEngine::new(&project)
                    .map_err(|e| tracing::error!("EmbeddingEngine init failed: {e}"))
                    .ok();
            }
        }
        self.project_runtime
            .embedding
            .read()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Read-only access to embedding state without triggering initialization.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_ref(&self) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        self.project_runtime
            .embedding
            .read()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Drop the current embedding engine (called on project switch).
    #[cfg(feature = "semantic")]
    pub(crate) fn reset_embedding(&self) {
        let mut guard = self
            .project_runtime
            .embedding
            .write()
            .unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }

    /// Phase P3: unified embedding readiness snapshot. Does **not**
    /// trigger engine initialization (read-only via `embedding_ref`),
    /// so a disk-only index is correctly reported as
    /// `loaded=false, indexed_symbols=0` until a handler explicitly
    /// warms the engine via `embedding_engine()`.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_status(&self) -> EmbeddingStatus {
        let guard = self.embedding_ref();
        if let Some(engine) = guard.as_ref() {
            let info = engine.index_info();
            EmbeddingStatus {
                loaded: true,
                indexed_symbols: info.indexed_symbols,
            }
        } else {
            EmbeddingStatus {
                loaded: false,
                indexed_symbols: 0,
            }
        }
    }

    #[cfg(not(feature = "semantic"))]
    pub(crate) fn embedding_status(&self) -> EmbeddingStatus {
        EmbeddingStatus {
            loaded: false,
            indexed_symbols: 0,
        }
    }

    /// Lazy-loaded SCIP backend. Loads the SCIP index on first access
    /// and caches it for subsequent calls. Returns None if no index found.
    #[cfg(feature = "scip-backend")]
    pub(crate) fn scip(&self) -> Option<&codelens_engine::ScipBackend> {
        self.project_runtime
            .scip_backend
            .get_or_init(|| {
                let project = self.project();
                codelens_engine::ScipBackend::detect(project.as_path())
                    .and_then(|path| {
                        tracing::info!(path = %path.display(), "loading SCIP index");
                        codelens_engine::ScipBackend::load(&path)
                            .inspect_err(|e| {
                                tracing::warn!(error = %e, "failed to load SCIP index");
                            })
                            .ok()
                    })
                    .map(Arc::new)
            })
            .as_ref()
            .map(|arc| arc.as_ref())
    }
}
