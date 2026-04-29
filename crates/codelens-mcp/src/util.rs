/// Shared utility helpers used across the crate.
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

#[cfg(test)]
mod tests {
    use super::canonical_sha256_hex;
    use serde_json::json;

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
