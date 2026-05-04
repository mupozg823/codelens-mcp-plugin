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

/// Run `git -C <root> rev-parse --short=7 HEAD` to read the project's
/// current short SHA. Returns `None` when git is unavailable, the
/// project is not a git checkout, or the override env var is set to
/// an empty string. Tests can short-circuit the subprocess call by
/// setting `CODELENS_HEAD_GIT_SHA_OVERRIDE`.
fn current_head_git_sha(project_root: &std::path::Path) -> Option<String> {
    if let Some(override_value) = std::env::var_os("CODELENS_HEAD_GIT_SHA_OVERRIDE") {
        let value = override_value.into_string().ok()?;
        return if value.is_empty() { None } else { Some(value) };
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("--short=7")
        .arg("HEAD")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let trimmed = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Combine mtime-staleness and HEAD git_sha mismatch into a single
/// drift verdict. Two independent signals trigger `stale = true`:
///
/// - `mtime_stale`: on-disk executable mtime is newer than daemon
///   start time (Phase 4a — daemon outlived its own binary)
/// - HEAD git_sha mismatch: daemon's compile-time `BUILD_GIT_SHA`
///   differs from the project's current HEAD short SHA, so a fix
///   merged after the binary was built is silently absent
///
/// `mtime_stale` takes precedence in `reason_code` so existing
/// consumers keep their semantics. The `"unknown"` sentinel emitted
/// by `build.rs` for non-git builds is treated as "no signal".
/// Issue #221: even when both sides come from `git rev-parse --short`
/// they can disagree in width (git widens to 8+ chars on prefix
/// collisions), so the strict `==` comparison previously used here
/// raised a false-positive `head_git_sha_mismatch` for the same
/// commit. Compare by common prefix instead — two SHAs match when
/// one is a prefix of the other (subject to a 4-char minimum so we
/// don't treat trivially-short strings as a wildcard).
fn shas_share_prefix(a: &str, b: &str) -> bool {
    const MIN_PREFIX_LEN: usize = 4;
    if a.len() < MIN_PREFIX_LEN || b.len() < MIN_PREFIX_LEN {
        return false;
    }
    a.starts_with(b) || b.starts_with(a)
}

fn classify_drift(
    mtime_stale: bool,
    head_git_sha: Option<&str>,
    binary_git_sha: &str,
) -> (bool, Option<&'static str>, Option<&'static str>) {
    let head_mismatch = matches!(
        head_git_sha,
        Some(head)
            if !head.is_empty()
                && head != "unknown"
                && binary_git_sha != "unknown"
                && !shas_share_prefix(head, binary_git_sha)
    );
    let stale = mtime_stale || head_mismatch;
    let reason_code = match (mtime_stale, head_mismatch) {
        (true, _) => Some("stale_daemon_binary"),
        (false, true) => Some("head_git_sha_mismatch"),
        _ => None,
    };
    let reason = match (mtime_stale, head_mismatch) {
        (true, _) => Some(
            "disk executable is newer than the running daemon; restart the MCP server to pick up the latest build",
        ),
        (false, true) => Some(
            "daemon binary git_sha does not match project HEAD; rebuild and restart to pick up newer commits",
        ),
        _ => None,
    };
    (stale, reason_code, reason)
}

/// Runtime check for two independent daemon-staleness failure modes:
///
/// 1. Phase 4a: the on-disk executable mtime is newer than the daemon
///    start time (binary was rebuilt while the long-running daemon
///    kept serving the older in-memory copy).
/// 2. HEAD git_sha mismatch: the project's current HEAD differs from
///    `BUILD_GIT_SHA`, so a fix merged after the binary was compiled
///    is silently absent. Detected when `project_root` is supplied
///    and `git rev-parse --short=7 HEAD` succeeds.
///
/// Either signal sets `stale_daemon = true`; the mtime signal takes
/// precedence in `reason_code` so existing consumers keep semantics.
pub(crate) fn daemon_binary_drift_payload(
    daemon_started_at: &str,
    project_root: Option<&std::path::Path>,
) -> serde_json::Value {
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
    let mtime_stale = modified_seconds > daemon_started_seconds;
    let head_git_sha = project_root.and_then(current_head_git_sha);
    let (stale_daemon, reason_code, reason) =
        classify_drift(mtime_stale, head_git_sha.as_deref(), BUILD_GIT_SHA);
    let status = if stale_daemon { "stale" } else { "ok" };
    let recommended_action = if stale_daemon {
        Some("restart_mcp_server")
    } else {
        None
    };
    json!({
        "status": status,
        "stale_daemon": stale_daemon,
        "restart_recommended": stale_daemon,
        "reason_code": reason_code,
        "recommended_action": recommended_action,
        "action_target": if stale_daemon { Some("daemon") } else { None },
        "executable_path": executable_path.to_string_lossy(),
        "executable_modified_at": format_rfc3339_utc(modified_seconds),
        "daemon_started_at": daemon_started_at,
        "binary_build_time": BUILD_TIME,
        "binary_git_sha": BUILD_GIT_SHA,
        "head_git_sha": head_git_sha,
        "reason": reason,
    })
}

#[cfg(test)]
mod tests {
    use super::{classify_drift, format_rfc3339_utc, parse_rfc3339_utc_seconds};

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

    #[test]
    fn classify_drift_reports_head_mismatch_when_binary_sha_lags_head() {
        let (stale, code, reason) = classify_drift(false, Some("237e4465"), "f7885f9b");
        assert!(stale, "head mismatch must trigger stale=true");
        assert_eq!(code, Some("head_git_sha_mismatch"));
        assert!(reason.unwrap().contains("does not match project HEAD"));
    }

    #[test]
    fn classify_drift_clears_when_head_matches_binary() {
        let (stale, code, reason) = classify_drift(false, Some("237e4465"), "237e4465");
        assert!(!stale);
        assert_eq!(code, None);
        assert_eq!(reason, None);
    }

    #[test]
    fn classify_drift_prefers_mtime_stale_signal_over_head_mismatch() {
        let (stale, code, reason) = classify_drift(true, Some("237e4465"), "f7885f9b");
        assert!(stale);
        assert_eq!(
            code,
            Some("stale_daemon_binary"),
            "mtime stale must take precedence in reason_code"
        );
        assert!(reason.unwrap().contains("disk executable is newer"));
    }

    #[test]
    fn classify_drift_treats_unknown_sentinel_as_no_signal() {
        let (stale, code, _) = classify_drift(false, Some("unknown"), "f7885f9b");
        assert!(!stale, "unknown HEAD must not trigger mismatch");
        assert_eq!(code, None);

        let (stale, code, _) = classify_drift(false, Some("237e4465"), "unknown");
        assert!(!stale, "unknown binary SHA must not trigger mismatch");
        assert_eq!(code, None);
    }

    #[test]
    fn classify_drift_skips_when_head_unavailable() {
        let (stale, code, _) = classify_drift(false, None, "f7885f9b");
        assert!(!stale);
        assert_eq!(code, None);
    }

    #[test]
    fn classify_drift_skips_when_head_string_is_empty() {
        let (stale, code, _) = classify_drift(false, Some(""), "f7885f9b");
        assert!(!stale);
        assert_eq!(code, None);
    }

    /// Issue #221 regression: same commit but with two different git
    /// `--short` widths (build script captured 8 chars; runtime
    /// `current_head_git_sha` returns 7) must not trigger a
    /// false-positive `head_git_sha_mismatch`. Common-prefix match
    /// is the new contract.
    #[test]
    fn classify_drift_treats_unequal_length_shas_with_common_prefix_as_match() {
        // 7 chars (HEAD via --short=7) vs 8 chars (build via --short
        // widened by git collision-avoidance). Same commit.
        let (stale, code, reason) = classify_drift(false, Some("f28620a"), "f28620a5");
        assert!(
            !stale,
            "common-prefix shas must not trigger head_git_sha_mismatch"
        );
        assert_eq!(code, None);
        assert_eq!(reason, None);

        // Symmetric: 8 chars (HEAD) vs 7 chars (binary).
        let (stale, code, _) = classify_drift(false, Some("f28620a5"), "f28620a");
        assert!(!stale);
        assert_eq!(code, None);
    }

    /// Real mismatch (different commit) must still be detected — the
    /// prefix relaxation must not silence genuine drift.
    #[test]
    fn classify_drift_still_detects_real_mismatch_with_different_prefix() {
        // Different commits, different first chars.
        let (stale, code, _) = classify_drift(false, Some("f28620a"), "abcd1234");
        assert!(stale, "different prefixes must still trigger mismatch");
        assert_eq!(code, Some("head_git_sha_mismatch"));

        // Same first 3 chars but diverging by char 4 — below the 4-char
        // minimum we still treat as mismatch (the rule requires at least
        // 4 matching chars to be considered a prefix relationship).
        let (stale, code, _) = classify_drift(false, Some("f28000a"), "f28620a5");
        assert!(stale);
        assert_eq!(code, Some("head_git_sha_mismatch"));
    }
}
