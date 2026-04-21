//! Structured "decision" records for MCP tool responses.
//!
//! Each entry describes one internal decision (sampling, shadow-file
//! suppression, backend downgrade, …) that changed the answer relative
//! to "run the query unfiltered and return everything". The emitter
//! (`inject_into`) writes the full set into both `data.limits_applied`
//! and `_meta.decisions` so consumers that walk either location see an
//! identical, structured explanation.
//!
//! Phase 1 wires three kinds on `find_referencing_symbols`:
//!   - `sampling`            — returned < count because of sample_limit / max_results
//!   - `shadow_suppression`  — files dropped because they re-declare the symbol
//!   - `backend_degraded`    — LSP failed, fell back to tree-sitter
//!
//! Later phases add `budget_prune`, `depth_limit`, `filter_applied`,
//! `exact_match_only`, `index_partial`. See
//! docs/superpowers/specs/2026-04-19-transparency-fields-design.md.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitsKind {
    Sampling,
    ShadowSuppression,
    BackendDegraded,
    BudgetPrune,
    DepthLimit,
    FilterApplied,
    ExactMatchOnly,
    IndexPartial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LimitsApplied {
    pub kind: LimitsKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    pub reason: String,
    pub remedy: String,
}

impl LimitsApplied {
    /// Result array was truncated from `total` to `returned` because of
    /// a sampling/pagination parameter. `param` names the parameter
    /// responsible (e.g. `"sample_limit=8"`).
    pub fn sampling(total: usize, returned: usize, param: impl Into<String>) -> Self {
        let dropped = total.saturating_sub(returned);
        Self {
            kind: LimitsKind::Sampling,
            total: Some(total),
            returned: Some(returned),
            dropped: Some(dropped),
            param: Some(param.into()),
            reason: format!("returned {returned} of {total} (sampled)"),
            remedy: "set full_results=true or raise max_results to retrieve the full set".into(),
        }
    }

    /// Whole files were dropped by shadow-file suppression because they
    /// redefine the target symbol. `dropped_files` is the number of
    /// files removed; the call retains structural recall inside the
    /// declaration file.
    pub fn shadow_suppression(dropped_files: usize) -> Self {
        Self {
            kind: LimitsKind::ShadowSuppression,
            total: None,
            returned: None,
            dropped: Some(dropped_files),
            param: None,
            reason: format!(
                "{dropped_files} file(s) dropped because they re-declare the target symbol (shadow suppression)"
            ),
            remedy: "pass declaration_file to scope the search, or inspect the shadowing files individually".into(),
        }
    }

    /// The preferred backend failed (LSP, SCIP, …) and the tool
    /// fell back to an alternative. `reason` is the raw failure
    /// message; `fallback_backend` is the backend that actually served
    /// the response.
    pub fn backend_degraded(
        reason: impl Into<String>,
        fallback_backend: impl Into<String>,
    ) -> Self {
        let fallback = fallback_backend.into();
        Self {
            kind: LimitsKind::BackendDegraded,
            total: None,
            returned: None,
            dropped: None,
            param: None,
            reason: reason.into(),
            remedy: format!(
                "served by {fallback}; run `check_lsp_status` to diagnose the preferred backend, then `get_lsp_recipe` for install instructions"
            ),
        }
    }

    /// The ranker's budget-aware prune dropped symbols whose blended
    /// score did not fit in the caller's `max_tokens`. `returned` is
    /// the kept count, `total` is the candidate set before pruning,
    /// `last_kept_score` is the score of the lowest-ranked kept entry
    /// so the caller can judge how close they are to losing relevant
    /// context. `param` names the budget parameter (`max_tokens=…`).
    pub fn budget_prune(
        returned: usize,
        total: usize,
        last_kept_score: f64,
        param: impl Into<String>,
    ) -> Self {
        let dropped = total.saturating_sub(returned);
        Self {
            kind: LimitsKind::BudgetPrune,
            total: Some(total),
            returned: Some(returned),
            dropped: Some(dropped),
            param: Some(param.into()),
            reason: format!(
                "kept top {returned} of {total} by blended score; last kept score {last_kept_score:.2}"
            ),
            remedy:
                "raise max_tokens or narrow the query to fit the most relevant context in budget"
                    .into(),
        }
    }

