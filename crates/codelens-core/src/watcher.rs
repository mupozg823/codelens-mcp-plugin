use crate::import_graph::GraphCache;
use crate::symbols::SymbolIndex;
use crate::vfs;
use anyhow::Result;
use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tracing::{debug, warn};

/// File watcher that automatically re-indexes changed files.
pub struct FileWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    running: Arc<AtomicBool>,
    events_processed: Arc<AtomicU64>,
    files_reindexed: Arc<AtomicU64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WatcherStats {
    pub running: bool,
    pub events_processed: u64,
    pub files_reindexed: u64,
    /// Number of files that failed to index (available when symbol index is queried).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_failures: Option<usize>,
}

impl FileWatcher {
    /// Start watching the project root for file changes.
    /// Changed files are automatically re-indexed via `SymbolIndex::index_files`
    /// and the `GraphCache` is invalidated.
    pub fn start(
        root: &Path,
        symbol_index: Arc<SymbolIndex>,
        graph_cache: Arc<GraphCache>,
    ) -> Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let events_processed = Arc::new(AtomicU64::new(0));
        let files_reindexed = Arc::new(AtomicU64::new(0));

        let running_clone = running.clone();
        let events_clone = events_processed.clone();
        let files_clone = files_reindexed.clone();

        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                if !running_clone.load(Ordering::Relaxed) {
                    return;
                }
                let events = match res {
                    Ok(events) => events,
                    Err(e) => {
                        warn!(error = %e, "file watcher error");
                        return;
                    }
                };

                // Classify raw watcher events into changed/removed
                let mut raw_changed: Vec<PathBuf> = Vec::new();
                let mut raw_removed: Vec<PathBuf> = Vec::new();

                for event in &events {
                    let path = &event.path;
                    match event.kind {
                        DebouncedEventKind::Any => {
                            if path.is_file() {
                                raw_changed.push(path.clone());
                            } else {
                                raw_removed.push(path.clone());
                            }
                        }
                        DebouncedEventKind::AnyContinuous => {} // ongoing writes — skip
                        _ => {}
                    }
                }

                events_clone.fetch_add(events.len() as u64, Ordering::Relaxed);

                // Normalize through VFS layer (filters, deduplicates, detects renames)
                let file_events = vfs::normalize_events(&raw_changed, &raw_removed);
                let (changed, removed, renamed) = vfs::partition_events(&file_events);

                debug!(
                    changed = changed.len(),
                    removed = removed.len(),
                    renamed = renamed.len(),
                    total_events = events.len(),
                    "watcher batch processed"
                );

                if changed.is_empty() && removed.is_empty() {
                    return;
                }

                let mut reindexed = 0u64;
                if !changed.is_empty() {
                    match symbol_index.index_files(&changed) {
                        Ok(n) => {
                            reindexed += n as u64;
                            // Clear failures for successfully indexed files
                            let db = symbol_index.db();
                            for file in &changed {
                                let rel = file.to_string_lossy();
                                let _ = db.clear_index_failure(&rel);
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, count = changed.len(), "index_files batch failed");
                            // Record failure for each file in the batch
                            let db = symbol_index.db();
                            for file in &changed {
                                let rel = file.to_string_lossy();
                                let _ = db.record_index_failure(
                                    &rel,
                                    "index_batch_error",
                                    &e.to_string(),
                                );
                            }
                        }
                    }
                }
                if !removed.is_empty() {
                    match symbol_index.remove_files(&removed) {
                        Ok(n) => reindexed += n as u64,
                        Err(e) => warn!(error = %e, "remove_files failed"),
                    }
                }

                if reindexed > 0 {
                    graph_cache.invalidate();
                    files_clone.fetch_add(reindexed, Ordering::Relaxed);
                    debug!(reindexed, "graph cache invalidated");
                }
            },
        )?;

        // Watch the project root recursively
        debouncer
            .watcher()
            .watch(root, notify::RecursiveMode::Recursive)?;

        Ok(Self {
            _debouncer: debouncer,
            running,
            events_processed,
            files_reindexed,
        })
    }

    pub fn stats(&self) -> WatcherStats {
        WatcherStats {
            running: self.running.load(Ordering::Relaxed),
            events_processed: self.events_processed.load(Ordering::Relaxed),
            files_reindexed: self.files_reindexed.load(Ordering::Relaxed),
            index_failures: None,
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}
