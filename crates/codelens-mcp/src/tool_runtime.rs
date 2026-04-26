use crate::error::CodeLensError;
use crate::protocol::{BackendKind, ToolResponseMeta};
use crate::AppState;

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
