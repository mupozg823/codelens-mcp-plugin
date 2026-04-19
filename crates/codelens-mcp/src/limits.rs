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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitsKind {
    Sampling,
    ShadowSuppression,
    BackendDegraded,
    // Phase 2 kinds added here as they land:
    // BudgetPrune, DepthLimit, FilterApplied, ExactMatchOnly, IndexPartial,
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
}
