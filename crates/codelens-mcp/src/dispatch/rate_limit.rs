//! Per-session rate limit + doom-loop argument hashing.

use crate::error::CodeLensError;
use crate::AppState;

/// Routing metadata keys that must be excluded from the doom-loop hash
/// so that identical semantic tool calls with different routing profiles
/// are still detected as consecutive repeats.
const DOOM_LOOP_SKIP_KEYS: &[&str] = &["_profile", "_compact"];

/// Per-session rate limit. Returns an error if the session has exceeded
/// the call budget within the sliding window. Default: 300 calls/minute.
/// Override via `CODELENS_RATE_LIMIT` env var.
pub(crate) fn check_rate_limit(
    state: &AppState,
    session: &crate::session_context::SessionRequestContext,
) -> Option<CodeLensError> {
    let limit: u64 = std::env::var("CODELENS_RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);

    let session_calls = state.metrics().session_call_count(&session.session_id);
    if session_calls > limit {
        Some(CodeLensError::Validation(format!(
            "Rate limit exceeded: {} calls in this session (limit: {}). \
             Override with CODELENS_RATE_LIMIT env var.",
            session_calls, limit
        )))
    } else {
        None
    }
}

/// Zero-allocation recursive hash for a JSON argument value.
///
/// Walks the value tree using one discriminator byte per node and hashes
/// primitive payloads directly, avoiding the `v.to_string()` per-field
/// allocation pattern that was used by the inline hasher previously.
/// Numbers still go through their canonical string form (because
/// `serde_json::Number` does not implement `Hash` for f64 soundness
/// reasons), but a typical tool call's numeric payloads are very small
/// — five to ten bytes at most — so the remaining allocations are
/// bounded and noise-sized.
///
/// The hash is intentionally stable within a process and across
/// semantically equivalent argument shapes, but not stable across
/// processes or serde_json versions. That matches the existing
/// doom-loop contract: the hash is only used as a key in an in-memory
/// session map for "same tool+args called consecutively" detection.
///
/// Determinism on object iteration relies on
/// `serde_json`'s `preserve_order` feature, which is enabled in the
/// workspace `Cargo.toml`.
fn hash_json_value<H: std::hash::Hasher>(value: &serde_json::Value, hasher: &mut H) {
    use std::hash::Hash;
    match value {
        serde_json::Value::Null => 0u8.hash(hasher),
        serde_json::Value::Bool(b) => {
            1u8.hash(hasher);
            b.hash(hasher);
        }
        serde_json::Value::Number(n) => {
            2u8.hash(hasher);
            // `Number` does not implement `Hash` directly. Its canonical
            // string form is what serde_json uses for Display, so we use
            // the same form here to stay consistent with the previous
            // `v.to_string()` behaviour for numeric primitives.
            n.to_string().hash(hasher);
        }
        serde_json::Value::String(s) => {
            3u8.hash(hasher);
            s.hash(hasher);
        }
        serde_json::Value::Array(arr) => {
            4u8.hash(hasher);
            arr.len().hash(hasher);
            for item in arr {
                hash_json_value(item, hasher);
            }
        }
        serde_json::Value::Object(obj) => {
            5u8.hash(hasher);
            obj.len().hash(hasher);
            for (k, v) in obj {
                k.hash(hasher);
                hash_json_value(v, hasher);
            }
        }
    }
}

/// Hash the top-level argument object for doom-loop repeat detection.
///
/// At the top level, keys listed in [`DOOM_LOOP_SKIP_KEYS`] are ignored
/// so that a routing-metadata change alone (e.g. switching profiles)
/// does not mask an otherwise-identical consecutive call. Nested objects
/// are hashed in full via [`hash_json_value`] — only the top-level
/// skip-list applies.
pub(crate) fn hash_args_for_doom_loop(arguments: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Some(obj) = arguments.as_object() {
        5u8.hash(&mut hasher);
        // We intentionally do NOT hash obj.len() here: after the skip
        // filter the live key count might differ from the raw length,
        // and the iteration below already stops the hash from colliding
        // across disjoint key sets because each present key is hashed.
        for (k, v) in obj {
            if DOOM_LOOP_SKIP_KEYS.contains(&k.as_str()) {
                continue;
            }
            k.hash(&mut hasher);
            hash_json_value(v, &mut hasher);
        }
    } else {
        hash_json_value(arguments, &mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::hash_args_for_doom_loop;
    use serde_json::json;

    #[test]
    fn identical_args_produce_identical_hash() {
        let a = json!({"file_path": "src/lib.rs", "include_body": true, "max_matches": 5});
        let b = json!({"file_path": "src/lib.rs", "include_body": true, "max_matches": 5});
        assert_eq!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn different_string_values_produce_different_hash() {
        let a = json!({"file_path": "src/lib.rs"});
        let b = json!({"file_path": "src/main.rs"});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn different_numeric_values_produce_different_hash() {
        let a = json!({"max_matches": 5});
        let b = json!({"max_matches": 10});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn different_bool_values_produce_different_hash() {
        let a = json!({"include_body": true});
        let b = json!({"include_body": false});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn top_level_profile_is_excluded_from_hash() {
        let a = json!({"file_path": "src/lib.rs", "_profile": "planner-readonly"});
        let b = json!({"file_path": "src/lib.rs", "_profile": "builder-minimal"});
        assert_eq!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn top_level_compact_is_excluded_from_hash() {
        let a = json!({"file_path": "src/lib.rs", "_compact": true});
        let b = json!({"file_path": "src/lib.rs", "_compact": false});
        assert_eq!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn top_level_skip_keys_coexist_with_content_keys_correctly() {
        let bare = json!({"file_path": "src/lib.rs"});
        let with_profile = json!({"file_path": "src/lib.rs", "_profile": "builder-minimal"});
        assert_eq!(
            hash_args_for_doom_loop(&bare),
            hash_args_for_doom_loop(&with_profile)
        );
    }

    #[test]
    fn nested_objects_contribute_to_hash() {
        let a = json!({"options": {"recursive": true}});
        let b = json!({"options": {"recursive": false}});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn arrays_contribute_to_hash() {
        let a = json!({"paths": ["a.rs", "b.rs"]});
        let b = json!({"paths": ["a.rs", "c.rs"]});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn array_order_contributes_to_hash() {
        let a = json!({"paths": ["a.rs", "b.rs"]});
        let b = json!({"paths": ["b.rs", "a.rs"]});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn non_object_arguments_are_still_hashed() {
        let a = json!("bare_string_arg");
        let b = json!("different_string_arg");
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn nested_profile_key_is_not_skipped() {
        // Only the *top-level* _profile key is routing metadata.
        // A nested key happening to be named `_profile` is real payload
        // and must contribute to the hash.
        let a = json!({"meta": {"_profile": "a"}});
        let b = json!({"meta": {"_profile": "b"}});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn integer_vs_float_zero_differ() {
        // serde_json::Number preserves the written form; 0 and 0.0 have
        // different canonical strings, so they must produce different
        // hashes. This matches the previous `v.to_string()` behaviour.
        let a = json!({"n": 0});
        let b = json!({"n": 0.0});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }

    #[test]
    fn null_and_missing_keys_differ() {
        let a = json!({"field": null});
        let b = json!({});
        assert_ne!(hash_args_for_doom_loop(&a), hash_args_for_doom_loop(&b));
    }
}
