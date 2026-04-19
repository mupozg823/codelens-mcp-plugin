//! Shared helpers for Phase 2+ transparency wiring on tool handlers
//! that don't own the `lsp::finalize_text_refs_response` machinery.
//!
//! Every Phase 2 tool follows the same pattern: build a
//! `Vec<LimitsApplied>` locally, then attach it to both the outgoing
//! `data` payload (as `limits_applied`) and the outgoing
//! `ToolResponseMeta.decisions`. This module owns that seam so the
//! handler files stay free of envelope bookkeeping.

use crate::limits::{self, LimitsApplied};
use crate::protocol::ToolResponseMeta;
use serde_json::Value;

/// Attach `decisions` to both `data.limits_applied` (an empty array if
/// the slice is empty) and `meta.decisions` (empty vec if empty).
/// Always present when the tool participates in the transparency
/// layer — callers that opt in should call this unconditionally with
/// a possibly-empty slice, so consumers can tell "no trims today"
/// from "this tool doesn't participate".
pub(crate) fn attach_decisions_to_meta(
    data: &mut Value,
    meta: &mut ToolResponseMeta,
    decisions: Vec<LimitsApplied>,
) {
    let mut fake_meta = serde_json::json!({});
    limits::inject_into(data, &mut fake_meta, &decisions);
    if let Some(array) = fake_meta.get("decisions").and_then(|v| v.as_array()) {
        meta.decisions = array.clone();
    } else {
        meta.decisions = Vec::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{AnalysisSource, Freshness, ToolResponseMeta};
    use serde_json::json;

    fn blank_meta() -> ToolResponseMeta {
        ToolResponseMeta {
            backend_used: "tree_sitter".into(),
            confidence: 0.9,
            degraded_reason: None,
            source: AnalysisSource::Native,
            partial: false,
            freshness: Freshness::Live,
            staleness_ms: None,
            decisions: Vec::new(),
        }
    }

    #[test]
    fn empty_decisions_yield_empty_limits_applied_and_meta() {
        let mut data = json!({ "symbols": [] });
        let mut meta = blank_meta();
        attach_decisions_to_meta(&mut data, &mut meta, Vec::new());
        assert_eq!(data["limits_applied"], json!([]));
        assert!(meta.decisions.is_empty());
    }

    #[test]
    fn nonempty_decisions_are_byte_equal_on_data_and_meta() {
        let mut data = json!({ "symbols": [] });
        let mut meta = blank_meta();
        let decisions = vec![
            LimitsApplied::depth_limit("depth=1"),
            LimitsApplied::budget_prune(10, 50, 0.3, "max_tokens=4000"),
        ];
        attach_decisions_to_meta(&mut data, &mut meta, decisions);
        let data_array = data["limits_applied"].as_array().expect("array");
        assert_eq!(data_array.len(), 2);
        assert_eq!(data_array, &meta.decisions);
    }
}
