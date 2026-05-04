//! SCIP backend freshness helpers shared by every tool that resolves
//! through `codelens_engine::ScipBackend`.
//!
//! Issue #235 / #236 introduced per-call staleness detection on
//! `find_symbol` so its precise-tier 0.98 confidence wouldn't lie when
//! the on-disk `index.scip` pre-dated the resolved source files. Issue
//! #240 extends the same probe to `find_referencing_symbols` and
//! `get_callers`, which exhibit the identical silent-miss shape.
//!
//! The helper is a best-effort I/O probe: any missing file, unreadable
//! mtime, or absent index returns `None` so the caller never fabricates
//! a stale-evidence claim from missing data.

#[cfg(feature = "scip-backend")]
use serde_json::json;

#[cfg(feature = "scip-backend")]
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct ScipStaleness {
    pub(crate) index_path: String,
    pub(crate) stale_files: Vec<(String, u64)>,
}

#[cfg(feature = "scip-backend")]
pub(crate) fn detect_scip_staleness(
    project_root: &std::path::Path,
    candidate_files: &[String],
) -> Option<ScipStaleness> {
    let index_path = codelens_engine::ScipBackend::detect(project_root)?;
    let index_mtime = std::fs::metadata(&index_path).ok()?.modified().ok()?;
    let mut stale = Vec::new();
    for rel in candidate_files {
        let abs = project_root.join(rel);
        let Ok(meta) = std::fs::metadata(&abs) else {
            continue;
        };
        let Ok(file_mtime) = meta.modified() else {
            continue;
        };
        if file_mtime > index_mtime {
            let age_secs = file_mtime
                .duration_since(index_mtime)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            stale.push((rel.clone(), age_secs));
        }
    }
    (!stale.is_empty()).then(|| ScipStaleness {
        index_path: index_path.display().to_string(),
        stale_files: stale,
    })
}

/// Build the `scip_index_stale_warning` payload that every SCIP-resolved
/// tool surfaces when `detect_scip_staleness` flags one or more files.
/// Centralised here so the message + recommended action stay identical
/// across `find_symbol`, `find_referencing_symbols`, and `get_callers` —
/// callers can branch on a single `code: scip_index_stale` signal.
#[cfg(feature = "scip-backend")]
pub(crate) fn scip_stale_warning_payload(stale: &ScipStaleness) -> serde_json::Value {
    let stale_files_payload: Vec<serde_json::Value> = stale
        .stale_files
        .iter()
        .map(|(file, age)| json!({"file_path": file, "newer_than_index_by_seconds": age}))
        .collect();
    json!({
        "code": "scip_index_stale",
        "message": "SCIP index pre-dates one or more resolved source files; reported line / body / signature may not match current source. Regenerate the index before trusting precise-tier results.",
        "recommended_action": "regenerate_scip_index",
        "action_target": "scip_backend",
        "index_path": stale.index_path,
        "stale_files": stale_files_payload,
    })
}

#[cfg(all(test, feature = "scip-backend"))]
mod tests {
    use super::{ScipStaleness, detect_scip_staleness, scip_stale_warning_payload};
    use std::time::{Duration, SystemTime};

    fn build_fixture(
        index_age_secs: u64,
        source_age_secs: u64,
        sources: &[&str],
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let now = SystemTime::now();
        let index_mtime = now - Duration::from_secs(index_age_secs);
        let source_mtime = now - Duration::from_secs(source_age_secs);
        let index_path = root.join("index.scip");
        std::fs::write(&index_path, b"stub").expect("write index");
        filetime::set_file_mtime(
            &index_path,
            filetime::FileTime::from_system_time(index_mtime),
        )
        .expect("backdate index");
        for rel in sources {
            let abs = root.join(rel);
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent).expect("mkdirs");
            }
            std::fs::write(&abs, b"// placeholder\n").expect("write source");
            filetime::set_file_mtime(&abs, filetime::FileTime::from_system_time(source_mtime))
                .expect("backdate source");
        }
        dir
    }

    #[test]
    fn staleness_detected_when_source_is_newer_than_index() {
        let dir = build_fixture(600, 60, &["src/lib.rs"]);
        let staleness = detect_scip_staleness(dir.path(), &["src/lib.rs".to_owned()])
            .expect("source newer than index → staleness Some");
        assert_eq!(staleness.stale_files.len(), 1);
        assert_eq!(staleness.stale_files[0].0, "src/lib.rs");
        assert!(staleness.stale_files[0].1 >= 300);
        assert!(staleness.index_path.ends_with("index.scip"));
    }

    #[test]
    fn no_staleness_when_index_is_newer_than_source() {
        let dir = build_fixture(60, 600, &["src/lib.rs"]);
        let staleness = detect_scip_staleness(dir.path(), &["src/lib.rs".to_owned()]);
        assert!(staleness.is_none(), "got {staleness:?}");
    }

    #[test]
    fn missing_index_returns_none_silently() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("src.rs"), b"x").expect("source");
        assert!(detect_scip_staleness(dir.path(), &["src.rs".to_owned()]).is_none());
    }

    #[test]
    fn missing_source_file_is_skipped_not_failed() {
        let dir = build_fixture(600, 60, &[]);
        assert!(detect_scip_staleness(dir.path(), &["src/does_not_exist.rs".to_owned()]).is_none());
    }

    #[test]
    fn warning_payload_carries_required_fields() {
        let stale = ScipStaleness {
            index_path: "/tmp/index.scip".to_owned(),
            stale_files: vec![("src/a.rs".to_owned(), 1234)],
        };
        let payload = scip_stale_warning_payload(&stale);
        assert_eq!(payload["code"], "scip_index_stale");
        assert_eq!(payload["recommended_action"], "regenerate_scip_index");
        assert_eq!(payload["action_target"], "scip_backend");
        assert_eq!(payload["index_path"], "/tmp/index.scip");
        assert_eq!(payload["stale_files"][0]["file_path"], "src/a.rs");
        assert_eq!(
            payload["stale_files"][0]["newer_than_index_by_seconds"],
            1234
        );
    }
}
