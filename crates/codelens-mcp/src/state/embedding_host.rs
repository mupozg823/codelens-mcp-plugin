//! Embedding engine and SCIP backend accessors for `AppState`.
//!
//! Pure move from `state.rs` — no logic changes.

#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
#[cfg(feature = "scip-backend")]
use std::sync::Arc;

use super::AppState;

impl AppState {
    /// Get or initialize embedding engine for the current project.
    /// Fast path (read lock) if already initialized; slow path (write lock) for first init.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_engine(
        &self,
    ) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        // Fast path: already initialized
        {
            let guard = self.embedding.read().unwrap_or_else(|p| p.into_inner());
            if guard.is_some() {
                return guard;
            }
        }
        // Slow path: initialize under write lock
        {
            let mut wguard = self.embedding.write().unwrap_or_else(|p| p.into_inner());
            if wguard.is_none() {
                let project = self.project();
                *wguard = EmbeddingEngine::new(&project)
                    .map_err(|e| tracing::error!("EmbeddingEngine init failed: {e}"))
                    .ok();
            }
        }
        self.embedding.read().unwrap_or_else(|p| p.into_inner())
    }

    /// Read-only access to embedding state without triggering initialization.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_ref(&self) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        self.embedding.read().unwrap_or_else(|p| p.into_inner())
    }

    /// Drop the current embedding engine (called on project switch).
    #[cfg(feature = "semantic")]
    pub(crate) fn reset_embedding(&self) {
        let mut guard = self.embedding.write().unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }

    /// Lazy-loaded SCIP backend. Loads the SCIP index on first access
    /// and caches it for subsequent calls. Returns None if no index found.
    #[cfg(feature = "scip-backend")]
    pub(crate) fn scip(&self) -> Option<&codelens_engine::ScipBackend> {
        self.scip_backend
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