    /// `get_symbols_overview` trimmed the tree because the requested
    /// (or default) depth cap would have exceeded the caller's token
    /// budget. `param` names the depth cap driving the decision.
    pub fn depth_limit(param: impl Into<String>) -> Self {
        Self {
            kind: LimitsKind::DepthLimit,
            total: None,
            returned: None,
            dropped: None,
            param: Some(param.into()),
            reason: "symbol tree trimmed at the depth limit".into(),
            remedy: "pass an explicit `depth` greater than the current limit, or narrow `path` to a sub-tree".into(),
        }
    }

    /// The tool applied a caller-supplied filter (glob, file type,
    /// exclude pattern) that narrowed the candidate set before
    /// matching. `param` names the filter (`file_glob=…`).
    pub fn filter_applied(param: impl Into<String>) -> Self {
        Self {
            kind: LimitsKind::FilterApplied,
            total: None,
            returned: None,
            dropped: None,
            param: Some(param.into()),
            reason: "caller-supplied filter narrowed the candidate set before matching".into(),
            remedy: "remove or broaden the filter to see matches that were excluded".into(),
        }
    }

    /// `find_symbol` refused to return a fuzzy match because the
    /// caller did not opt into one and the exact name was not found.
    /// `query` is the rejected input so the remedy text can cite it.
    pub fn exact_match_only(query: impl Into<String>) -> Self {
        let q = query.into();
        Self {
            kind: LimitsKind::ExactMatchOnly,
            total: None,
            returned: None,
            dropped: None,
            param: Some(format!("name={q}")),
            reason: format!("no exact match for `{q}`; fuzzy matching requires a different tool"),
            remedy: "call bm25_symbol_search or search_workspace_symbols for fuzzy / partial-name retrieval".into(),
        }
    }

    /// A required index (embedding, SCIP, symbol) was not fully warm
    /// when the call was served; the result may be less complete than
    /// the tool could produce on a fully indexed repo. `index` names
    /// the cold lane (`semantic`, `scip`, `symbols`).
    pub fn index_partial(index: impl Into<String>) -> Self {
        let index = index.into();
        Self {
            kind: LimitsKind::IndexPartial,
            total: None,
            returned: None,
            dropped: None,
            param: Some(format!("index={index}")),
            reason: format!("{index} index was not fully warm when the call was served"),
            remedy: "call refresh_symbol_index or warm the index out-of-band before relying on completeness".into(),
        }
    }
}

