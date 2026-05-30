/// Shared utility helpers used across the crate.
/// Push `value` into `items` only if it is not already present.
/// Works for any `PartialEq` type.
pub(crate) fn push_unique<T: PartialEq>(items: &mut Vec<T>, value: T) {
    if !items.contains(&value) {
        items.push(value);
    }
}

/// Convenience wrapper for `Vec<String>` that accepts `impl Into<String>`.
pub(crate) fn push_unique_string(items: &mut Vec<String>, value: impl Into<String>) {
    push_unique(items, value.into());
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Compute the canonical sha256-hex hash of a JSON value. Stable
/// regardless of object key ordering.
pub fn canonical_sha256_hex(value: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let canonical = canonicalise(value);
    let bytes =
        serde_json::to_vec(&canonical).expect("canonical JSON value is always serialisable");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn canonicalise(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut sorted: Vec<(String, Value)> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalise(v)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalise).collect()),
        other => other.clone(),
    }
}

/// Lexical scope canonicalization (no filesystem access): collapses `.`/`..`
/// components and trailing-slash differences so the same project under a
/// different path representation compares equal. Avoids `fs::canonicalize`
/// (which requires the path to exist and resolves symlinks) so scope matching
/// stays pure and works for deleted/virtual scopes.
fn canonicalize_scope(s: &str) -> String {
    use std::path::{Component, Path, PathBuf};
    let mut out = PathBuf::new();
    for comp in Path::new(s).components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Scope-equality check shared by `job_store` and `artifact_store`. Either side
/// `None` matches (unscoped). When both are present, paths are lexically
/// canonicalized first so trailing-slash / `.`-`..` representation differences
/// of the same project do not produce a silent miss (former G8-class bug, where
/// an explicit `path` arg and `current_project_scope()` resolved the same
/// project to different string forms).
pub(crate) fn matches_scope(scope: Option<&str>, current: Option<&str>) -> bool {
    match (scope, current) {
        (Some(s), Some(c)) => canonicalize_scope(s) == canonicalize_scope(c),
        (None, _) => true,
        (_, None) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_sha256_hex;
    use serde_json::json;

    #[test]
    fn matches_scope_canonicalizes_path_representation() {
        use super::matches_scope;
        // 같은 프로젝트의 다른 경로 표현은 match (canonicalize 일원화)
        assert!(
            matches_scope(Some("/proj"), Some("/proj/")),
            "trailing slash"
        );
        assert!(
            matches_scope(Some("/a/proj"), Some("/a/sub/../proj")),
            "비정규화 경로(.. 포함)"
        );
        // 다른 프로젝트는 여전히 불일치 (검사 의미 보존)
        assert!(!matches_scope(Some("/proj"), Some("/other")));
        // None 의미 보존
        assert!(matches_scope(None, Some("/proj")));
        assert!(matches_scope(Some("/proj"), None));
    }

    #[test]
    fn canonical_sha256_hex_is_key_order_independent() {
        let a = json!({ "alpha": 1, "beta": 2 });
        let b = json!({ "beta": 2, "alpha": 1 });
        assert_eq!(canonical_sha256_hex(&a), canonical_sha256_hex(&b));
    }

    #[test]
    fn canonical_sha256_hex_reflects_value_change() {
        let a = json!({ "alpha": 1 });
        let b = json!({ "alpha": 2 });
        assert_ne!(canonical_sha256_hex(&a), canonical_sha256_hex(&b));
    }

    #[test]
    fn canonical_sha256_hex_handles_nested_objects() {
        let a = json!({ "outer": { "inner_b": 2, "inner_a": 1 } });
        let b = json!({ "outer": { "inner_a": 1, "inner_b": 2 } });
        assert_eq!(canonical_sha256_hex(&a), canonical_sha256_hex(&b));
    }
}
