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

use serde_json::json;

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

fn civil_from_days(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn days_from_civil(year: i64, month: u64, day: u64) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_index = month as i64 + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_index + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn format_rfc3339_utc(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let secs_in_day = unix_seconds % 86_400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn parse_fixed_u64(value: &[u8]) -> Option<u64> {
    std::str::from_utf8(value).ok()?.parse::<u64>().ok()
}

fn parse_fixed_i64(value: &[u8]) -> Option<i64> {
    std::str::from_utf8(value).ok()?.parse::<i64>().ok()
}

fn parse_rfc3339_utc_seconds(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.get(19) != Some(&b'Z')
    {
        return None;
    }
    let year = parse_fixed_i64(&bytes[0..4])?;
    let month = parse_fixed_u64(&bytes[5..7])?;
    let day = parse_fixed_u64(&bytes[8..10])?;
    let hour = parse_fixed_u64(&bytes[11..13])?;
    let minute = parse_fixed_u64(&bytes[14..16])?;
    let second = parse_fixed_u64(&bytes[17..19])?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + hour * 3600 + minute * 60 + second)
}

fn current_executable_path() -> Result<std::path::PathBuf, String> {
    if let Some(path) = std::env::var_os("CODELENS_EXECUTABLE_PATH_OVERRIDE") {
        return Ok(std::path::PathBuf::from(path));
    }
    std::env::current_exe().map_err(|err| format!("current_exe unavailable: {err}"))
}

/// Runtime check for the Phase 4a failure mode: the daemon is still
/// serving requests from an older in-memory binary while the
/// executable on disk has been replaced with a newer build.
///
/// `stale_daemon = true` means the executable path currently visible on
/// disk has an mtime newer than `daemon_started_at`, so restarting the
/// daemon is recommended before trusting version-sensitive behavior.
pub(crate) fn daemon_binary_drift_payload(daemon_started_at: &str) -> serde_json::Value {
    let daemon_started_seconds = match parse_rfc3339_utc_seconds(daemon_started_at) {
        Some(value) => value,
        None => {
            return json!({
                "status": "unknown",
                "stale_daemon": false,
                "restart_recommended": false,
                "reason": "unable to parse daemon_started_at"
            });
        }
    };
    let executable_path = match current_executable_path() {
        Ok(path) => path,
        Err(reason) => {
            return json!({
                "status": "unknown",
                "stale_daemon": false,
                "restart_recommended": false,
                "reason": reason,
            });
        }
    };
    let modified = match std::fs::metadata(&executable_path)
        .and_then(|metadata| metadata.modified())
        .map_err(|err| format!("unable to inspect executable metadata: {err}"))
    {
        Ok(value) => value,
        Err(reason) => {
            return json!({
                "status": "unknown",
                "stale_daemon": false,
                "restart_recommended": false,
                "executable_path": executable_path.to_string_lossy(),
                "reason": reason,
            });
        }
    };
    let modified_seconds = match modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
    {
        Ok(value) => value,
        Err(_) => {
            return json!({
                "status": "unknown",
                "stale_daemon": false,
                "restart_recommended": false,
                "executable_path": executable_path.to_string_lossy(),
                "reason": "executable mtime predates unix epoch"
            });
        }
    };
    let stale_daemon = modified_seconds > daemon_started_seconds;
    let status = if stale_daemon { "stale" } else { "ok" };
    json!({
        "status": status,
        "stale_daemon": stale_daemon,
        "restart_recommended": stale_daemon,
        "executable_path": executable_path.to_string_lossy(),
        "executable_modified_at": format_rfc3339_utc(modified_seconds),
        "daemon_started_at": daemon_started_at,
        "binary_build_time": BUILD_TIME,
        "binary_git_sha": BUILD_GIT_SHA,
        "reason": if stale_daemon {
            Some("disk executable is newer than the running daemon; restart the MCP server to pick up the latest build")
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{format_rfc3339_utc, parse_rfc3339_utc_seconds};

    #[test]
    fn rfc3339_utc_round_trips_known_epoch_values() {
        let samples = [
            (0, "1970-01-01T00:00:00Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (1_712_793_600, "2024-04-11T00:00:00Z"),
        ];
        for (unix_seconds, expected) in samples {
            assert_eq!(format_rfc3339_utc(unix_seconds), expected);
            assert_eq!(parse_rfc3339_utc_seconds(expected), Some(unix_seconds));
        }
    }
}
