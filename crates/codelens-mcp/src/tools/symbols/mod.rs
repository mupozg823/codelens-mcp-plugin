mod analyzer;
mod formatter;
mod handlers;

pub(crate) use analyzer::{semantic_lane_ready, semantic_results_for_query, semantic_status};
pub use handlers::{
    bm25_symbol_search, find_symbol, flatten_symbols, get_complexity, get_project_structure,
    get_ranked_context, get_symbols_overview, refresh_symbol_index, search_symbols_fuzzy,
};

#[cfg(test)]
mod tests {
    use super::analyzer::{annotate_ranked_context_provenance, merge_semantic_ranked_entries};
    use super::formatter::truncate_body_preview;
    use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};
    use serde_json::json;

    #[test]
    fn merge_semantic_ranked_entries_inserts_and_upgrades() {
        let mut result = RankedContextResult {
            query: "rename across project".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 128,
            pruned_count: 0,
            last_kept_score: 0.0,
            symbols: vec![RankedContextEntry {
                name: "project_scope_renames_across_files".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-engine/src/rename.rs".to_owned(),
                line: 10,
                signature: "fn project_scope_renames_across_files".to_owned(),
                body: None,
                relevance_score: 32,
            }],
        };

        merge_semantic_ranked_entries(
            "rename a variable or function across the project",
            &mut result,
            vec![
                SemanticMatch {
                    symbol_name: "project_scope_renames_across_files".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                    line: 10,
                    signature: "fn project_scope_renames_across_files".to_owned(),
                    name_path: "project_scope_renames_across_files".to_owned(),
                    score: 0.41,
                },
                SemanticMatch {
                    symbol_name: "rename_symbol".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                    line: 42,
                    signature: "fn rename_symbol".to_owned(),
                    name_path: "rename_symbol".to_owned(),
                    score: 0.93,
                },
            ],
            8,
        );

        assert_eq!(result.symbols[0].name, "rename_symbol");
        assert!(result.symbols[0].relevance_score >= 90);
        assert!(
            result
                .symbols
                .iter()
                .find(|entry| entry.name == "project_scope_renames_across_files")
                .unwrap()
                .relevance_score
                > 32
        );
    }

    #[test]
    fn short_phrase_merge_only_inserts_top_confident_semantic_hit() {
        let mut result = RankedContextResult {
            query: "change function parameters".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 64,
            pruned_count: 0,
            last_kept_score: 0.0,
            symbols: vec![RankedContextEntry {
                name: "change_signature".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-engine/src/refactor.rs".to_owned(),
                line: 12,
                signature: "fn change_signature".to_owned(),
                body: None,
                relevance_score: 41,
            }],
        };

        merge_semantic_ranked_entries(
            "change function parameters",
            &mut result,
            vec![
                SemanticMatch {
                    symbol_name: "apply_signature_change".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-engine/src/refactor.rs".to_owned(),
                    line: 44,
                    signature: "fn apply_signature_change".to_owned(),
                    name_path: "apply_signature_change".to_owned(),
                    score: 0.32,
                },
                SemanticMatch {
                    symbol_name: "rewrite_call_arguments".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-engine/src/refactor.rs".to_owned(),
                    line: 60,
                    signature: "fn rewrite_call_arguments".to_owned(),
                    name_path: "rewrite_call_arguments".to_owned(),
                    score: 0.27,
                },
            ],
            8,
        );

        assert!(
            result
                .symbols
                .iter()
                .any(|entry| entry.name == "apply_signature_change")
        );
        assert!(
            !result
                .symbols
                .iter()
                .any(|entry| entry.name == "rewrite_call_arguments")
        );
    }

    #[test]
    fn truncate_body_preview_respects_utf8_boundaries() {
        let body = "가나다abc";
        let (preview, truncated) = truncate_body_preview(body, 10, 4);
        assert!(truncated);
        assert!(preview.starts_with("가"));
        assert!(!preview.starts_with("가나"));
    }

    #[test]
    fn annotate_ranked_context_provenance_marks_structural_and_semantic_entries() {
        let result = RankedContextResult {
            query: "rename across project".to_owned(),
            count: 2,
            token_budget: 1200,
            chars_used: 128,
            pruned_count: 0,
            last_kept_score: 0.0,
            symbols: vec![
                RankedContextEntry {
                    name: "project_scope_renames_across_files".to_owned(),
                    kind: "function".to_owned(),
                    file: "crates/codelens-engine/src/rename.rs".to_owned(),
                    line: 10,
                    signature: "fn project_scope_renames_across_files".to_owned(),
                    body: None,
                    relevance_score: 64,
                },
                RankedContextEntry {
                    name: "rename_symbol".to_owned(),
                    kind: "function".to_owned(),
                    file: "crates/codelens-engine/src/rename.rs".to_owned(),
                    line: 42,
                    signature: "fn rename_symbol".to_owned(),
                    body: None,
                    relevance_score: 91,
                },
            ],
        };
        let structural_keys = std::collections::HashSet::from([format!(
            "{}:{}",
            "crates/codelens-engine/src/rename.rs", "project_scope_renames_across_files"
        )]);
        let semantic_results = vec![
            SemanticMatch {
                symbol_name: "project_scope_renames_across_files".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                line: 10,
                signature: "fn project_scope_renames_across_files".to_owned(),
                name_path: "project_scope_renames_across_files".to_owned(),
                score: 0.411,
            },
            SemanticMatch {
                symbol_name: "rename_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                line: 42,
                signature: "fn rename_symbol".to_owned(),
                name_path: "rename_symbol".to_owned(),
                score: 0.933,
            },
        ];

        let mut payload = json!(result);
        annotate_ranked_context_provenance(&mut payload, &structural_keys, &semantic_results);

        let symbols = payload["symbols"].as_array().unwrap();
        assert_eq!(
            symbols[0]["provenance"]["source"],
            json!("semantic_boosted")
        );
        assert_eq!(symbols[1]["provenance"]["source"], json!("semantic_added"));
        assert_eq!(symbols[1]["provenance"]["semantic_score"], json!(0.933));
    }
}
