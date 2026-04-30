//! Build-time metadata emission for `codelens-mcp`.
//!
//! Phase 4b (§capability-reporting follow-up): Phase 4a identified a
//! class of debugging confusion where the running daemon's memory
//! image drifted from the source + disk binary. A daemon started
//! from a pre-Phase-4a release still reported the pre-fix behaviour
//! even though the source files and the release binary on disk were
//! already up-to-date. The user had no single-call way to detect
//! "running daemon is stale".
//!
//! This build script embeds three facts at compile time so
//! `get_capabilities` can expose them at runtime:
//!
//! 1. **CODELENS_BUILD_GIT_SHA** — short git SHA (or `unknown` if
//!    `git` is unavailable or the source tree is not a git checkout).
//! 2. **CODELENS_BUILD_TIME** — ISO-8601 build timestamp in UTC.
//! 3. **CODELENS_BUILD_VERSION** — the `CARGO_PKG_VERSION` cargo
//!    already provides, re-exposed through the same build-info
//!    module for consistency.
//!
//! All three are injected via `cargo:rustc-env=KEY=VALUE` and read at
//! runtime via `env!()` (infallible — compile-time guaranteed to
//! exist).
//!
//! The script re-runs when the git HEAD changes, so a local rebuild
//! after `git commit` picks up the new SHA. Build time advances on
//! every clean build; incremental builds may reuse the previous
//! build time (acceptable — the intent is to catch "daemon running
//! from commit X, source is at commit Y", not "daemon build is
//! exactly N seconds old").

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // Re-run whenever HEAD or the refs change, so `git commit`
    // triggers a rebuild that picks up the new SHA.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");

    // Copy workspace-level files into OUT_DIR so include_str! works
    // both in workspace builds (paths resolve upward) and in
    // published-crate verification (paths do not exist — fallback).
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));

    let workspace_cargo_src = manifest_dir.join("../../Cargo.toml");
    let workspace_cargo_dst = out_dir.join("workspace-cargo-toml");
    copy_or_fallback(
        &workspace_cargo_src,
        &workspace_cargo_dst,
        "[workspace.package]\nversion = \"unknown\"\n",
    );
    println!("cargo:rerun-if-changed={}", workspace_cargo_src.display());

    let schema_src = manifest_dir.join("../../docs/schemas/handoff-artifact.v1.json");
    let schema_dst = out_dir.join("handoff-artifact.v1.json");
    copy_or_fallback(
        &schema_src,
        &schema_dst,
        r#"{"schema_version":"codelens-handoff-artifact-v1","fallback":true}"#,
    );
    println!("cargo:rerun-if-changed={}", schema_src.display());

    // 1. Git SHA (short, 7-char)
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=CODELENS_BUILD_GIT_SHA={git_sha}");

    // 2. Build timestamp (RFC3339 UTC). We format this manually to
    // avoid pulling in `chrono` as a build-script dependency — the
    // format is a trivial `%Y-%m-%dT%H:%M:%SZ`.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let build_time = format_iso8601_utc(now);
    println!("cargo:rustc-env=CODELENS_BUILD_TIME={build_time}");

    // 3. Dirty flag — if the working tree has uncommitted changes
    // relative to HEAD, append `-dirty` to the SHA in the env var
    // output. This helps distinguish a build from an exact commit vs
    // a build from a commit-plus-uncommitted-edits.
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false);
    println!("cargo:rustc-env=CODELENS_BUILD_GIT_DIRTY={}", dirty);
}

fn copy_or_fallback(src: &PathBuf, dst: &PathBuf, fallback: &str) {
    if src.exists() {
        fs::copy(src, dst)
            .unwrap_or_else(|err| panic!("copy {} -> {}: {err}", src.display(), dst.display()));
    } else {
        fs::write(dst, fallback)
            .unwrap_or_else(|err| panic!("write fallback {}: {err}", dst.display()));
    }
}

/// Format a UNIX timestamp (seconds since epoch) as
/// `YYYY-MM-DDTHH:MM:SSZ` (RFC 3339, UTC). Pure integer arithmetic,
/// no dependencies. Good from 1970-01-01 to 9999-12-31.
fn format_iso8601_utc(unix_seconds: u64) -> String {
    // Days since epoch
    let days = (unix_seconds / 86_400) as i64;
    let secs_in_day = unix_seconds % 86_400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;

    // Convert days-since-1970 to (year, month, day). Algorithm from
    // Howard Hinnant's "date algorithms" — days-since-civil-epoch.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // year of era
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month zero-indexed from March
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + (if m <= 2 { 1 } else { 0 });

    format!(
        "{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z",
        year = year,
        m = m,
        d = d,
        hour = hour,
        minute = minute,
        second = second,
    )
}
