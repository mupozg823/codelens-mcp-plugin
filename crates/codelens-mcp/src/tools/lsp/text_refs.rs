use crate::limits::{self, LimitsApplied};
use serde_json::json;

pub(super) fn build_text_refs_response_with_decisions(
    references: Vec<serde_json::Value>,
    total_count: usize,
    sampled: bool,
    include_context: bool,
    extra_decisions: Vec<LimitsApplied>,
) -> serde_json::Value {
    let returned_count = references.len();
    let mut data = json!({
        "references": references,
        "count": total_count,
        "returned_count": returned_count,
        "sampled": sampled,
        "include_context": include_context,
    });
    let mut meta = json!({});

    let mut decisions: Vec<LimitsApplied> = Vec::with_capacity(1 + extra_decisions.len());
    if sampled {
        let entry = LimitsApplied::sampling(total_count, returned_count, "sample_limit");
        data["sampling_notice"] = json!(format!(
            "Returned {returned_count} of {total_count} matches (sampled). \
             Set `full_results=true` or raise `max_results` to retrieve the full set."
        ));
        decisions.push(entry);
    }
    decisions.extend(extra_decisions);

    limits::inject_into(&mut data, &mut meta, &decisions);
    json!({ "data": data, "_meta": meta })
}

fn compact_text_references(
    references: Vec<codelens_engine::TextReference>,
    include_context: bool,
    full_results: bool,
    sample_limit: usize,
) -> (Vec<serde_json::Value>, usize, bool) {
    let total_count = references.len();
    let effective_limit = if full_results {
        references.len()
    } else {
        sample_limit.min(references.len())
    };
    let sampled = !full_results && total_count > effective_limit;
    let compact = references
        .into_iter()
        .take(effective_limit)
        .map(|reference| {
            let container = reference.enclosing_symbol.as_ref().map(|symbol| {
                json!({
                    "name_path": symbol.name_path,
                    "kind": symbol.kind,
                    "signature": symbol.signature,
                    "start_line": symbol.start_line,
                    "end_line": symbol.end_line,
                })
            });
            let line_text = reference.line_content.trim_end_matches('\n');
            let match_line_number = reference.line;
            let before_start_line = reference
                .line
                .saturating_sub(reference.context_before.len());
            let before_decorated: Vec<String> = reference
                .context_before
                .iter()
                .enumerate()
                .map(|(idx, text)| format!("... {}: {}", before_start_line + idx, text))
                .collect();
            let after_decorated: Vec<String> = reference
                .context_after
                .iter()
                .enumerate()
                .map(|(idx, text)| format!("... {}: {}", match_line_number + 1 + idx, text))
                .collect();
            let snippet = json!({
                "line": match_line_number,
                "match": format!("> {}: {}", match_line_number, line_text),
                "text": line_text,
                "before": reference.context_before,
                "after": reference.context_after,
                "before_decorated": before_decorated,
                "after_decorated": after_decorated,
            });
            let mut value = json!({
                "file_path": reference.file_path,
                "line": reference.line,
                "column": reference.column,
                "is_declaration": reference.is_declaration,
                "container": container,
                "snippet": snippet,
            });
            if include_context {
                value["line_content"] = json!(reference.line_content);
                if let Some(symbol) = reference.enclosing_symbol {
                    value["enclosing_symbol"] = json!(symbol);
                }
            }
            value
        })
        .collect::<Vec<_>>();
    (compact, total_count, sampled)
}

