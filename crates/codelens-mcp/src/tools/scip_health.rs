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
const SCIP_GENERATION_SUMMARY: &str = ".codelens/scip-generation-summary.json";

#[cfg(feature = "scip-backend")]
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct ScipStaleness {
    pub(crate) index_path: String,
    pub(crate) stale_files: Vec<(String, u64)>,
}

#[cfg(feature = "scip-backend")]
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct ScipGeneratorWarnings {
    pub(crate) summary_path: String,
    pub(crate) log_path: String,
    pub(crate) warning_count: usize,
    pub(crate) precision_risk_warning_count: usize,
    pub(crate) known_generator_noise_count: usize,
    pub(crate) duplicate_symbol_count: usize,
    pub(crate) missing_document_definition_count: usize,
    pub(crate) unnamed_enclosing_definition_count: usize,
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

#[cfg(feature = "scip-backend")]
pub(crate) fn detect_scip_generator_warnings(
    project_root: &std::path::Path,
) -> Option<ScipGeneratorWarnings> {
    let index_path = codelens_engine::ScipBackend::detect(project_root)?;
    let summary_path = project_root.join(SCIP_GENERATION_SUMMARY);
    let summary_meta = std::fs::metadata(&summary_path).ok()?;
    let index_mtime = std::fs::metadata(&index_path).ok()?.modified().ok()?;
    let summary_mtime = summary_meta.modified().ok()?;
    if summary_mtime < index_mtime {
        return None;
    }

    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&summary_path).ok()?).ok()?;
    let warning_count = summary.get("warning_count")?.as_u64()? as usize;
    if warning_count == 0 {
        return None;
    }

    let log_path = summary
        .get("log_path")
        .and_then(|value| value.as_str())
        .unwrap_or(".codelens/scip-generation.log")
        .to_owned();
    let duplicate_symbol_count = summary
        .get("duplicate_symbol_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let missing_document_definition_count = summary
        .get("missing_document_definition_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let unnamed_enclosing_definition_count = summary
        .get("unnamed_enclosing_definition_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let precision_risk_warning_count = summary
        .get("precision_risk_warning_count")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(duplicate_symbol_count + missing_document_definition_count);
    let known_generator_noise_count = summary
        .get("known_generator_noise_count")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(unnamed_enclosing_definition_count);
    Some(ScipGeneratorWarnings {
        summary_path: summary_path.display().to_string(),
        log_path,
        warning_count,
        precision_risk_warning_count,
        known_generator_noise_count,
        duplicate_symbol_count,
        missing_document_definition_count,
        unnamed_enclosing_definition_count,
    })
}

/// Issue #243: convert a 0-indexed SCIP `parse_range` line to the
/// 1-indexed convention every other CodeLens surface (tree-sitter,
/// `read_file`, grep, IDE) uses. Single source of truth so the +1
/// shift can't drift out of sync between `find_symbol`,
/// `find_referencing_symbols`, and `get_callers`. Saturating add is
/// a defence against `usize::MAX` sentinel rows we don't expect to
/// see but shouldn't panic on either.
#[cfg(feature = "scip-backend")]
pub(crate) fn scip_line_to_display(scip_line: usize) -> usize {
    scip_line.saturating_add(1)
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

#[cfg(feature = "scip-backend")]
pub(crate) fn scip_generator_warnings_payload(
    warnings: &ScipGeneratorWarnings,
) -> serde_json::Value {
    let (severity, message, recommended_action) = if warnings.precision_risk_warning_count > 0 {
        (
            "degraded_precision",
            "Last SCIP generation completed with file/symbol precision-risk warnings. The index is still usable, but duplicate or missing SCIP definitions may make precise-tier lookup ambiguous for affected symbols.",
            "inspect_scip_generation_log",
        )
    } else {
        (
            "generator_noise_only",
            "Last SCIP generation completed with known rust-analyzer generator noise only. The index is still usable and no file/symbol precision-risk warnings were reported.",
            "no_action_required",
        )
    };
    json!({
        "code": "scip_generator_warnings",
        "severity": severity,
        "message": message,
        "recommended_action": recommended_action,
        "action_target": "scip_backend",
        "generator": "rust-analyzer scip",
        "summary_path": warnings.summary_path.as_str(),
        "log_path": warnings.log_path.as_str(),
        "warning_count": warnings.warning_count,
        "precision_risk_warning_count": warnings.precision_risk_warning_count,
        "known_generator_noise_count": warnings.known_generator_noise_count,
        "duplicate_symbol_count": warnings.duplicate_symbol_count,
        "missing_document_definition_count": warnings.missing_document_definition_count,
        "unnamed_enclosing_definition_count": warnings.unnamed_enclosing_definition_count,
    })
}

#[cfg(all(test, feature = "scip-backend"))]
mod tests {
    use super::{
        ScipStaleness, detect_scip_generator_warnings, detect_scip_staleness,
        scip_generator_warnings_payload, scip_line_to_display, scip_stale_warning_payload,
    };
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

    fn write_generation_summary(
        root: &std::path::Path,
        warning_count: usize,
        duplicate_symbol_count: usize,
        missing_document_definition_count: usize,
        unnamed_enclosing_definition_count: usize,
    ) -> std::path::PathBuf {
        let codelens_dir = root.join(".codelens");
        std::fs::create_dir_all(&codelens_dir).expect("mkdir .codelens");
        let summary_path = codelens_dir.join("scip-generation-summary.json");
        let precision_risk_warning_count =
            duplicate_symbol_count + missing_document_definition_count;
        let known_generator_noise_count = unnamed_enclosing_definition_count;
        std::fs::write(
            &summary_path,
            format!(
                r#"{{
                    "schema_version": 2,
                    "generator": "rust-analyzer scip",
                    "log_path": ".codelens/scip-generation.log",
                    "warning_count": {warning_count},
                    "precision_risk_warning_count": {precision_risk_warning_count},
                    "known_generator_noise_count": {known_generator_noise_count},
                    "duplicate_symbol_count": {duplicate_symbol_count},
                    "missing_document_definition_count": {missing_document_definition_count},
                    "unnamed_enclosing_definition_count": {unnamed_enclosing_definition_count}
                }}"#
            ),
        )
        .expect("write generation summary");
        summary_path
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

    #[test]
    fn scip_line_to_display_shifts_zero_indexed_to_one_indexed() {
        // Issue #243 regression: SCIP `parse_range` returns 0-indexed
        // line numbers per spec; the rest of the CodeLens surface
        // (tree-sitter / read_file / grep / IDE) is 1-indexed. The
        // helper must shift exactly one row.
        assert_eq!(scip_line_to_display(0), 1);
        assert_eq!(scip_line_to_display(93), 94);
        assert_eq!(scip_line_to_display(413), 414);
    }

    #[test]
    fn scip_line_to_display_saturates_on_max_sentinel() {
        // `find_callees` uses `usize::MAX` as a synthetic
        // next-definition sentinel — adding 1 must not panic.
        assert_eq!(scip_line_to_display(usize::MAX), usize::MAX);
    }

    #[test]
    fn generator_warnings_summary_is_structured_when_current() {
        let dir = build_fixture(60, 600, &["src/lib.rs"]);
        let summary_path = write_generation_summary(dir.path(), 132, 17, 24, 91);

        let warnings = detect_scip_generator_warnings(dir.path()).expect("warnings");
        assert_eq!(warnings.warning_count, 132);
        assert_eq!(warnings.precision_risk_warning_count, 41);
        assert_eq!(warnings.known_generator_noise_count, 91);
        assert_eq!(warnings.duplicate_symbol_count, 17);
        assert_eq!(warnings.missing_document_definition_count, 24);
        assert_eq!(warnings.unnamed_enclosing_definition_count, 91);
        assert_eq!(warnings.log_path, ".codelens/scip-generation.log");
        assert_eq!(warnings.summary_path, summary_path.display().to_string());

        let payload = scip_generator_warnings_payload(&warnings);
        assert_eq!(payload["code"], "scip_generator_warnings");
        assert_eq!(payload["severity"], "degraded_precision");
        assert_eq!(payload["recommended_action"], "inspect_scip_generation_log");
        assert_eq!(payload["action_target"], "scip_backend");
        assert_eq!(payload["precision_risk_warning_count"], 41);
        assert_eq!(payload["known_generator_noise_count"], 91);
        assert_eq!(payload["duplicate_symbol_count"], 17);
    }

    #[test]
    fn generator_warnings_payload_downgrades_noise_only_runs() {
        let dir = build_fixture(60, 600, &["src/lib.rs"]);
        write_generation_summary(dir.path(), 91, 0, 0, 91);

        let warnings = detect_scip_generator_warnings(dir.path()).expect("warnings");
        assert_eq!(warnings.precision_risk_warning_count, 0);
        assert_eq!(warnings.known_generator_noise_count, 91);

        let payload = scip_generator_warnings_payload(&warnings);
        assert_eq!(payload["severity"], "generator_noise_only");
        assert_eq!(payload["recommended_action"], "no_action_required");
        assert_eq!(payload["precision_risk_warning_count"], 0);
        assert_eq!(payload["known_generator_noise_count"], 91);
    }

    #[test]
    fn generator_warnings_summary_ignores_zero_warning_runs() {
        let dir = build_fixture(60, 600, &["src/lib.rs"]);
        write_generation_summary(dir.path(), 0, 0, 0, 0);
        assert!(detect_scip_generator_warnings(dir.path()).is_none());
    }

    #[test]
    fn generator_warnings_summary_ignores_stale_summary() {
        let dir = build_fixture(60, 600, &["src/lib.rs"]);
        let summary_path = write_generation_summary(dir.path(), 132, 17, 24, 91);
        let stale = SystemTime::now() - Duration::from_secs(600);
        filetime::set_file_mtime(&summary_path, filetime::FileTime::from_system_time(stale))
            .expect("backdate summary");
        assert!(detect_scip_generator_warnings(dir.path()).is_none());
    }
}
