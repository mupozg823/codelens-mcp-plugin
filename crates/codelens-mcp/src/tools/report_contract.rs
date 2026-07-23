use super::report_payload::{build_handle_payload, infer_risk_level};
use super::report_verifier::build_verifier_contract;
use super::{AppState, ToolResult, success_meta};
use crate::protocol::BackendKind;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{ToolProfile, ToolSurface};
use serde_json::Value;
use std::collections::BTreeMap;

fn overlapping_claims_from_section(value: &Value) -> Vec<Value> {
    value
        .get("claims")
        .and_then(|claims| claims.as_array())
        .cloned()
        .unwrap_or_default()
}

fn overlapping_claims_from_sections(sections: &BTreeMap<String, Value>) -> Vec<Value> {
    sections
        .get("coordination_overlaps")
        .map(overlapping_claims_from_section)
        .unwrap_or_default()
}

fn overlapping_claims_from_artifact(state: &AppState, analysis_id: &str) -> Vec<Value> {
    state
        .peek_analysis_section(analysis_id, "coordination_overlaps")
        .ok()
        .map(|value| overlapping_claims_from_section(&value))
        .unwrap_or_default()
}

/// Issue #342: bind an args-only cache key to the symbol-index generation
/// so a cached report artifact and a fresh recomputation always describe
/// the same index state — the fingerprint is derived from the same source
/// the analysis reads. `None` must stay `None`: generic artifacts keep
/// their `cache_key = None` warm/cold-tier semantics (G2).
fn fingerprint_cache_key(
    cache_key: Option<String>,
    max_indexed_at: Option<i64>,
    file_count: usize,
) -> Option<String> {
    cache_key.map(|key| format!("{key}|idx:{}:{file_count}", max_indexed_at.unwrap_or(0)))
}

