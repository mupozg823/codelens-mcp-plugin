mod analyzer;
mod formatter;
mod fusion;
mod handlers;

pub(crate) use analyzer::{semantic_results_for_query, semantic_status};
pub use handlers::{
    bm25_symbol_search, find_symbol, flatten_symbols, get_complexity, get_project_structure,
    get_ranked_context, get_symbols_overview, refresh_symbol_index, search_symbols_fuzzy,
};

#[cfg(test)]
mod tests {
    use super::analyzer::{
        annotate_ranked_context_provenance, merge_semantic_ranked_entries,
        merge_sparse_ranked_entries,
    };
    use super::formatter::truncate_body_preview;
    use crate::symbol_corpus::SymbolDocument;
    use crate::symbol_retrieval::ScoredSymbol;
    use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};
    use serde_json::json;

    #[test]
    fn merge_semantic_ranked_entries_inserts_and_upgrades() {
        let mut result = RankedContextResult {
            query: "rename across project".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 128,
            symbols: vec![RankedContextEntry {
                name: "project_scope_renames_across_files".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-core/src/rename.rs".to_owned(),
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
                    file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 10,
                    signature: "fn project_scope_renames_across_files".to_owned(),
                    name_path: "project_scope_renames_across_files".to_owned(),
                    score: 0.41,
                },
                SemanticMatch {
                    symbol_name: "rename_symbol".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 42,
                    signature: "fn rename_symbol".to_owned(),
                    name_path: "rename_symbol".to_owned(),
                    score: 0.93,
                },
            ],
            8,
        );

        assert_eq!(result.symbols[0].name, "rename_symbol");
        assert!(result.symbols[0].relevance_score >= 80);
        assert!(result.symbols[0].relevance_score < 90);
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
            symbols: vec![RankedContextEntry {
                name: "change_signature".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-core/src/refactor.rs".to_owned(),
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
                    file_path: "crates/codelens-core/src/refactor.rs".to_owned(),
                    line: 44,
                    signature: "fn apply_signature_change".to_owned(),
                    name_path: "apply_signature_change".to_owned(),
                    score: 0.32,
                },
                SemanticMatch {
                    symbol_name: "rewrite_call_arguments".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-core/src/refactor.rs".to_owned(),
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
    fn semantic_only_entries_do_not_outrank_strong_structural_evidence() {
        let mut result = RankedContextResult {
            query: "route http request to mcp handler".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 64,
            symbols: vec![RankedContextEntry {
                name: "mcp_post_handler".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-mcp/src/server/transport_http.rs".to_owned(),
                line: 344,
                signature: "async fn mcp_post_handler".to_owned(),
                body: None,
                relevance_score: 91,
            }],
        };

        merge_semantic_ranked_entries(
            "route http request to mcp handler",
            &mut result,
            vec![SemanticMatch {
                symbol_name: "unrelated_route_helper".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-mcp/src/server/router.rs".to_owned(),
                line: 20,
                signature: "fn unrelated_route_helper".to_owned(),
                name_path: "unrelated_route_helper".to_owned(),
                score: 0.99,
            }],
            8,
        );

        assert_eq!(result.symbols[0].name, "mcp_post_handler");
        let semantic_only = result
            .symbols
            .iter()
            .find(|entry| entry.name == "unrelated_route_helper")
            .expect("semantic-only entry should still be visible as a hint");
        assert!(
            semantic_only.relevance_score < 90,
            "semantic-only hints should be capped below strong structural evidence"
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
            symbols: vec![
                RankedContextEntry {
                    name: "project_scope_renames_across_files".to_owned(),
                    kind: "function".to_owned(),
                    file: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 10,
                    signature: "fn project_scope_renames_across_files".to_owned(),
                    body: None,
                    relevance_score: 64,
                },
                RankedContextEntry {
                    name: "rename_symbol".to_owned(),
                    kind: "function".to_owned(),
                    file: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 42,
                    signature: "fn rename_symbol".to_owned(),
                    body: None,
                    relevance_score: 91,
                },
            ],
        };
        let structural_keys = std::collections::HashSet::from([format!(
            "{}:{}",
            "crates/codelens-core/src/rename.rs", "project_scope_renames_across_files"
        )]);
        let semantic_results = vec![
            SemanticMatch {
                symbol_name: "project_scope_renames_across_files".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                line: 10,
                signature: "fn project_scope_renames_across_files".to_owned(),
                name_path: "project_scope_renames_across_files".to_owned(),
                score: 0.411,
            },
            SemanticMatch {
                symbol_name: "rename_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                line: 42,
                signature: "fn rename_symbol".to_owned(),
                name_path: "rename_symbol".to_owned(),
                score: 0.933,
            },
        ];

        let mut payload = json!(result);
        annotate_ranked_context_provenance(&mut payload, &structural_keys, &semantic_results, &[]);

        let symbols = payload["symbols"].as_array().unwrap();
        assert_eq!(
            symbols[0]["provenance"]["source"],
            json!("semantic_boosted")
        );
        assert_eq!(symbols[1]["provenance"]["source"], json!("semantic_added"));
        assert_eq!(symbols[1]["provenance"]["semantic_score"], json!(0.933));
    }

    #[test]
    fn merge_sparse_ranked_entries_inserts_and_upgrades() {
        let mut result = RankedContextResult {
            query: "natural language retrieval".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 128,
            symbols: vec![RankedContextEntry {
                name: "semantic_query_for_embedding_search".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-mcp/src/tools/query_analysis/bridge.rs".to_owned(),
                line: 10,
                signature: "fn semantic_query_for_embedding_search".to_owned(),
                body: None,
                relevance_score: 44,
            }],
        };

        merge_sparse_ranked_entries(
            "improve natural language retrieval with bm25 and rerank",
            &mut result,
            vec![
                ScoredSymbol {
                    document: SymbolDocument {
                        symbol_id: "1".to_owned(),
                        name: "semantic_query_for_embedding_search".to_owned(),
                        name_path: "semantic_query_for_embedding_search".to_owned(),
                        kind: "function".to_owned(),
                        signature: "fn semantic_query_for_embedding_search".to_owned(),
                        file_path: "crates/codelens-mcp/src/tools/query_analysis/bridge.rs"
                            .to_owned(),
                        module_path: "tools::query_analysis::bridge".to_owned(),
                        doc_comment: String::new(),
                        body_lexical_chunk: String::new(),
                        language: "rust",
                        line_start: 10,
                        is_test: false,
                        is_generated: false,
                        exported: false,
                    },
                    score: 3.9,
                    matched_terms: vec!["retrieval".to_owned(), "rerank".to_owned()],
                },
                ScoredSymbol {
                    document: SymbolDocument {
                        symbol_id: "2".to_owned(),
                        name: "bm25_symbol_search".to_owned(),
                        name_path: "bm25_symbol_search".to_owned(),
                        kind: "function".to_owned(),
                        signature: "fn bm25_symbol_search".to_owned(),
                        file_path: "crates/codelens-mcp/src/tools/symbols/handlers.rs".to_owned(),
                        module_path: "tools::symbols::handlers".to_owned(),
                        doc_comment: String::new(),
                        body_lexical_chunk: String::new(),
                        language: "rust",
                        line_start: 172,
                        is_test: false,
                        is_generated: false,
                        exported: true,
                    },
                    score: 5.2,
                    matched_terms: vec!["bm25".to_owned(), "retrieval".to_owned()],
                },
            ],
            4,
        );

        assert_eq!(result.symbols[0].name, "bm25_symbol_search");
        assert!(
            result
                .symbols
                .iter()
                .find(|entry| entry.name == "semantic_query_for_embedding_search")
                .unwrap()
                .relevance_score
                > 44
        );
    }

    #[test]
    fn annotate_ranked_context_provenance_marks_sparse_entries() {
        let result = RankedContextResult {
            query: "bm25 retrieval".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 96,
            symbols: vec![RankedContextEntry {
                name: "bm25_symbol_search".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-mcp/src/tools/symbols/handlers.rs".to_owned(),
                line: 172,
                signature: "fn bm25_symbol_search".to_owned(),
                body: None,
                relevance_score: 82,
            }],
        };
        let structural_keys = std::collections::HashSet::new();
        let sparse_results = vec![ScoredSymbol {
            document: SymbolDocument {
                symbol_id: "2".to_owned(),
                name: "bm25_symbol_search".to_owned(),
                name_path: "bm25_symbol_search".to_owned(),
                kind: "function".to_owned(),
                signature: "fn bm25_symbol_search".to_owned(),
                file_path: "crates/codelens-mcp/src/tools/symbols/handlers.rs".to_owned(),
                module_path: "tools::symbols::handlers".to_owned(),
                doc_comment: String::new(),
                body_lexical_chunk: String::new(),
                language: "rust",
                line_start: 172,
                is_test: false,
                is_generated: false,
                exported: true,
            },
            score: 5.2,
            matched_terms: vec!["bm25".to_owned(), "retrieval".to_owned()],
        }];

        let mut payload = json!(result);
        annotate_ranked_context_provenance(&mut payload, &structural_keys, &[], &sparse_results);

        let symbols = payload["symbols"].as_array().unwrap();
        assert_eq!(symbols[0]["provenance"]["source"], json!("sparse_added"));
        assert_eq!(symbols[0]["provenance"]["sparse_score"], json!(5.2));
    }
}