pub(super) fn finalize_text_refs_response(
    report: codelens_engine::TextRefsReport,
    include_context: bool,
    full_results: bool,
    sample_limit: usize,
    leading_decisions: Vec<LimitsApplied>,
    mut meta: crate::protocol::ToolResponseMeta,
) -> (serde_json::Value, crate::protocol::ToolResponseMeta) {
    let shadow_count = report.shadow_files_suppressed.len();
    let (references, total_count, sampled) = compact_text_references(
        report.references,
        include_context,
        full_results,
        sample_limit,
    );
    let mut extra = leading_decisions;
    if shadow_count > 0 {
        extra.push(LimitsApplied::shadow_suppression(shadow_count));
    }
    let envelope = build_text_refs_response_with_decisions(
        references,
        total_count,
        sampled,
        include_context,
        extra,
    );
    let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
    let decisions_array = envelope
        .get("_meta")
        .and_then(|m| m.get("decisions"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    meta.decisions = decisions_array;
    (data, meta)
}

#[cfg(test)]
mod sampling_notice_tests {
    use super::build_text_refs_response_with_decisions;
    use serde_json::json;

    #[test]
    fn notice_and_limits_are_absent_when_not_sampled() {
        let resp = build_text_refs_response_with_decisions(
            vec![json!({"file_path": "a.py", "line": 1})],
            1,
            false,
            false,
            Vec::new(),
        );
        assert_eq!(resp["data"]["sampled"], json!(false));
        assert!(resp["data"].get("sampling_notice").is_none());
        assert_eq!(resp["data"]["limits_applied"], json!([]));
        assert_eq!(resp["_meta"]["decisions"], json!([]));
    }

    #[test]
    fn sampled_response_contains_structured_sampling_entry_and_headline_notice() {
        let refs = vec![
            json!({"file_path": "a.py", "line": 1}),
            json!({"file_path": "a.py", "line": 2}),
        ];
        let resp = build_text_refs_response_with_decisions(refs, 62, true, false, Vec::new());
        assert_eq!(resp["data"]["sampled"], json!(true));

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("sampling"));
        assert_eq!(limits[0]["total"], json!(62));
        assert_eq!(limits[0]["returned"], json!(2));
        assert_eq!(limits[0]["dropped"], json!(60));
        assert!(
            limits[0]["remedy"]
                .as_str()
                .unwrap()
                .contains("full_results=true"),
            "remedy must guide caller: {}",
            limits[0]["remedy"]
        );

        assert_eq!(resp["data"]["limits_applied"], resp["_meta"]["decisions"]);

        let notice = resp["data"]["sampling_notice"].as_str().expect("string");
        assert!(notice.contains("2 of 62"), "notice={notice}");
    }

    #[test]
    fn shadow_suppression_emits_decision_when_files_dropped() {
        use crate::limits::LimitsApplied;

        let refs = vec![json!({"file_path": "a.py", "line": 1})];
        let extra = vec![LimitsApplied::shadow_suppression(2)];
        let resp = build_text_refs_response_with_decisions(refs, 1, false, false, extra);

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("shadow_suppression"));
        assert_eq!(limits[0]["dropped"], json!(2));
        assert_eq!(resp["data"]["limits_applied"], resp["_meta"]["decisions"]);
    }

    #[test]
    fn fallback_path_emits_backend_degraded_decision() {
        use crate::limits::LimitsApplied;

        let refs = vec![json!({"file_path": "a.py", "line": 1})];
        let extra = vec![LimitsApplied::backend_degraded(
            "LSP failed, used tree-sitter",
            "tree_sitter",
        )];
        let resp = build_text_refs_response_with_decisions(refs, 1, false, false, extra);

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("backend_degraded"));
        assert!(limits[0]["reason"].as_str().unwrap().contains("LSP failed"));
        assert!(
            limits[0]["remedy"]
                .as_str()
                .unwrap()
                .contains("tree_sitter")
        );
    }

    #[test]
    fn all_combinations_keep_data_and_meta_byte_equal() {
        use crate::limits::LimitsApplied;

        let scenarios: Vec<(bool, Vec<LimitsApplied>)> = vec![
            (false, vec![]),
            (true, vec![]),
            (false, vec![LimitsApplied::shadow_suppression(3)]),
            (
                true,
                vec![
                    LimitsApplied::shadow_suppression(1),
                    LimitsApplied::backend_degraded("LSP failed", "tree_sitter"),
                ],
            ),
        ];

        for (sampled, extra) in scenarios {
            let refs = vec![json!({"file_path": "a.py", "line": 1})];
            let extra_len = extra.len();
            let resp = build_text_refs_response_with_decisions(refs, 5, sampled, false, extra);
            assert_eq!(
                resp["data"]["limits_applied"], resp["_meta"]["decisions"],
                "byte-equality failed for sampled={sampled}, extra_len={extra_len}"
            );
        }
    }
}
