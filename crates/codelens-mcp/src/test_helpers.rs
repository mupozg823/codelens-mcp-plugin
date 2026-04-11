//! Shared test utilities to eliminate duplicated temp project factories.

#[cfg(test)]
pub(crate) mod fixtures {
    use codelens_core::ProjectRoot;

    /// Create a unique temporary directory with a sample source file,
    /// suitable for constructing a `ProjectRoot` in tests.
    pub fn temp_project_root(label: &str) -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-test-{label}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "fn sample() {}\n").unwrap();
        ProjectRoot::new(&dir).unwrap()
    }

    /// Create a unique temporary directory path (without sample files).
    /// Used exclusively by HTTP transport tests.
    #[cfg(feature = "http")]
    pub fn temp_project_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-test-{label}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
