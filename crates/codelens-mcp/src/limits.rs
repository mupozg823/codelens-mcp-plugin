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
}
