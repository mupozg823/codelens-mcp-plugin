//! Embedding engine and SCIP backend accessors for `AppState`.
//!
//! Pure move from `state.rs` — no logic changes.

#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
#[cfg(feature = "scip-backend")]
use std::sync::Arc;

use super::AppState;

impl AppState {
    /// `true` when the cached engine was built for a project other than
    /// the CURRENT request's project (#357: with request-scoped bindings
    /// the engine is no longer dropped on switch, so accessors must
    /// detect the mismatch and rebuild instead of serving wrong-project
    /// embeddings).
    #[cfg(feature = "semantic")]
    fn embedding_root_mismatch(&self) -> bool {
        let current_root = self.project().as_path().to_path_buf();
        let root_guard = self
            .embedding_root
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        matches!(&*root_guard, Some(root) if *root != current_root)
    }

    /// Get or initialize embedding engine for the current project.
    /// Fast path (read lock) if already initialized; slow path (write lock) for first init.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_engine(
        &self,
    ) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        if self.embedding_root_mismatch() {
            self.reset_embedding();
        }
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
                let mut root_guard = self
                    .embedding_root
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                *root_guard = wguard.is_some().then(|| project.as_path().to_path_buf());
            }
        }
        self.embedding.read().unwrap_or_else(|p| p.into_inner())
    }

    /// Read-only access to embedding state without triggering initialization.
    /// A root mismatch reads as "not initialized" for the current project.
    #[cfg(feature = "semantic")]
    pub(crate) fn embedding_ref(&self) -> std::sync::RwLockReadGuard<'_, Option<EmbeddingEngine>> {
        if self.embedding_root_mismatch() {
            self.reset_embedding();
        }
        self.embedding.read().unwrap_or_else(|p| p.into_inner())
    }

    /// Drop the current embedding engine (called on project switch).
    #[cfg(feature = "semantic")]
    pub(crate) fn reset_embedding(&self) {
        let mut guard = self.embedding.write().unwrap_or_else(|p| p.into_inner());
        *guard = None;
        let mut root_guard = self
            .embedding_root
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *root_guard = None;
    }

    /// Lazy-loaded SCIP backend for the current project. A shared daemon can
    /// serve multiple project-bound HTTP sessions, so the backend cache is keyed
    /// by project root instead of process-global first access.
    #[cfg(feature = "scip-backend")]
    pub(crate) fn scip(&self) -> Option<Arc<codelens_engine::ScipBackend>> {
        let project = self.project();
        let project_root = project.as_path().to_path_buf();
        {
            let cache = self.scip_backends.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(backend) = cache.get(&project_root) {
                return Some(Arc::clone(backend));
            }
        }

        let index_path = codelens_engine::ScipBackend::detect(project.as_path())?;
        tracing::info!(
            project_root = %project_root.display(),
            path = %index_path.display(),
            "loading SCIP index"
        );
        let backend = Arc::new(
            codelens_engine::ScipBackend::load(&index_path)
                .inspect_err(|e| {
                    tracing::warn!(
                        project_root = %project_root.display(),
                        path = %index_path.display(),
                        error = %e,
                        "failed to load SCIP index"
                    );
                })
                .ok()?,
        );

        let mut cache = self.scip_backends.lock().unwrap_or_else(|p| p.into_inner());
        let entry = cache.entry(project_root).or_insert(backend);
        Some(Arc::clone(entry))
    }

    #[cfg(feature = "scip-backend")]
    pub(crate) fn drop_scip_backend_for_project(&self, project_root: &std::path::Path) {
        let mut cache = self.scip_backends.lock().unwrap_or_else(|p| p.into_inner());
        cache.remove(project_root);
    }
}
