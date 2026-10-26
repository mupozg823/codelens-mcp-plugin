//! Shared test utilities to eliminate duplicated temp project factories.

#[cfg(test)]
pub(crate) mod fixtures {
    use codelens_engine::ProjectRoot;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQ: AtomicU64 = AtomicU64::new(0);
    static PROCESS_ROOT: OnceLock<PathBuf> = OnceLock::new();

    /// Fixture dirs live under `<tmp>/codelens-test-fixtures/<pid>/` instead
    /// of loose `<tmp>/codelens-test-*` entries. Nothing owned the loose dirs,
    /// so every run left them behind (thousands per hour of local test runs).
    /// The first fixture of a process removes the roots of processes that no
    /// longer exist; live parallel test processes keep theirs.
    fn process_root() -> &'static Path {
        PROCESS_ROOT.get_or_init(|| {
            let base = std::env::temp_dir().join("codelens-test-fixtures");
            std::fs::create_dir_all(&base).unwrap();
            remove_dead_process_roots(&base);
            let root = base.join(std::process::id().to_string());
            std::fs::create_dir_all(&root).unwrap();
            root
        })
    }

    #[cfg(unix)]
    fn remove_dead_process_roots(base: &Path) {
        let Ok(entries) = std::fs::read_dir(base) else {
            return;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<libc::pid_t>().ok())
            else {
                continue;
            };
            if pid <= 0 || pid as u32 == std::process::id() {
                continue;
            }
            // SAFETY: signal 0 only checks that the pid exists; nothing is sent.
            let exists = unsafe { libc::kill(pid, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
            if !exists {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    #[cfg(not(unix))]
    fn remove_dead_process_roots(_base: &Path) {}

    /// Create a fresh, empty fixture directory for this test process.
    pub fn fixture_dir(label: &str) -> PathBuf {
        let dir = process_root().join(format!("{label}-{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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
        let dir = fixture_dir(label);
        std::fs::write(dir.join("lib.rs"), "fn sample() {}\n").unwrap();
        ProjectRoot::new(&dir).unwrap()
    }

    /// Create a unique temporary directory path (without sample files).
    /// Used exclusively by HTTP transport tests.
    #[cfg(feature = "http")]
    pub fn temp_project_dir(label: &str) -> PathBuf {
        fixture_dir(label)
    }

    #[cfg(unix)]
    #[test]
    fn dead_process_roots_are_removed_and_live_ones_kept() {
        let base = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        let dead = base.path().join(dead_pid.to_string());
        let current = base.path().join(std::process::id().to_string());
        let parent = base
            .path()
            .join(std::os::unix::process::parent_id().to_string());
        let not_a_pid = base.path().join("not-a-pid");
        for dir in [&dead, &current, &parent, &not_a_pid] {
            std::fs::create_dir_all(dir.join("leaf")).unwrap();
        }

        remove_dead_process_roots(base.path());

        assert!(!dead.exists(), "root of an exited process must be removed");
        assert!(current.exists(), "own root must survive");
        assert!(parent.exists(), "root of a live process must survive");
        assert!(not_a_pid.exists(), "non-pid entries are not ours to remove");
    }
}
