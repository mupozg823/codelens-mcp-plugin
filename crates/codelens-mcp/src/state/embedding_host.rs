//! Embedding engine and SCIP backend accessors for `AppState`.
//!
//! Originally a pure move from `state.rs`; it now also owns the idle-eviction
//! policy that keeps a long-lived daemon from parking the ONNX model forever.

#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
#[cfg(feature = "scip-backend")]
use std::sync::Arc;

use super::AppState;

/// Idle window before a resident embedding engine is dropped. Long enough that an
/// active session never pays the reload, short enough that a daemon left open
/// overnight does not park the model's footprint.
///
/// The eviction cluster's only non-test caller is the `http` transport's
/// cleanup loop, so a `semantic`-without-`http` build sees it as dead code;
/// the unit tests below still exercise it in that combination.
#[cfg(feature = "semantic")]
#[cfg_attr(not(feature = "http"), allow(dead_code))]
const DEFAULT_EMBED_IDLE_TTL_SECS: u64 = 900;

/// Idle policy, split from the clock and the lock so it is directly testable.
/// All three conditions must hold: the sweep is enabled, the engine has actually
/// been used (an untouched daemon has nothing to drop), and it is still resident.
#[cfg(feature = "semantic")]
#[cfg_attr(not(feature = "http"), allow(dead_code))]
fn embedding_is_idle(
    idle_for: Option<std::time::Duration>,
    ttl: Option<std::time::Duration>,
    engine_resident: bool,
) -> bool {
    match (idle_for, ttl) {
        (Some(idle_for), Some(ttl)) => engine_resident && idle_for >= ttl,
        _ => false,
    }
}

/// Clear `slot` when the engine in it has gone idle, returning `true` when it
/// dropped one. Never waits for the lock: a held guard means a request is using
/// the engine (an index build holds it for minutes), so it is not idle, and the
/// caller is the HTTP daemon's async cleanup loop, where a blocking wait parks a
/// runtime worker.
#[cfg(feature = "semantic")]
#[cfg_attr(not(feature = "http"), allow(dead_code))]
fn drop_if_idle<T>(
    slot: &std::sync::RwLock<Option<T>>,
    idle_for: Option<std::time::Duration>,
    ttl: Option<std::time::Duration>,
) -> bool {
    let mut guard = match slot.try_write() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return false,
    };
    if !embedding_is_idle(idle_for, ttl, guard.is_some()) {
        return false;
    }
    *guard = None;
    true
}

/// `None` disables the idle sweep (`CODELENS_EMBED_IDLE_TTL_SECS=0`).
#[cfg(feature = "semantic")]
#[cfg_attr(not(feature = "http"), allow(dead_code))]
fn configured_embedding_idle_ttl() -> Option<std::time::Duration> {
    let secs = std::env::var("CODELENS_EMBED_IDLE_TTL_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_EMBED_IDLE_TTL_SECS);
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

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
        self.touch_embedding();
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

    /// Record that the engine was just handed out for real work. Deliberately not
    /// called from `embedding_ref`, which also serves status readers like
    /// `get_capabilities` — letting a status poll refresh the clock would keep the
    /// model resident forever.
    #[cfg(feature = "semantic")]
    fn touch_embedding(&self) {
        *self
            .embedding_last_used
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(std::time::Instant::now());
    }

    /// Drop the engine once it has gone unused for the configured TTL, returning
    /// `true` when it actually dropped one. Called from the daemon's periodic
    /// cleanup task: a long-lived daemon that served a single semantic query would
    /// otherwise hold the ONNX model for its whole lifetime. The next
    /// `embedding_engine` call transparently reloads it.
    ///
    /// `CODELENS_EMBED_IDLE_TTL_SECS=0` disables the sweep.
    #[cfg(feature = "semantic")]
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    pub(crate) fn drop_idle_embedding(&self) -> bool {
        let idle_for = self
            .embedding_last_used
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map(|last_used| last_used.elapsed());
        if !drop_if_idle(&self.embedding, idle_for, configured_embedding_idle_ttl()) {
            return false;
        }
        *self
            .embedding_root
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        *self
            .embedding_last_used
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        true
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

#[cfg(all(test, feature = "semantic"))]
mod tests {
    use super::{drop_if_idle, embedding_is_idle};
    use std::sync::{Arc, RwLock, mpsc};
    use std::time::Duration;

    const TTL: Option<Duration> = Some(Duration::from_secs(900));

    #[test]
    fn idle_sweep_fires_only_past_the_ttl_on_a_resident_engine() {
        assert!(embedding_is_idle(
            Some(Duration::from_secs(1000)),
            TTL,
            true
        ));
        assert!(embedding_is_idle(Some(Duration::from_secs(900)), TTL, true));
    }

    #[test]
    fn idle_sweep_holds_a_recently_used_engine() {
        assert!(!embedding_is_idle(
            Some(Duration::from_secs(899)),
            TTL,
            true
        ));
        assert!(!embedding_is_idle(Some(Duration::ZERO), TTL, true));
    }

    #[test]
    fn idle_sweep_is_a_no_op_without_ttl_use_or_a_resident_engine() {
        // `CODELENS_EMBED_IDLE_TTL_SECS=0`
        assert!(!embedding_is_idle(
            Some(Duration::from_secs(1000)),
            None,
            true
        ));
        // never handed out — nothing to drop
        assert!(!embedding_is_idle(None, TTL, true));
        // already dropped (e.g. by a project switch)
        assert!(!embedding_is_idle(
            Some(Duration::from_secs(1000)),
            TTL,
            false
        ));
    }

    #[test]
    fn idle_sweep_never_waits_for_an_engine_in_use() {
        // The sweep runs on the HTTP daemon's async cleanup loop. An index build
        // holds the engine for minutes without refreshing its last-use time, so
        // the engine looks idle; waiting for the write lock then parked an async
        // worker and the whole daemon stopped answering until the build ended.
        let slot = Arc::new(RwLock::new(Some(1u8)));
        let in_use = slot.read().unwrap();
        let (tx, rx) = mpsc::channel();
        let sweep_slot = Arc::clone(&slot);
        std::thread::spawn(move || {
            let _ = tx.send(drop_if_idle(
                &sweep_slot,
                Some(Duration::from_secs(1000)),
                TTL,
            ));
        });
        let dropped = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the idle sweep blocked behind an engine in use");
        assert!(!dropped);
        drop(in_use);
        assert!(slot.read().unwrap().is_some());
    }

    #[test]
    fn idle_sweep_drops_an_idle_engine_nobody_holds() {
        let slot = RwLock::new(Some(1u8));
        assert!(drop_if_idle(&slot, Some(Duration::from_secs(1000)), TTL));
        assert!(slot.read().unwrap().is_none());
        assert!(!drop_if_idle(&slot, Some(Duration::from_secs(1000)), TTL));
    }
}
