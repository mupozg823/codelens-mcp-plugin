//! Shared test fixture helpers for `codelens-engine` unit tests.
//!
//! Centralises the tempfile pattern after the v1.13.31 macOS path-collision
//! flake (`scope_analysis::tests::detects_import_reference`, commit 5075bc8b)
//! and the v1.13.29 #332 search-module flake (commit 45b76f38). Every
//! fixture that used to call
//! `std::env::temp_dir().join(format!("...{nanos}", ...))` was racing on
//! macOS where `SystemTime::now()` quantises to ~1 µs (vs 1 ns on Linux),
//! so two tests sharing a fixture would land on the same temp directory and
//! overwrite each other's input files mid-extraction. `tempfile::TempDir`
//! is process-unique by construction.

#![cfg(test)]

use std::path::PathBuf;

/// Create a fresh tempfile-backed directory with a stable prefix.
///
/// Returns `(TempDir, PathBuf)`:
/// - `TempDir` is the drop guard — keep it bound (often as `_temp_dir`)
///   so the directory survives until the fixture goes out of scope.
/// - `PathBuf` is a convenience clone of `td.path()` for callers that
///   want to `fs::write(dir.join("..."), ...)` without re-borrowing the
///   guard each time.
///
/// Replaces the legacy `std::env::temp_dir().join(format!("...{nanos}", ...))`
/// pattern. See module docs for the race condition this avoids.
pub(crate) fn make_unique_temp_dir(prefix: &str) -> (tempfile::TempDir, PathBuf) {
    let td = tempfile::TempDir::with_prefix(prefix).expect("tempfile creation");
    let path = td.path().to_path_buf();
    (td, path)
}
