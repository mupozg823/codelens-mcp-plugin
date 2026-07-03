use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::{BackendKind, ToolResponseMeta};

/// Tool handler result type — every handler returns this.
pub type ToolResult = Result<(serde_json::Value, ToolResponseMeta), CodeLensError>;

pub fn success_meta(backend: BackendKind, confidence: f64) -> ToolResponseMeta {
    ToolResponseMeta {
        backend_used: backend.to_string(),
        confidence,
        degraded_reason: None,
        source: crate::protocol::AnalysisSource::Native,
        partial: false,
        freshness: crate::protocol::Freshness::Live,
        staleness_ms: None,
    }
}

/// Like `success_meta` but sets `degraded_reason` to flag that the result
/// is from a heuristic / non-semantic backend (e.g. tree-sitter line-range
/// arithmetic). Confidence should be lowered from the ideal semantic value.
pub fn degraded_meta(backend: BackendKind, confidence: f64, reason: &str) -> ToolResponseMeta {
    ToolResponseMeta {
        backend_used: backend.to_string(),
        confidence,
        degraded_reason: Some(reason.to_owned()),
        source: crate::protocol::AnalysisSource::Native,
        partial: false,
        freshness: crate::protocol::Freshness::Live,
        staleness_ms: None,
    }
}

pub fn required_string<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, CodeLensError> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam(key.to_owned()))
}

pub type ToolHandler = fn(&AppState, &serde_json::Value) -> ToolResult;

// ── Common argument extractors ────────────────────────────────────────

pub const PATH_ALIAS_DEPRECATION: &str =
    "DEPRECATED v1.13.23 — use `path`. Soft alias maintained until v1.14.0.";

pub fn path_alias_warning(alias: &str) -> serde_json::Value {
    serde_json::json!({
        "param": alias,
        "replacement": "path",
        "message": PATH_ALIAS_DEPRECATION,
    })
}

/// Extract an optional string argument.
pub fn optional_string<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Extract an optional u64 argument with a default value.
#[allow(dead_code)]
pub fn optional_u64(value: &serde_json::Value, key: &str, default: u64) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

/// Extract an optional usize argument with a default value.
pub fn optional_usize(value: &serde_json::Value, key: &str, default: usize) -> usize {
    value
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default)
}

