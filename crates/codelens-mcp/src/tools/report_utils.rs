use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn strings_from_array(
    value: Option<&Vec<Value>>,
    field: &str,
    limit: usize,
) -> Vec<String> {
    value
        .into_iter()
        .flatten()
        .take(limit)
        .filter_map(|entry| {
            if let Some(text) = entry.as_str() {
                Some(text.to_owned())
            } else if let Some(obj) = entry.as_object() {
                obj.get(field)
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(entry.to_string()))
            } else {
                Some(entry.to_string())
            }
        })
        .collect()
}

pub(super) fn stable_cache_key(
    tool_name: &str,
    arguments: &Value,
    keys: &[&str],
) -> Option<String> {
    stable_cache_key_with_extras(tool_name, arguments, keys, &BTreeMap::new())
}

/// Issue #225: variant of [`stable_cache_key`] that accepts caller-supplied
/// `extras` merged into the keyed fields. Tools that work over file
/// content (e.g. `diff_aware_references`) need to invalidate the cache
/// when the underlying file content changes, not only when the
/// `changed_files` argument string changes — `extras` carries the
/// per-file mtime / hash digest that captures that.
pub(super) fn stable_cache_key_with_extras(
    tool_name: &str,
    arguments: &Value,
    keys: &[&str],
    extras: &BTreeMap<String, Value>,
) -> Option<String> {
    let mut fields = BTreeMap::new();
    for key in keys {
        if let Some(value) = arguments.get(*key)
            && !value.is_null()
        {
            fields.insert((*key).to_owned(), value.clone());
        }
    }
    for (key, value) in extras {
        fields.insert(key.clone(), value.clone());
    }
    if fields.is_empty() {
        None
    } else {
        Some(
            json!({
                "tool": tool_name,
                "fields": fields,
            })
            .to_string(),
        )
    }
}

/// Issue #225 helper: collect per-file mtime digests for a list of
/// changed files so the cache key invalidates when the file content
/// changes on disk. Files that cannot be stat'd contribute `0` so a
/// missing-file → present-file transition still flips the digest.
pub(super) fn collect_file_mtime_digests(
    project_root: &std::path::Path,
    changed_files: &[String],
) -> BTreeMap<String, Value> {
    changed_files
        .iter()
        .map(|path| {
            let resolved = project_root.join(path);
            let mtime_secs = std::fs::metadata(&resolved)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|dur| dur.as_secs())
                .unwrap_or(0);
            let len = std::fs::metadata(&resolved)
                .map(|meta| meta.len())
                .unwrap_or(0);
            (
                path.clone(),
                json!({"mtime_secs": mtime_secs, "len_bytes": len}),
            )
        })
        .collect()
}

pub(super) fn extract_handle_fields(payload: &Value) -> (Option<String>, Vec<String>) {
    let analysis_id = payload
        .get("analysis_id")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let estimated_sections = payload
        .get("available_sections")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (analysis_id, estimated_sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #225 regression: same arguments + same changed_files but
    /// different per-file digest must produce different cache keys so
    /// `diff_aware_references` does not reuse a stale analysis after
    /// the file content materially changed on disk.
    #[test]
    fn cache_key_changes_when_file_digest_changes() {
        let arguments = json!({"changed_files": ["a.rs", "b.rs"]});
        let mut extras_v1 = BTreeMap::new();
        extras_v1.insert(
            "file_digests".to_owned(),
            json!({
                "a.rs": {"mtime_secs": 1000, "len_bytes": 256},
                "b.rs": {"mtime_secs": 1001, "len_bytes": 128},
            }),
        );
        let mut extras_v2 = BTreeMap::new();
        // Same files but a.rs got rewritten — mtime + len shifted.
        extras_v2.insert(
            "file_digests".to_owned(),
            json!({
                "a.rs": {"mtime_secs": 2500, "len_bytes": 320},
                "b.rs": {"mtime_secs": 1001, "len_bytes": 128},
            }),
        );

        let key_v1 = stable_cache_key_with_extras(
            "diff_aware_references",
            &arguments,
            &["changed_files"],
            &extras_v1,
        );
        let key_v2 = stable_cache_key_with_extras(
            "diff_aware_references",
            &arguments,
            &["changed_files"],
            &extras_v2,
        );

        assert_ne!(
            key_v1, key_v2,
            "different file digests must produce distinct cache keys"
        );
    }

    /// Backward compat: same arguments + same digests must produce the
    /// same key (cache hit semantics preserved when nothing changed).
    #[test]
    fn cache_key_stable_when_inputs_repeat() {
        let arguments = json!({"changed_files": ["a.rs"]});
        let mut extras = BTreeMap::new();
        extras.insert(
            "file_digests".to_owned(),
            json!({"a.rs": {"mtime_secs": 1000, "len_bytes": 256}}),
        );

        let key_a = stable_cache_key_with_extras(
            "diff_aware_references",
            &arguments,
            &["changed_files"],
            &extras,
        );
        let key_b = stable_cache_key_with_extras(
            "diff_aware_references",
            &arguments,
            &["changed_files"],
            &extras,
        );

        assert_eq!(key_a, key_b);
    }

    /// `stable_cache_key` (the no-extras alias) must keep emitting the
    /// same result as before — only callers that opt into the extras
    /// variant pick up content invalidation.
    #[test]
    fn stable_cache_key_unchanged_for_existing_callers() {
        let arguments = json!({"path": "src/lib.rs", "max_nodes": 50});
        let key = stable_cache_key("mermaid_module_graph", &arguments, &["path", "max_nodes"]);
        assert!(key.is_some());
        let parsed: Value = serde_json::from_str(&key.unwrap()).expect("valid json");
        assert_eq!(parsed["tool"], json!("mermaid_module_graph"));
        assert_eq!(parsed["fields"]["path"], json!("src/lib.rs"));
        assert_eq!(parsed["fields"]["max_nodes"], json!(50));
        // No leak from extras map — backward compat invariant.
        assert!(parsed["fields"].get("file_digests").is_none());
    }

    /// `collect_file_mtime_digests` returns a stable, sorted-by-key
    /// map and falls back to zero values for missing files (so a
    /// file's appearance/disappearance still flips the digest).
    #[test]
    fn file_mtime_digests_handles_missing_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let existing = tempdir.path().join("present.rs");
        std::fs::write(&existing, b"fn main() {}").expect("write file");
        let files = vec!["present.rs".to_owned(), "absent.rs".to_owned()];
        let digests = collect_file_mtime_digests(tempdir.path(), &files);
        assert!(digests.contains_key("present.rs"));
        assert!(digests.contains_key("absent.rs"));
        // Present file has non-zero len_bytes; absent file is zero/zero.
        assert!(digests["present.rs"]["len_bytes"].as_u64().unwrap() > 0);
        assert_eq!(digests["absent.rs"]["mtime_secs"], json!(0));
        assert_eq!(digests["absent.rs"]["len_bytes"], json!(0));
    }
}