fn section_array_len(sections: &BTreeMap<String, Value>, section: &str, field: &str) -> usize {
    sections
        .get(section)
        .and_then(|value| value.get(field))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn analysis_is_incomplete(sections: &BTreeMap<String, Value>) -> bool {
    matches!(
        sections
            .get("analysis_completeness")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("partial" | "unavailable")
    )
}

fn cached_artifact_satisfies_current_contract(
    tool_name: &str,
    available_sections: &[String],
) -> bool {
    !matches!(tool_name, "module_boundary_report" | "mermaid_module_graph")
        || available_sections
            .iter()
            .any(|section| section == "analysis_completeness")
}

fn infer_handle_risk_level(
    tool_name: &str,
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
    sections: &BTreeMap<String, Value>,
) -> String {
    if tool_name == "module_boundary_report" {
        let cycle_count = section_array_len(sections, "cycle_hits", "cycles");
        if cycle_count > 0 {
            return "high".to_owned();
        }
        let coupling_count = section_array_len(sections, "coupling_hits", "couplings");
        let importer_count = section_array_len(sections, "impact", "direct_importers");
        let affected_count = sections
            .get("impact")
            .and_then(|value| value.get("total_affected_files"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if analysis_is_incomplete(sections)
            || coupling_count > 0
            || importer_count > 0
            || affected_count > 0
        {
            return "medium".to_owned();
        }
        return "low".to_owned();
    }

    if tool_name == "mermaid_module_graph" {
        let stats = sections.get("stats");
        let edge_count = stats
            .and_then(|value| value.get("module_edge_count"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let upstream_count = stats
            .and_then(|value| value.get("upstream_total"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let downstream_count = stats
            .and_then(|value| value.get("downstream_total"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        return if analysis_is_incomplete(sections)
            || edge_count > 0
            || upstream_count > 0
            || downstream_count > 0
        {
            "medium".to_owned()
        } else {
            "low".to_owned()
        };
    }

    infer_risk_level(summary, top_findings, next_actions).to_owned()
}

fn attach_analysis_completeness(payload: &mut Value, completeness: Option<&Value>) {
    if let Some(completeness) = completeness {
        payload["analysis_completeness"] = completeness.clone();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make_handle_response(
    state: &AppState,
    tool_name: &str,
    cache_key: Option<String>,
    summary: String,
    top_findings: Vec<String>,
    confidence: f64,
    next_actions: Vec<String>,
    mut sections: BTreeMap<String, Value>,
    touched_files: Vec<String>,
    symbol_hint: Option<String>,
    arguments: Option<&Value>,
) -> ToolResult {
    // Both the reuse lookup and the store below must see the same
    // fingerprinted key, so the transform happens once, up front.
    let cache_key = {
        let index = state.symbol_index();
        fingerprint_cache_key(
            cache_key,
            index.max_indexed_at().ok().flatten(),
            index.file_count().unwrap_or(0),
        )
    };
    let logical_session_id = arguments
        .map(SessionRequestContext::from_json)
        .map(|session| session.session_id);
    let logical_session_id = logical_session_id.as_deref();
    let risk_level =
        infer_handle_risk_level(tool_name, &summary, &top_findings, &next_actions, &sections);
    let ci_audit = matches!(*state.surface(), ToolSurface::Profile(ToolProfile::CiAudit));
    let inline_overlapping_claims = overlapping_claims_from_sections(&sections);
    let verifier = build_verifier_contract(
        state,
        tool_name,
        &summary,
        &top_findings,
        &next_actions,
        &mut sections,
        &touched_files,
        symbol_hint.as_deref(),
    );
    let analysis_completeness = sections.get("analysis_completeness").cloned();
    if let Some(cache_key) = cache_key.as_deref()
        && let Some((artifact, tier)) =
            state.find_reusable_analysis_tiered_for_current_scope(tool_name, cache_key)
        && cached_artifact_satisfies_current_contract(tool_name, &artifact.available_sections)
    {
        state
            .metrics()
            .record_analysis_cache_hit_tiered_for_session(tier, logical_session_id);
        let mut data = build_handle_payload(
            tool_name,
            &artifact.id,
            &artifact.summary,
            &artifact.top_findings,
            &artifact.risk_level,
            artifact.confidence,
            &artifact.next_actions,
            &artifact.blockers,
            &artifact.readiness,
            &artifact.verifier_checks,
            &artifact.available_sections,
            &touched_files,
            true,
            ci_audit,
        );
        attach_analysis_completeness(&mut data, analysis_completeness.as_ref());
        data["cache_hit_tier"] = serde_json::json!(tier.as_str());
        let overlapping_claims = overlapping_claims_from_artifact(state, &artifact.id);
        if !overlapping_claims.is_empty() {
            data["overlapping_claims"] = serde_json::json!(overlapping_claims);
        }
        state.metrics().record_quality_contract_emitted_for_session(
            data["quality_focus"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            data["recommended_checks"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            data["performance_watchpoints"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            logical_session_id,
        );
        state
            .metrics()
            .record_verifier_contract_emitted_for_session(
                data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
                data["verifier_checks"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
                logical_session_id,
            );
        return Ok((data, success_meta(BackendKind::Hybrid, artifact.confidence)));
    }
    let artifact = state.store_analysis_for_current_scope(
        tool_name,
        cache_key,
        summary.clone(),
        top_findings.clone(),
        risk_level,
        confidence,
        next_actions.clone(),
        verifier.blockers.clone(),
        verifier.readiness.clone(),
        verifier.verifier_checks.clone(),
        sections,
    )?;
    let mut data = build_handle_payload(
        tool_name,
        &artifact.id,
        &artifact.summary,
        &artifact.top_findings,
        &artifact.risk_level,
        artifact.confidence,
        &artifact.next_actions,
        &artifact.blockers,
        &artifact.readiness,
        &artifact.verifier_checks,
        &artifact.available_sections,
        &touched_files,
        false,
        ci_audit,
    );
    attach_analysis_completeness(&mut data, analysis_completeness.as_ref());
    if !inline_overlapping_claims.is_empty() {
        data["overlapping_claims"] = serde_json::json!(inline_overlapping_claims);
    }
    state.metrics().record_quality_contract_emitted_for_session(
        data["quality_focus"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        data["recommended_checks"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        data["performance_watchpoints"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        logical_session_id,
    );
    state
        .metrics()
        .record_verifier_contract_emitted_for_session(
            data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
            data["verifier_checks"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            logical_session_id,
        );
    // Cross-phase: inject recent analysis IDs so agents can reference prior results.
    let prior_ids = state.recent_analysis_ids();
    if prior_ids.len() > 1 {
        // Exclude current analysis from the list.
        let prior: Vec<_> = prior_ids
            .iter()
            .filter(|id| id.as_str() != artifact.id)
            .cloned()
            .collect();
        if !prior.is_empty() {
            data["prior_analyses"] = serde_json::json!(prior);
        }
    }
    Ok((data, success_meta(BackendKind::Hybrid, confidence)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;
    use serde_json::json;

    fn issue_report(state: &AppState, cache_key: Option<String>) -> Value {
        let sections = BTreeMap::from([(
            "analysis_completeness".to_owned(),
            json!({
                "status": "complete",
                "scope_kind": "file",
                "in_scope_file_count": 1,
                "in_scope_file_limit_hit": false,
            }),
        )]);
        make_handle_response(
            state,
            "module_boundary_report",
            cache_key,
            "boundary summary".to_owned(),
            vec!["finding".to_owned()],
            0.9,
            vec!["action".to_owned()],
            sections,
            Vec::new(),
            None,
            None,
        )
        .expect("handle response")
        .0
    }

    fn args_only_key() -> Option<String> {
        // Shape produced by `stable_cache_key` — args only, no content signal.
        Some(r#"{"fields":{"path":"lib.rs"},"tool":"module_boundary_report"}"#.to_owned())
    }

    #[test]
    fn module_boundary_risk_uses_positive_cycle_evidence() {
        let mut sections = BTreeMap::new();
        sections.insert("cycle_hits".to_owned(), json!({"cycles": []}));
        sections.insert("coupling_hits".to_owned(), json!({"couplings": []}));
        sections.insert(
            "impact".to_owned(),
            json!({"direct_importers": [], "total_affected_files": 0}),
        );
        sections.insert(
            "analysis_completeness".to_owned(),
            json!({"status": "complete"}),
        );

        assert_eq!(
            infer_handle_risk_level(
                "module_boundary_report",
                "structural risk report",
                &["0 cycle hit(s)".to_owned()],
                &["Check cycle evidence".to_owned()],
                &sections,
            ),
            "low"
        );

        sections.insert(
            "cycle_hits".to_owned(),
            json!({"cycles": [["a.py", "b.py", "a.py"]]}),
        );
        assert_eq!(
            infer_handle_risk_level(
                "module_boundary_report",
                "structural risk report",
                &["1 cycle hit(s)".to_owned()],
                &[],
                &sections,
            ),
            "high"
        );
    }

    #[test]
    fn partial_architecture_evidence_is_medium_risk() {
        let mut sections = BTreeMap::new();
        sections.insert("cycle_hits".to_owned(), json!({"cycles": []}));
        sections.insert("coupling_hits".to_owned(), json!({"couplings": []}));
        sections.insert(
            "impact".to_owned(),
            json!({"direct_importers": [], "total_affected_files": 0}),
        );
        sections.insert(
            "analysis_completeness".to_owned(),
            json!({"status": "partial"}),
        );

        assert_eq!(
            infer_handle_risk_level(
                "module_boundary_report",
                "structural report",
                &[],
                &[],
                &sections,
            ),
            "medium"
        );
    }

    #[test]
    fn unavailable_architecture_evidence_is_not_low_risk() {
        let mut sections = BTreeMap::new();
        sections.insert("cycle_hits".to_owned(), json!({"cycles": []}));
        sections.insert("coupling_hits".to_owned(), json!({"couplings": []}));
        sections.insert(
            "impact".to_owned(),
            json!({"direct_importers": [], "total_affected_files": 0}),
        );
        sections.insert(
            "analysis_completeness".to_owned(),
            json!({"status": "unavailable"}),
        );

        assert_eq!(
            infer_handle_risk_level(
                "module_boundary_report",
                "structural report",
                &[],
                &[],
                &sections,
            ),
            "medium"
        );
    }

    #[test]
    fn legacy_architecture_cache_without_completeness_is_not_reused() {
        let project = temp_project_root("cache-architecture-contract");
        let state = AppState::new_minimal(project, ToolPreset::Full);
        state
            .symbol_index()
            .get_symbols_overview("lib.rs", 1)
            .expect("index lib.rs");
        let legacy_key = {
            let index = state.symbol_index();
            fingerprint_cache_key(
                args_only_key(),
                index.max_indexed_at().ok().flatten(),
                index.file_count().unwrap_or(0),
            )
        };
        state
            .store_analysis_for_current_scope(
                "module_boundary_report",
                legacy_key,
                "legacy boundary summary".to_owned(),
                vec![],
                "low".to_owned(),
                0.9,
                vec![],
                vec![],
                crate::runtime_types::AnalysisReadiness::default(),
                vec![],
                BTreeMap::new(),
            )
            .expect("store legacy artifact");

        let response = issue_report(&state, args_only_key());

        assert_eq!(response["reused"], json!(false));
        assert_ne!(response["summary"], json!("legacy boundary summary"));
    }

    /// G2 invariant: generic artifacts (`cache_key = None`) must keep
    /// their warm/cold-tier semantics — the fingerprint never conjures
    /// a key out of `None`.
    #[test]
    fn fingerprint_preserves_none_key() {
        assert_eq!(fingerprint_cache_key(None, Some(1_000), 42), None);
    }

    #[test]
    fn fingerprint_stable_for_same_index_generation() {
        let key = || Some("k".to_owned());
        assert_eq!(
            fingerprint_cache_key(key(), Some(1_000), 42),
            fingerprint_cache_key(key(), Some(1_000), 42),
        );
    }

    /// Add/modify/move signal: `MAX(indexed_at)` moved → different key.
    #[test]
    fn fingerprint_changes_when_max_indexed_at_moves() {
        let key = || Some("k".to_owned());
        assert_ne!(
            fingerprint_cache_key(key(), Some(1_000), 42),
            fingerprint_cache_key(key(), Some(2_000), 42),
        );
    }

    /// Pure-deletion signal: MAX unchanged, count moved → different key.
    #[test]
    fn fingerprint_changes_when_file_count_moves() {
        let key = || Some("k".to_owned());
        assert_ne!(
            fingerprint_cache_key(key(), Some(1_000), 42),
            fingerprint_cache_key(key(), Some(1_000), 41),
        );
    }

    /// Disk-format compatibility: a pre-#342 artifact persisted with the
    /// raw args-only key must never match the fingerprinted key — old
    /// entries degrade to misses, no migration required.
    #[test]
    fn fingerprinted_key_never_matches_legacy_key() {
        let legacy = args_only_key();
        let fingerprinted = fingerprint_cache_key(args_only_key(), Some(1_000), 42);
        assert_ne!(legacy, fingerprinted);
    }

    /// Invariant: same arguments + unchanged index generation must keep
    /// hitting the exact cache tier — the #342 fix may only add misses
    /// when the index actually changed.
    #[test]
    fn exact_cache_hit_preserved_when_index_unchanged() {
        let project = temp_project_root("cache-fp-stable");
        let state = AppState::new_minimal(project, ToolPreset::Full);
        state
            .symbol_index()
            .get_symbols_overview("lib.rs", 1)
            .expect("index lib.rs");

        let first = issue_report(&state, args_only_key());
        assert_eq!(first["reused"], json!(false));
        let second = issue_report(&state, args_only_key());
        assert_eq!(second["reused"], json!(true));
        assert_eq!(second["cache_hit_tier"], json!("exact"));
    }

    /// Issue #342 regression: a file added to the index after an artifact
    /// was cached must invalidate the exact-tier reuse for the same
    /// arguments — the cached analysis no longer reflects the index.
    #[test]
    fn index_file_add_invalidates_exact_cache() {
        let project = temp_project_root("cache-fp-add");
        let state = AppState::new_minimal(project.clone(), ToolPreset::Full);
        state
            .symbol_index()
            .get_symbols_overview("lib.rs", 1)
            .expect("index lib.rs");

        issue_report(&state, args_only_key());
        let warm = issue_report(&state, args_only_key());
        assert_eq!(warm["reused"], json!(true));

        std::fs::write(project.as_path().join("extra.rs"), "fn extra() {}\n")
            .expect("write extra.rs");
        state
            .symbol_index()
            .get_symbols_overview("extra.rs", 1)
            .expect("index extra.rs");

        let after_add = issue_report(&state, args_only_key());
        assert_eq!(
            after_add["reused"],
            json!(false),
            "index generation changed (file added) — cached artifact must not be reused"
        );
    }

    /// Issue #342 regression (move = delete + add, file count unchanged):
    /// the fresh `indexed_at` of the re-added path must flip the
    /// fingerprint even though the count stays identical.
    #[test]
    fn index_file_move_invalidates_exact_cache() {
        let project = temp_project_root("cache-fp-move");
        let state = AppState::new_minimal(project.clone(), ToolPreset::Full);
        state
            .symbol_index()
            .get_symbols_overview("lib.rs", 1)
            .expect("index lib.rs");

        issue_report(&state, args_only_key());
        let warm = issue_report(&state, args_only_key());
        assert_eq!(warm["reused"], json!(true));

        // Simulate the watcher's rename handling: tombstone the old path,
        // index the new one. `indexed_at` has millisecond granularity, so
        // tick past the original generation before re-indexing.
        std::fs::rename(
            project.as_path().join("lib.rs"),
            project.as_path().join("moved.rs"),
        )
        .expect("rename lib.rs");
        state
            .symbol_index()
            .db()
            .delete_file("lib.rs")
            .expect("tombstone old path");
        std::thread::sleep(std::time::Duration::from_millis(5));
        state
            .symbol_index()
            .get_symbols_overview("moved.rs", 1)
            .expect("index moved.rs");

        let after_move = issue_report(&state, args_only_key());
        assert_eq!(
            after_move["reused"],
            json!(false),
            "index generation changed (file moved) — cached artifact must not be reused"
        );
    }
}