/// Extract an optional bool argument with a default value.
pub fn optional_bool(value: &serde_json::Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Extract an optional usize argument, accepting any of `aliases` as
/// equivalent to `canonical`. Aliases are checked in order; the first
/// match wins. Existing tools that pass a single canonical name are
/// unaffected (pass an empty `aliases` slice). Used to absorb common
/// agent typos like `limit`/`top_k` for `max_results` without
/// silently dropping the value.
///
/// Returns `default` only when neither the canonical key nor any alias
/// resolves to a u64 in the JSON envelope.
pub fn optional_usize_with_aliases(
    value: &serde_json::Value,
    canonical: &str,
    aliases: &[&str],
    default: usize,
) -> usize {
    if let Some(v) = value.get(canonical).and_then(|v| v.as_u64()) {
        return v as usize;
    }
    for alias in aliases {
        if let Some(v) = value.get(*alias).and_then(|v| v.as_u64()) {
            return v as usize;
        }
    }
    default
}

/// Normalise a raw `files.indexed_at` value to epoch **seconds** and classify
/// staleness relative to `now_secs`.
///
/// Correctness note: `files.indexed_at` is written in epoch *milliseconds*
/// (`db::ops` upsert path), but `index_freshness_hint` historically compared it
/// against `now.as_secs()`. That unit mismatch made `age_secs` a large negative
/// number that clamped to `0`, so every response reported `staleness_hint:
/// "fresh"` / `refresh_recommended: false` — the documented freshness signal
/// could never fire. We normalise defensively here: values above ~1e12 are
/// milliseconds and are divided down; smaller values are already seconds
/// (legacy/mixed indexes). Returns `(epoch_secs, age_secs, hint, refresh)`.
fn classify_index_freshness(now_secs: i64, raw_indexed_at: i64) -> (i64, i64, &'static str, bool) {
    let max_at = if raw_indexed_at > 1_000_000_000_000 {
        raw_indexed_at / 1000
    } else {
        raw_indexed_at
    };
    let age_secs = (now_secs - max_at).max(0);
    let (hint, refresh_recommended) = if age_secs < 60 {
        ("fresh", false)
    } else if age_secs < 600 {
        ("recent", false)
    } else if age_secs < 3600 {
        ("possibly_stale", false)
    } else {
        ("stale", true)
    };
    (max_at, age_secs, hint, refresh_recommended)
}

/// Freshness hint for tool responses that read from the on-disk symbol
/// index. Compares the index's newest `indexed_at` against wall-clock
/// time so callers can detect a stale daemon without having to diff
/// results against the working tree.
///
/// Buckets:
///   - `fresh`           — newest file indexed < 60s ago
///   - `recent`          — 60s..600s
///   - `possibly_stale`  — 600s..3600s
///   - `stale`           — >= 3600s (sets `refresh_recommended: true`)
///
/// Returns `None` when the index is empty (callers omit the hint to
/// avoid noise on a fresh project).
pub fn index_freshness_hint(state: &AppState) -> Option<serde_json::Value> {
    use serde_json::json;
    let raw = state.symbol_index().max_indexed_at().ok().flatten()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let (max_at, age_secs, hint, refresh_recommended) = classify_index_freshness(now, raw);
    Some(json!({
        "newest_indexed_at_epoch_secs": max_at,
        "newest_indexed_age_secs": age_secs,
        "staleness_hint": hint,
        "refresh_recommended": refresh_recommended,
    }))
}

/// Return the names of every top-level key in `value` that does not
/// appear in `known` — both the canonical names AND any registered
/// aliases. Tools surface this list in their response so an agent that
/// passes (e.g.) `{"query":"x","threshold":0.5}` to a tool that does
/// not honor `threshold` sees the field was ignored, instead of
/// receiving silent default behavior.
///
/// Keys whose name begins with `_` are treated as harness-internal
/// metadata (e.g. the `_session_id` that `call_tool` auto-injects in
/// integration tests, the `_meta` envelope MCP clients sometimes
/// attach) and are NEVER reported as unknown — they are not user
/// input. Without this exception every `unknown_args` array on every
/// tool would noisily include `_session_id`.
///
/// Returns an empty Vec for non-object values so non-object inputs
/// fall through to the handler's existing argument parsing without
/// spurious "unknown" claims.
pub fn collect_unknown_args(value: &serde_json::Value, known: &[&str]) -> Vec<String> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut unknown: Vec<String> = map
        .keys()
        .filter(|k| !k.starts_with('_'))
        .filter(|k| !known.iter().any(|allowed| allowed == k))
        .cloned()
        .collect();
    unknown.sort();
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn freshness_millisecond_timestamp_is_normalised_not_always_fresh() {
        // Regression: `files.indexed_at` is written in epoch ms, but the hint
        // compared it against `now.as_secs()`. A 2h-old index (ms timestamp)
        // used to clamp to age 0 / "fresh". After the unit fix it must report
        // "stale" and recommend a refresh.
        let now_secs = 2_000_000_000; // arbitrary fixed "now" in seconds
        let two_hours_ago_ms = (now_secs - 7_200) * 1000;
        let (epoch_secs, age_secs, hint, refresh) =
            classify_index_freshness(now_secs, two_hours_ago_ms);
        assert_eq!(
            epoch_secs,
            now_secs - 7_200,
            "ms input must normalise to secs"
        );
        assert_eq!(age_secs, 7_200);
        assert_eq!(hint, "stale");
        assert!(refresh, "a >1h-old index must recommend refresh");
    }

    #[test]
    fn freshness_recent_millisecond_timestamp_reports_fresh() {
        let now_secs = 2_000_000_000;
        let ten_secs_ago_ms = (now_secs - 10) * 1000;
        let (_epoch, age_secs, hint, refresh) = classify_index_freshness(now_secs, ten_secs_ago_ms);
        assert_eq!(age_secs, 10);
        assert_eq!(hint, "fresh");
        assert!(!refresh);
    }

    #[test]
    fn freshness_legacy_second_timestamp_still_classified() {
        // Defensive: a value already in seconds (< 1e12) must not be divided.
        let now_secs = 2_000_000_000;
        let legacy_secs = now_secs - 700; // 700s ago, already seconds
        let (epoch_secs, age_secs, hint, _refresh) =
            classify_index_freshness(now_secs, legacy_secs);
        assert_eq!(
            epoch_secs, legacy_secs,
            "second-scale input must pass through"
        );
        assert_eq!(age_secs, 700);
        assert_eq!(hint, "possibly_stale");
    }

    #[test]
    fn optional_usize_with_aliases_prefers_canonical() {
        // Canonical name takes precedence even when an alias is also
        // present — protects callers from accidental contradictory
        // values silently flipping based on iteration order.
        let args = json!({"max_results": 10, "limit": 99});
        let n = optional_usize_with_aliases(&args, "max_results", &["limit", "top_k"], 20);
        assert_eq!(n, 10);
    }

    #[test]
    fn optional_usize_with_aliases_accepts_first_alias() {
        // The motivating P0 case: agent sends `limit: 5` to a tool
        // whose canonical name is `max_results`. Pre-fix, this
        // silently fell back to default 20. Post-fix, the alias is
        // honored.
        let args = json!({"limit": 5});
        let n = optional_usize_with_aliases(&args, "max_results", &["limit", "top_k"], 20);
        assert_eq!(n, 5);
    }

    #[test]
    fn optional_usize_with_aliases_falls_back_when_absent() {
        let args = json!({"query": "x"});
        let n = optional_usize_with_aliases(&args, "max_results", &["limit", "top_k"], 20);
        assert_eq!(n, 20);
    }

    #[test]
    fn collect_unknown_args_returns_keys_outside_allowlist() {
        let args = json!({"query": "x", "limit": 5, "banana": 1, "carrot": "c"});
        let known = ["query", "limit"];
        let unknown = collect_unknown_args(&args, &known);
        assert_eq!(unknown, vec!["banana".to_owned(), "carrot".to_owned()]);
    }

    #[test]
    fn collect_unknown_args_empty_for_clean_input() {
        let args = json!({"query": "x", "limit": 5});
        let unknown = collect_unknown_args(&args, &["query", "limit"]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn collect_unknown_args_skips_underscore_prefixed_harness_keys() {
        // Harness-internal keys like `_session_id` (auto-injected by
        // the integration-test `call_tool` helper) and `_meta` (some
        // MCP clients attach this to every tool call) are not user
        // input. They must never surface as "unknown" or every tool
        // response would carry noisy `unknown_args: ["_session_id"]`.
        let args = json!({
            "query": "x",
            "_session_id": "abc",
            "_meta": {"trace": "..."},
            "real_unknown": 1,
        });
        let unknown = collect_unknown_args(&args, &["query"]);
        assert_eq!(unknown, vec!["real_unknown".to_owned()]);
    }

    #[test]
    fn collect_unknown_args_empty_for_non_object() {
        // Defensive: a tool that receives a positional/string-only
        // argument shouldn't crash trying to compute "unknown keys".
        // Return empty so the handler's existing parsing handles it.
        assert!(collect_unknown_args(&json!("plain"), &["query"]).is_empty());
        assert!(collect_unknown_args(&json!(null), &["query"]).is_empty());
    }
}
