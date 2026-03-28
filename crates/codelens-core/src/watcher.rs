use crate::import_graph::GraphCache;
use crate::project::is_excluded;
use crate::symbols::{language_for_path, SymbolIndex};
use anyhow::Result;
use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

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
}

impl FileWatcher {
    /// Start watching the project root for file changes.
    /// Changed files are automatically re-indexed via `SymbolIndex::index_files`
    /// and the `GraphCache` is invalidated.
    pub fn start(
        root: &Path,
        symbol_index: Arc<Mutex<SymbolIndex>>,
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
                    Err(_) => return,
                };

                let mut changed: Vec<PathBuf> = Vec::new();
                let mut removed: Vec<PathBuf> = Vec::new();

                for event in &events {
                    let path = &event.path;
                    if is_excluded(path) {
                        continue;
                    }
                    // Only watch files with supported language extensions
                    if language_for_path(path).is_none() {
                        continue;
                    }
                    match event.kind {
                        DebouncedEventKind::Any => {
                            if path.is_file() {
                                changed.push(path.clone());
                            } else {
                                // File no longer exists → treat as removal
                                removed.push(path.clone());
                            }
                        }
                        DebouncedEventKind::AnyContinuous => {
                            // Ongoing writes — skip until stabilized
                        }
                        _ => {}
                    }
                }

                events_clone.fetch_add(events.len() as u64, Ordering::Relaxed);

                if changed.is_empty() && removed.is_empty() {
                    return;
                }

                let mut reindexed = 0u64;
                if let Ok(mut index) = symbol_index.lock() {
                    if !changed.is_empty() {
                        if let Ok(n) = index.index_files(&changed) {
                            reindexed += n as u64;
                        }
                    }
                    if !removed.is_empty() {
                        if let Ok(n) = index.remove_files(&removed) {
                            reindexed += n as u64;
                        }
                    }
                }

                if reindexed > 0 {
                    graph_cache.invalidate();
                    files_clone.fetch_add(reindexed, Ordering::Relaxed);
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
