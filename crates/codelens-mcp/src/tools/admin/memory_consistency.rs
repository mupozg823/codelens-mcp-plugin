//! Memory-consistency detector: surfaces stale `.codelens/memories/*.md` files.
//!
//! Extracted from `admin.rs` (ADR-0009 §2 + §6). Single responsibility:
//! `audit_memory_consistency` + its private helper `file_has_stable_marker`.
//!
//! The `#[cfg(test)]` block also carries the full surface-audit regression
//! suite (originally in `admin.rs`) because those tests exercise both this
//! module's functions and the sibling detectors via `super::` — keeping them
//! co-located with the module that owns the `file_has_stable_marker` private
//! helper (accessible only within this file's test scope via `super::*`).

use super::super::{AppState, ToolResult, optional_usize, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use serde_json::{Value, json};

/// Surface project memory files (`.codelens/memories/*.md`) whose
/// modification time exceeds the staleness threshold. Memories are
/// frozen-in-time observations — without a freshness check they
/// silently drift from the codebase they describe (e.g. cited
/// file paths get renamed, cited symbols disappear, cited
/// architectural claims stop matching the code). This is the
/// self-auditability complement to the four detectors that audit
/// the tool surface; the same pattern (runtime query, admin-only,
/// preset_tags=[]) applies.
///
/// Threshold is configurable via `threshold_days` argument
/// (default 30, clamped 1..=3650). Each entry reports the file
/// path (relative to project root), age in days, and modification
/// timestamp (epoch millis) so callers can fold the output back
/// into a freshness ratchet.
pub fn audit_memory_consistency(state: &AppState, arguments: &Value) -> ToolResult {
    let threshold_days = optional_usize(arguments, "threshold_days", 30).clamp(1, 3650);
    let threshold_secs = (threshold_days as u64) * 24 * 60 * 60;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let memories_dir = state.memories_dir();
    let project_root = state.project().as_path().to_path_buf();

    let mut total_files = 0u64;
    let mut stable_skipped = 0u64;
    let mut stale: Vec<Value> = Vec::new();

    if memories_dir.exists() {
        let entries = match std::fs::read_dir(&memories_dir) {
            Ok(rd) => rd,
            Err(error) => {
                return Err(CodeLensError::Io(error));
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            total_files += 1;
            // `<!-- audit-skip: stable -->` marker (anywhere in the first
            // 4 lines) opts a memory out of staleness checks. Use for
            // ADRs, benchmark snapshots, post-mortems — entries whose
            // accuracy is pinned to a moment in time and is not expected
            // to age into invalidity.
            if file_has_stable_marker(&path) {
                stable_skipped += 1;
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let Ok(mtime_dur) = modified.duration_since(std::time::UNIX_EPOCH) else {
                continue;
            };
            let mtime_secs = mtime_dur.as_secs();
            let age_secs = now_secs.saturating_sub(mtime_secs);
            if age_secs <= threshold_secs {
                continue;
            }
            let relative = path
                .strip_prefix(&project_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());
            stale.push(json!({
                "file": relative,
                "age_days": age_secs / (24 * 60 * 60),
                "mtime_epoch_secs": mtime_secs,
            }));
        }
    }

    // Sort stale entries deterministically — oldest first surfaces
    // the most egregious drift to readers / dashboards.
    stale.sort_by(|a, b| {
        b["age_days"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["age_days"].as_u64().unwrap_or(0))
            .then_with(|| {
                a["file"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["file"].as_str().unwrap_or(""))
            })
    });

    let stale_count = stale.len();
    let all_clean = stale_count == 0;
    let mut next_actions: Vec<String> = Vec::new();
    if all_clean {
        next_actions.push(format!(
            "No stale memories ({total_files} file(s) scanned, all newer than {threshold_days} days)."
        ));
    } else {
        next_actions.push(format!(
            "{stale_count} memory file(s) older than {threshold_days} days. Re-verify against current code, update mtime by re-saving, or delete if obsolete.",
        ));
    }

    Ok((
        json!({
            "memories_dir": memories_dir.to_string_lossy(),
            "total_files": total_files,
            "stable_skipped": stable_skipped,
            "stale_count": stale_count,
            "stale_entries": stale,
            "threshold_days": threshold_days,
            "all_clean": all_clean,
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::Filesystem, 1.0),
    ))
}

/// Returns true when the first 4 lines of `path` contain the
/// `<!-- audit-skip: stable -->` marker (case-sensitive, exact text).
/// Used by [`audit_memory_consistency`] to opt out point-in-time
/// snapshots (ADRs, benchmark results, post-mortems) from staleness
/// checks. The narrow scan window keeps the IO bounded — a typo
/// further down in the file deliberately won't match.
fn file_has_stable_marker(path: &std::path::Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let reader = BufReader::new(file);
    reader
        .lines()
        .take(4)
        .filter_map(Result::ok)
        .any(|line| line.contains("<!-- audit-skip: stable -->"))
}

#[cfg(test)]
mod surface_audit_tests {
    use super::*;
    use crate::state::AppState;
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;

    fn make_state() -> AppState {
        // The audit reads only the static tool registry and the dispatch
        // table; the project root is essentially unused. Use the lightweight
        // minimal constructor so we don't spin up watchers or index workers
        // for every test invocation.
        AppState::new_minimal(temp_project_root("audit_v2"), ToolPreset::Full)
    }

    #[test]
    fn audit_returns_three_violation_buckets() {
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let data = payload.as_object().expect("object response");
        let violations = data["violations"].as_object().expect("violations object");
        assert!(violations.contains_key("missing_in_dispatch"));
        assert!(violations.contains_key("missing_in_toml"));
        assert!(violations.contains_key("orphan_in_preset"));
        assert!(data.contains_key("all_clean"));
        assert!(data.contains_key("violation_count"));
        assert!(data.contains_key("layers_checked"));
    }

    #[test]
    fn audit_surface_has_no_unintentional_drift() {
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        assert_eq!(
            payload["violation_count"].as_u64().unwrap_or(u64::MAX),
            0,
            "tool surface drift must stay at zero: {}",
            payload
        );
        assert_eq!(payload["all_clean"].as_bool(), Some(true));
    }

    #[test]
    fn audit_reports_script_parity_surface_drift_section() {
        // Phase 3 (#346): the runtime audit must speak the same 3-way
        // vocabulary as scripts/regen-tool-defs.py three_way_report so
        // CI (script) and live daemons (this tool) can be diffed 1:1.
        // Clean tree: every drift list empty except the pending-D3
        // allowlist; tombstone re-introduction is a violation bucket.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let drift = payload["surface_drift"]
            .as_object()
            .expect("surface_drift section present");
        assert_eq!(drift["dispatch_only"], json!([]), "got {drift:?}");
        assert_eq!(drift["schema_only"], json!([]), "got {drift:?}");
        assert_eq!(drift["preset_dead"], json!([]), "got {drift:?}");
        assert_eq!(drift["tombstone_reintroduced"], json!([]), "got {drift:?}");
        let allow = drift["allowlisted_dispatch_only"]
            .as_array()
            .expect("allowlisted_dispatch_only list");
        let symbolic = drift["pending_d3_symbolic_edit_core"]
            .as_array()
            .expect("pending_d3_symbolic_edit_core list");
        let substrate = drift["pending_d3_refactor_substrate"]
            .as_array()
            .expect("pending_d3_refactor_substrate list");
        for name in crate::tools::PENDING_D3_ALLOWLIST {
            assert!(
                allow.iter().any(|v| v == name),
                "{name} must appear in allowlisted_dispatch_only, got {allow:?}"
            );
        }
        for name in crate::tools::PENDING_D3_SYMBOLIC_EDIT_CORE {
            assert!(
                symbolic.iter().any(|v| v == name),
                "{name} must appear in pending_d3_symbolic_edit_core, got {symbolic:?}"
            );
        }
        for name in crate::tools::PENDING_D3_REFACTOR_SUBSTRATE {
            assert!(
                substrate.iter().any(|v| v == name),
                "{name} must appear in pending_d3_refactor_substrate, got {substrate:?}"
            );
        }
        assert_eq!(
            payload["summary"]["tombstoned_count"].as_u64(),
            Some(crate::tools::TOMBSTONED_TOOLS.len() as u64),
            "summary must carry the tombstone inventory size"
        );
    }

    #[test]
    fn audit_surfaces_pending_d3_allowlist_without_violations() {
        // #346: the 9 dispatch-only symbolic-edit/refactor tools are an
        // explicit carve-out, not drift. They must land in the
        // `pending_d3_allowlisted` bucket (mirroring the script's
        // DISPATCH_ONLY_ALLOWLIST) while `all_clean` stays true.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let bucket = payload["pending_d3_allowlisted"]
            .as_array()
            .expect("pending_d3_allowlisted bucket present");
        let symbolic = payload["pending_d3_symbolic_edit_core"]
            .as_array()
            .expect("pending_d3_symbolic_edit_core bucket present");
        let substrate = payload["pending_d3_refactor_substrate"]
            .as_array()
            .expect("pending_d3_refactor_substrate bucket present");
        for name in crate::tools::PENDING_D3_ALLOWLIST {
            assert!(
                bucket.iter().any(|v| v == name),
                "{name} must be surfaced in pending_d3_allowlisted, got {bucket:?}"
            );
        }
        assert_eq!(
            symbolic.len(),
            crate::tools::PENDING_D3_SYMBOLIC_EDIT_CORE.len()
        );
        assert_eq!(
            substrate.len(),
            crate::tools::PENDING_D3_REFACTOR_SUBSTRATE.len()
        );
        assert_eq!(
            symbolic.len() + substrate.len(),
            bucket.len(),
            "split pending-D3 buckets must cover the combined allowlist"
        );
        assert_eq!(payload["all_clean"].as_bool(), Some(true));
    }

    #[test]
    fn audit_self_includes_itself_in_dispatch() {
        // Once the dispatch_table arm + tools.toml entry land in the
        // same PR, calling the audit must see itself on both sides:
        // its own name must NOT appear in `missing_in_dispatch` or
        // `missing_in_toml`. This pins the registration to keep the
        // audit self-discoverable in subsequent PRs.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let violations = &payload["violations"];
        let missing_dispatch = violations["missing_in_dispatch"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "audit_tool_surface_consistency"))
            .unwrap_or(false);
        let missing_toml = violations["missing_in_toml"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "audit_tool_surface_consistency"))
            .unwrap_or(false);
        assert!(
            !missing_dispatch,
            "audit_tool_surface_consistency must be in dispatch_table (regression guard)"
        );
        assert!(
            !missing_toml,
            "audit_tool_surface_consistency must be in tools.toml (regression guard)"
        );
    }

    #[test]
    fn resurrected_detectors_registered_on_both_sides() {
        // Regression guard for the v1.13.27 surface trim follow-up: the two
        // detectors must stay in dispatch_table + tools.toml together. If
        // one drifts, the audit's `missing_in_*` buckets surface it.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let violations = &payload["violations"];
        for tool in ["find_phantom_modules", "find_redundant_definitions"] {
            let missing_dispatch = violations["missing_in_dispatch"]
                .as_array()
                .map(|arr| arr.iter().any(|v| v == tool))
                .unwrap_or(false);
            let missing_toml = violations["missing_in_toml"]
                .as_array()
                .map(|arr| arr.iter().any(|v| v == tool))
                .unwrap_or(false);
            assert!(!missing_dispatch, "{tool} must be in dispatch_table");
            assert!(!missing_toml, "{tool} must be in tools.toml");
        }
    }

    #[test]
    fn find_phantom_modules_on_empty_project_returns_zero() {
        let state = make_state();
        let (payload, _meta) =
            super::super::find_phantom_modules(&state, &json!({})).expect("call succeeds");
        assert_eq!(payload["count"].as_u64().unwrap_or(99), 0);
        assert!(payload["phantom_modules"].as_array().unwrap().is_empty());
        assert!(!payload["truncated"].as_bool().unwrap_or(true));
        assert_eq!(payload["max_results"].as_u64().unwrap_or(0), 50);
    }

    #[test]
    #[cfg(feature = "semantic")]
    fn resurrected_semantic_detectors_all_registered_in_toml() {
        // Sibling guard to find_misplaced_code_registered_in_toml.
        // find_similar_code, find_code_duplicates, classify_symbol all
        // have feature-gated dispatch handlers in dispatch/table.rs
        // that survived the Sprint B-3 schema trim. This pins the
        // four sibling schemas against future Sprint B-style cleanups.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let toml_tools: Vec<String> = payload["violations"]["missing_in_toml"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .collect();
        for tool in [
            "find_misplaced_code",
            "find_similar_code",
            "find_code_duplicates",
            "classify_symbol",
        ] {
            assert!(
                !toml_tools.contains(&tool.to_owned()),
                "{tool} must be in tools.toml — handler was preserved through Sprint B-3, schema restored here"
            );
        }
    }

    #[test]
    #[cfg(feature = "semantic")]
    fn find_misplaced_code_registered_in_toml() {
        // find_misplaced_code's dispatch handler has lived in
        // dispatch/table.rs since v1.13.6 (feature-gated on `semantic`).
        // The Sprint B-3 cleanup (6726e663) only dropped the tools.toml
        // schema; the handler remained. This guard pins the schema
        // re-registration so the audit no longer surfaces it as drift.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let toml_tools: Vec<String> = payload["violations"]["missing_in_toml"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .collect();
        assert!(
            !toml_tools.contains(&"find_misplaced_code".to_owned()),
            "find_misplaced_code must be in tools.toml — was dropped in Sprint B-3 and restored here"
        );
    }

    #[test]
    fn audit_memory_consistency_registered_on_both_sides() {
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let violations = &payload["violations"];
        let tool = "audit_memory_consistency";
        let missing_dispatch = violations["missing_in_dispatch"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == tool))
            .unwrap_or(false);
        let missing_toml = violations["missing_in_toml"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == tool))
            .unwrap_or(false);
        assert!(!missing_dispatch, "{tool} must be in dispatch_table");
        assert!(!missing_toml, "{tool} must be in tools.toml");
    }

    #[test]
    fn audit_memory_consistency_shape_and_threshold_keys() {
        let state = make_state();
        let (payload, _meta) = audit_memory_consistency(&state, &json!({})).expect("call succeeds");
        assert!(payload["total_files"].is_number());
        assert!(payload["stale_count"].is_number());
        assert!(payload["all_clean"].is_boolean());
        assert_eq!(payload["threshold_days"].as_u64().unwrap_or(0), 30);
        assert!(payload["memories_dir"].is_string());
        assert!(payload["stale_entries"].is_array());
        assert!(!payload["next_actions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn audit_memory_consistency_response_includes_stable_skipped_field() {
        // Even on a synthetic project the new `stable_skipped` field
        // must appear in the response envelope so callers can rely on it.
        let state = make_state();
        let (payload, _meta) = audit_memory_consistency(&state, &json!({})).expect("call succeeds");
        assert!(
            payload["stable_skipped"].is_number(),
            "stable_skipped must always be present (got: {})",
            payload["stable_skipped"]
        );
    }

    #[test]
    fn file_has_stable_marker_detects_only_within_first_four_lines() {
        use std::io::Write;

        let mut early = tempfile::NamedTempFile::new().expect("early marker file");
        writeln!(early, "# Title").unwrap();
        writeln!(early, "<!-- audit-skip: stable -->").unwrap();
        writeln!(early, "body").unwrap();
        early.flush().unwrap();
        assert!(
            file_has_stable_marker(early.path()),
            "marker on line 2 detected"
        );

        let mut late = tempfile::NamedTempFile::new().expect("late marker file");
        writeln!(late, "# line 1").unwrap();
        writeln!(late, "body line 2").unwrap();
        writeln!(late, "body line 3").unwrap();
        writeln!(late, "body line 4").unwrap();
        writeln!(late, "<!-- audit-skip: stable -->").unwrap();
        late.flush().unwrap();
        assert!(
            !file_has_stable_marker(late.path()),
            "marker beyond line 4 must not be detected — keeps the IO window bounded"
        );

        let mut missing = tempfile::NamedTempFile::new().expect("missing marker file");
        writeln!(missing, "# Title").unwrap();
        writeln!(missing, "no marker here").unwrap();
        missing.flush().unwrap();
        assert!(!file_has_stable_marker(missing.path()));
    }

    #[test]
    fn audit_memory_consistency_clamps_threshold_days() {
        let state = make_state();
        let (low, _) = audit_memory_consistency(&state, &json!({"threshold_days": 0}))
            .expect("call succeeds with 0");
        assert_eq!(low["threshold_days"].as_u64().unwrap_or(0), 1);
        let (high, _) = audit_memory_consistency(&state, &json!({"threshold_days": 10000}))
            .expect("call succeeds with 10000");
        assert_eq!(high["threshold_days"].as_u64().unwrap_or(0), 3650);
    }

    #[test]
    fn find_over_visible_apis_registered_on_both_sides() {
        // Regression guard: the detector must stay in dispatch_table +
        // tools.toml together. Same pattern as `audit_self_includes_itself_in_dispatch`.
        let state = make_state();
        let (payload, _meta) = super::super::audit_tool_surface_consistency(&state, &json!({}))
            .expect("audit succeeds");
        let violations = &payload["violations"];
        let missing_dispatch = violations["missing_in_dispatch"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "find_over_visible_apis"))
            .unwrap_or(false);
        let missing_toml = violations["missing_in_toml"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "find_over_visible_apis"))
            .unwrap_or(false);
        assert!(
            !missing_dispatch,
            "find_over_visible_apis must be in dispatch_table"
        );
        assert!(
            !missing_toml,
            "find_over_visible_apis must be in tools.toml"
        );
    }

    #[test]
    fn find_over_visible_apis_shape_and_policy_keys() {
        // Smoke-test the shape: should return JSON with violations,
        // violation_count, all_clean, readonly_surfaces_checked,
        // policy, next_actions. Content of violations depends on the
        // current preset whitelists, so we don't pin specific tool names.
        let state = make_state();
        let (payload, _meta) =
            super::super::find_over_visible_apis(&state, &json!({})).expect("call succeeds");
        assert!(payload["violation_count"].is_number());
        assert!(payload["all_clean"].is_boolean());
        let surfaces = payload["readonly_surfaces_checked"]
            .as_array()
            .expect("readonly_surfaces_checked is an array");
        assert_eq!(surfaces.len(), 3, "checks 3 readonly-intent surfaces");
        assert!(
            payload["policy"]["destructive_hint_true"].is_string(),
            "policy.destructive_hint_true documented",
        );
        assert!(
            payload["policy"]["approval_required_true"].is_string(),
            "policy.approval_required_true documented",
        );
        assert!(
            !payload["next_actions"].as_array().unwrap().is_empty(),
            "at least one next_action surfaced",
        );
        // Every violation entry must carry tool + surface + reasons.
        for v in payload["violations"].as_array().unwrap() {
            assert!(v["tool"].is_string());
            assert!(v["surface"].is_string());
            let reasons = v["reasons"].as_array().expect("reasons is an array");
            assert!(
                !reasons.is_empty(),
                "violation must cite at least one reason"
            );
        }
    }

    #[test]
    fn find_redundant_definitions_on_empty_project_returns_zero() {
        let state = make_state();
        let (payload, _meta) =
            super::super::find_redundant_definitions(&state, &json!({})).expect("call succeeds");
        assert_eq!(payload["count"].as_u64().unwrap_or(99), 0);
        assert!(
            payload["redundant_definitions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(!payload["truncated"].as_bool().unwrap_or(true));
        assert_eq!(payload["max_results"].as_u64().unwrap_or(0), 50);
    }
}