/// Serialize `decisions` once and attach the result to both
/// `data.limits_applied` and `meta.decisions`. Both targets MUST be
/// JSON objects; if either is not an object the corresponding side is
/// a no-op.
///
/// The two attached values are byte-identical clones of the same
/// serialized array — consumers that walk only `data` and consumers
/// that walk only `_meta` see the same thing.
pub fn inject_into(data: &mut Value, meta: &mut Value, decisions: &[LimitsApplied]) {
    let array = serde_json::to_value(decisions).unwrap_or_else(|_| Value::Array(Vec::new()));
    if let Some(obj) = data.as_object_mut() {
        obj.insert("limits_applied".into(), array.clone());
    }
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("decisions".into(), array);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn limits_applied_serializes_with_expected_field_names() {
        let entry = LimitsApplied {
            kind: LimitsKind::Sampling,
            total: Some(62),
            returned: Some(8),
            dropped: Some(54),
            param: Some("sample_limit=8".into()),
            reason: "sample_limit reached".into(),
            remedy: "set full_results=true or raise max_results".into(),
        };
        let v = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(v["kind"], json!("sampling"));
        assert_eq!(v["total"], json!(62));
        assert_eq!(v["returned"], json!(8));
        assert_eq!(v["dropped"], json!(54));
        assert_eq!(v["param"], json!("sample_limit=8"));
        assert_eq!(v["reason"], json!("sample_limit reached"));
        assert_eq!(
            v["remedy"],
            json!("set full_results=true or raise max_results")
        );
    }

    #[test]
    fn optional_numeric_fields_are_omitted_when_none() {
        let entry = LimitsApplied {
            kind: LimitsKind::BackendDegraded,
            total: None,
            returned: None,
            dropped: None,
            param: None,
            reason: "LSP unavailable".into(),
            remedy: "attach an LSP server via check_lsp_status".into(),
        };
        let v = serde_json::to_value(&entry).expect("serialize");
        assert!(v.get("total").is_none(), "total should be omitted: {v}");
        assert!(v.get("returned").is_none());
        assert!(v.get("dropped").is_none());
        assert!(v.get("param").is_none());
        assert_eq!(v["kind"], json!("backend_degraded"));
    }

    #[test]
    fn all_phase1_kinds_have_snake_case_wire_names() {
        for (kind, wire) in [
            (LimitsKind::Sampling, "sampling"),
            (LimitsKind::ShadowSuppression, "shadow_suppression"),
            (LimitsKind::BackendDegraded, "backend_degraded"),
        ] {
            let v = serde_json::to_value(kind).expect("serialize kind");
            assert_eq!(v, json!(wire), "kind {:?}", kind);
        }
    }

    #[test]
    fn sampling_constructor_fills_counts_and_remedy() {
        let entry = LimitsApplied::sampling(62, 8, "sample_limit=8");
        assert_eq!(entry.kind, LimitsKind::Sampling);
        assert_eq!(entry.total, Some(62));
        assert_eq!(entry.returned, Some(8));
        assert_eq!(entry.dropped, Some(54));
        assert_eq!(entry.param.as_deref(), Some("sample_limit=8"));
        assert!(entry.remedy.contains("full_results=true"));
        assert!(entry.remedy.contains("max_results"));
    }

    #[test]
    fn shadow_suppression_constructor_reports_file_count() {
        let entry = LimitsApplied::shadow_suppression(3);
        assert_eq!(entry.kind, LimitsKind::ShadowSuppression);
        assert_eq!(entry.dropped, Some(3));
        assert!(entry.param.is_none());
        assert!(entry.reason.contains("shadow"));
        assert!(entry.remedy.contains("declaration_file"));
    }

    #[test]
    fn backend_degraded_constructor_carries_reason() {
        let entry = LimitsApplied::backend_degraded("LSP failed", "tree_sitter");
        assert_eq!(entry.kind, LimitsKind::BackendDegraded);
        assert!(entry.reason.contains("LSP failed"));
        assert!(entry.remedy.contains("tree_sitter"));
        assert!(
            entry.remedy.contains("check_lsp_status"),
            "remedy must name check_lsp_status as the concrete next tool: {}",
            entry.remedy
        );
        assert!(
            entry.remedy.contains("get_lsp_recipe"),
            "remedy must name get_lsp_recipe as the install oracle: {}",
            entry.remedy
        );
        assert!(entry.total.is_none() && entry.returned.is_none() && entry.dropped.is_none());
    }

    #[test]
    fn inject_into_writes_both_locations_byte_identically() {
        let decisions = vec![
            LimitsApplied::sampling(62, 8, "sample_limit=8"),
            LimitsApplied::shadow_suppression(2),
        ];
        let mut data = json!({ "references": [] });
        let mut meta = json!({ "backend_used": "tree_sitter" });
        inject_into(&mut data, &mut meta, &decisions);
        assert_eq!(
            data["limits_applied"], meta["decisions"],
            "data.limits_applied and _meta.decisions must be byte-identical"
        );
        assert_eq!(data["limits_applied"].as_array().map(Vec::len), Some(2));
        assert_eq!(data["limits_applied"][0]["kind"], json!("sampling"));
        assert_eq!(
            data["limits_applied"][1]["kind"],
            json!("shadow_suppression")
        );
    }

    #[test]
    fn inject_into_writes_empty_array_when_decisions_empty() {
        let mut data = json!({ "references": [] });
        let mut meta = json!({});
        inject_into(&mut data, &mut meta, &[]);
        assert_eq!(data["limits_applied"], json!([]));
        assert_eq!(meta["decisions"], json!([]));
    }

    #[test]
    fn inject_into_preserves_existing_fields() {
        let decisions = vec![LimitsApplied::sampling(10, 5, "max_results=5")];
        let mut data = json!({ "references": ["a", "b"], "count": 10 });
        let mut meta = json!({ "backend_used": "tree_sitter", "confidence": 0.85 });
        inject_into(&mut data, &mut meta, &decisions);
        assert_eq!(data["references"], json!(["a", "b"]));
        assert_eq!(data["count"], json!(10));
        assert_eq!(meta["backend_used"], json!("tree_sitter"));
        assert_eq!(meta["confidence"], json!(0.85));
    }

    #[test]
    fn phase2_kinds_serialize_as_snake_case() {
        for (kind, wire) in [
            (LimitsKind::BudgetPrune, "budget_prune"),
            (LimitsKind::DepthLimit, "depth_limit"),
            (LimitsKind::FilterApplied, "filter_applied"),
            (LimitsKind::ExactMatchOnly, "exact_match_only"),
            (LimitsKind::IndexPartial, "index_partial"),
        ] {
            let v = serde_json::to_value(kind).expect("serialize kind");
            assert_eq!(v, json!(wire), "kind {:?}", kind);
        }
    }

    #[test]
    fn budget_prune_constructor_carries_drop_stats() {
        let entry = LimitsApplied::budget_prune(34, 258, 0.41, "max_tokens=6000");
        assert_eq!(entry.kind, LimitsKind::BudgetPrune);
        assert_eq!(entry.total, Some(258));
        assert_eq!(entry.returned, Some(34));
        assert_eq!(entry.dropped, Some(224));
        assert_eq!(entry.param.as_deref(), Some("max_tokens=6000"));
        assert!(
            entry.reason.contains("0.41"),
            "last_kept_score must be visible: {}",
            entry.reason
        );
        assert!(entry.remedy.contains("max_tokens"));
    }

    #[test]
    fn depth_limit_constructor_reports_param() {
        let entry = LimitsApplied::depth_limit("depth=2");
        assert_eq!(entry.kind, LimitsKind::DepthLimit);
        assert_eq!(entry.param.as_deref(), Some("depth=2"));
        assert!(entry.reason.contains("depth"));
        assert!(entry.remedy.contains("depth"));
    }

    #[test]
    fn filter_applied_constructor_names_filter() {
        let entry = LimitsApplied::filter_applied("file_glob=*.rs");
        assert_eq!(entry.kind, LimitsKind::FilterApplied);
        assert_eq!(entry.param.as_deref(), Some("file_glob=*.rs"));
        assert!(entry.reason.contains("filter"));
        assert!(entry.remedy.contains("remove") || entry.remedy.contains("broaden"));
    }

    #[test]
    fn exact_match_only_constructor_names_fallback_tools() {
        let entry = LimitsApplied::exact_match_only("register");
        assert_eq!(entry.kind, LimitsKind::ExactMatchOnly);
        assert!(entry.reason.contains("register"));
        assert!(entry.remedy.contains("bm25_symbol_search"));
        assert!(entry.remedy.contains("search_workspace_symbols"));
    }

    #[test]
    fn index_partial_constructor_reports_missing_signal() {
        let entry = LimitsApplied::index_partial("semantic");
        assert_eq!(entry.kind, LimitsKind::IndexPartial);
        assert_eq!(entry.param.as_deref(), Some("index=semantic"));
        assert!(entry.reason.contains("semantic"));
        assert!(entry.remedy.contains("refresh_symbol_index") || entry.remedy.contains("warm"));
    }
}
