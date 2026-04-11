//! Compile-time build metadata exposed to the `get_capabilities`
//! tool layer.
//!
//! Phase 4b (§capability-reporting follow-up): the previous
//! Phase 4a debugging session hit a footgun where a long-running
//! HTTP daemon's memory image drifted from the source + disk
//! binary. A user had no single-call way to detect "running daemon
//! ≠ current source". This module reads compile-time environment
//! variables injected by `build.rs` and exposes them alongside the
//! daemon's wall-clock start time, so one `get_capabilities`
//! request answers:
//!
//! - What version of CodeLens was this binary built from?
//! - Which git commit produced it?
//! - When was it built?
//! - When did the running daemon start?
//!
//! Downstream tooling (CLI dashboards, agent harnesses) can then
//! compare `binary_build_time` against `daemon_started_at` and
//! detect "the daemon has been running since before the binary
//! was rebuilt" — the exact Phase 4a failure mode.

/// The `CARGO_PKG_VERSION` at build time (e.g. `"1.5.0"`). This is
/// injected by cargo itself, not by our build script; we re-expose
/// it here to keep all build-info fields in one place.
pub(crate) const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git SHA (7 chars) at build time, or `"unknown"` if the
/// build happened outside a git checkout or `git` was unavailable.
/// Emitted by `build.rs` via `cargo:rustc-env=CODELENS_BUILD_GIT_SHA`.
pub(crate) const BUILD_GIT_SHA: &str = env!("CODELENS_BUILD_GIT_SHA");

/// RFC 3339 UTC timestamp when the binary was built
/// (`YYYY-MM-DDTHH:MM:SSZ`). Emitted by `build.rs`. Useful for
/// detecting "daemon started before the binary was rebuilt"
/// (compare against `daemon_started_at`).
pub(crate) const BUILD_TIME: &str = env!("CODELENS_BUILD_TIME");

/// `"true"` / `"false"` string — whether the working tree had
/// uncommitted changes relative to HEAD when this binary was
/// compiled. Useful for distinguishing a clean-commit release from
/// a build-with-local-edits development binary.
pub(crate) const BUILD_GIT_DIRTY: &str = env!("CODELENS_BUILD_GIT_DIRTY");

/// Parsed form of `BUILD_GIT_DIRTY` for boolean consumers.
pub(crate) fn build_git_dirty() -> bool {
    matches!(BUILD_GIT_DIRTY, "true" | "1" | "yes")
}
