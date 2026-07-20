//! Shared test utilities to eliminate duplicated temp project factories.

#[cfg(test)]
pub(crate) mod fixtures {
    use codelens_engine::ProjectRoot;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Unique dir-name suffix that stays collision-free across parallel
    /// TEST PROCESSES (nextest runs one process per test): the main test
    /// thread gets the same `ThreadId` in every process and the macOS
    /// realtime clock only ticks in microseconds, so `(nanos, thread_id)`
    /// alone collided on loaded CI runners — two processes then raced the
    /// same `.codelens/index/symbols.db` and died with "database is
    /// locked" in whatever test happened to lose (rotating CI flake).
    fn unique_suffix() -> String {
        format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed),
        )
    }

    /// Create a unique temporary directory with a sample source file,
    /// suitable for constructing a `ProjectRoot` in tests.
    pub fn temp_project_root(label: &str) -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!("codelens-test-{label}-{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "fn sample() {}\n").unwrap();
        ProjectRoot::new(&dir).unwrap()
    }

    /// Create a unique temporary directory path (without sample files).
    /// Used exclusively by HTTP transport tests.
    #[cfg(feature = "http")]
    pub fn temp_project_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("codelens-test-{label}-{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
