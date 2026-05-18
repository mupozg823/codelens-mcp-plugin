//! `get_ranked_context` — the symbol-query path that fuses
//! structural (tree-sitter / FTS) ranking with semantic + sparse
//! retrieval and emits a single ranked-context envelope.
//!
//! Stages, in order:
//!   1. **query analysis** (`tools::query_analysis::analyze_retrieval_query`)
//!      classifies the query as identifier / short_phrase /
//!      natural_language and produces an expanded retrieval form.
//!   2. **semantic retrieval** (`tools::semantic_retriever`) when the
//!      query is not pure-identifier; sparse retrieval (BM25F via
//!      `sparse_symbol_hits_for_query`) when natural-language tokens
//!      cover the query.
//!   3. **structural fetch** (`SymbolIndex::get_ranked_context_cached`)
//!      builds the base ranking with semantic scores as a soft prior.
//!   4. **rank fusion** (this file: `merge_semantic_ranked_entries`,
//!      `merge_sparse_ranked_entries`) folds the retrieval lanes back
//!      into the structural ranking with score caps that prevent
//!      semantic-only entries from outranking strong structural
//!      evidence.
//!   5. **payload shaping**
//!      (`compact_semantic_evidence` / `compact_sparse_evidence` /
//!      `annotate_ranked_context_provenance`) attaches per-symbol
//!      provenance and a compact evidence list, then emits the
//!      tool-evidence envelope.
//!
//! Visibility: only `run_ranked_context` is exposed. All stage
//! helpers and the rank-fusion policy stay private here. PR-A moved
//! the cross-cutting `semantic_retriever` out; PR-B (this PR) moves
//! the rest of the ranked-context stages into a single deep module.

use super::rank_fusion::{
    annotate_ranked_context_provenance, compact_semantic_evidence, compact_sparse_evidence,
    merge_semantic_ranked_entries, merge_sparse_ranked_entries,
};
use super::sparse_retriever::{adapt_budget_to_context_window, sparse_symbol_hits_for_query};
use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::session_context::SessionRequestContext;
use crate::tool_evidence;
use crate::tool_runtime::{
    ToolResult, collect_unknown_args, optional_bool, optional_string, optional_usize,
    required_string, success_meta,
};
use crate::tools::query_analysis::analyze_retrieval_query;
use crate::tools::semantic_retriever::semantic_results_for_query;
use serde_json::{Value, json};

pub(crate) fn run_ranked_context(state: &AppState, arguments: &Value) -> ToolResult {
    // P1-B — surface unknown_args. No `limit`/`top_k` alias here:
    // get_ranked_context's relevant control is `depth` (graph
    // expansion), not a top-N. See docs/design/arg-validation-policy.md.
    const KNOWN_ARGS: &[&str] = &[
        "query",
        "path",
        "file_path",
        "max_tokens",
        "context_window",
        "include_body",
        "depth",
        "disable_semantic",
        "expand_query",
        "session_id",
        "logical_session_id",
        "harness_phase",
        "lsp_boost",
    ];
    let query = required_string(arguments, "query")?;
    let query_analysis = analyze_retrieval_query(query);
    let path = optional_string(arguments, "path");
    let session = SessionRequestContext::from_json(arguments);
    // v1.10.1 floor: when the user does not supply `max_tokens`, take the
    // larger of the surface token budget and 16K. The active surface budget
    // is intentionally tight (8K on `preset:full`, 4K on
    // `refactor-full`), but hybrid retrieval (semantic + sparse +
    // structural evidence) routinely exceeds that, triggering Stage 5
    // truncation. See `docs/eval/v1.10.0-post-release-eval.md` (F3).
    const HYBRID_RETRIEVAL_FLOOR: usize = 16384;
    // v1.13.18 adaptive: when the host advertises its model context window
    // (e.g. 1M for Opus 4.7, 200K for Sonnet 4.6, 32K for older models),
    // scale the budget so we don't waste headroom on huge contexts and
    // don't blow up small ones. See `adapt_budget_to_context_window`.
    let context_window = arguments
        .get("context_window")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let max_tokens = arguments
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or_else(|| {
            let base = state
                .execution_token_budget(&session)
                .max(HYBRID_RETRIEVAL_FLOOR);
            match context_window {
                Some(window) => adapt_budget_to_context_window(base, window),
                None => base,
            }
        });
    let include_body = optional_bool(arguments, "include_body", false);
    let depth = optional_usize(arguments, "depth", 2);
    let disable_semantic = optional_bool(arguments, "disable_semantic", false);
    // v1.10.1: opt-out of n-gram query expansion. The default behaviour
    // (expand_query=true) preserves prior recall on partial-identifier
    // queries; setting expand_query=false disables snake_case /
    // camelCase / cartesian-token expansion for natural-language
    // queries that don't benefit from it.
    let expand_query = optional_bool(arguments, "expand_query", true);
    let unknown_args = collect_unknown_args(arguments, KNOWN_ARGS);
    let exact_identifier_projection = query_analysis.original_query
        != query_analysis.expanded_query
        && !query_analysis.expanded_query.contains(char::is_whitespace);
    let effective_disable_semantic =
        disable_semantic || query_analysis.prefer_lexical_only || exact_identifier_projection;
    let use_semantic_in_core = !effective_disable_semantic;
    let use_sparse_in_core = query_analysis.natural_language
        || (query_analysis.prefer_sparse_symbol_search
            && query_analysis.original_query.contains(char::is_whitespace));
    // Build semantic scores for hybrid ranking if embeddings are available.
    // The default model is the bundled CodeSearchNet MiniLM-L12 INT8 variant.
    let semantic_results = semantic_results_for_query(state, query, 50, effective_disable_semantic);
    let sparse_results = if use_sparse_in_core {
        sparse_symbol_hits_for_query(state, &query_analysis, 10, false, false, &session)?
    } else {
        Vec::new()
    };
    let semantic_scores = semantic_results
        .iter()
        .filter(|r| r.score > 0.05)
        .map(|r| (format!("{}:{}", r.file_path, r.symbol_name), r.score))
        .collect();

    // Boost scores for files recently accessed in this session
    let recent_files = state.recent_file_paths_for_session(&session);
    let mut boosted_scores: std::collections::HashMap<String, f64> = if use_semantic_in_core {
        semantic_scores
    } else {
        std::collections::HashMap::new()
    };
    if !recent_files.is_empty() {
        let boost = 0.15_f64;
        for (key, score) in boosted_scores.iter_mut() {
            if recent_files.iter().any(|f| key.starts_with(f.as_str())) {
                *score += boost;
            }
        }
    }

    // v1.10.1: when `expand_query=false`, use the user's literal query
    // for retrieval. The default keeps the n-gram expansion path so
    // partial-identifier queries still match across snake_case /
    // camelCase boundaries. See `docs/eval/v1.10.0-post-release-eval.md`
    // (F3).
    let retrieval_query: &str = if expand_query {
        &query_analysis.expanded_query
    } else {
        &query_analysis.original_query
    };

    // query-type-aware weights available via get_ranked_context_cached_with_query_type
    // but current dataset shows default weights are near-optimal (0.680 MRR).
    // Kept as None until per-type weight tuning yields measurable improvement.
    let mut result = state.symbol_index().get_ranked_context_cached(
        retrieval_query,
        path,
        max_tokens,
        include_body,
        depth,
        Some(&state.graph_cache()),
        boosted_scores,
    )?;
    let structural_keys = result
        .symbols
        .iter()
        .map(|entry| format!("{}:{}", entry.file, entry.name))
        .collect::<std::collections::HashSet<_>>();

    if !effective_disable_semantic {
        merge_semantic_ranked_entries(query, &mut result, semantic_results.clone(), 8);
    }
    if use_sparse_in_core {
        merge_sparse_ranked_entries(query, &mut result, sparse_results.clone(), 6);
    }

    // v1.5 Phase 2e: sparse term coverage bonus — post-process
    // re-ordering pass. Runs on the ORIGINAL user `query`, not the
    // MCP-expanded retrieval string, because the expansion adds dozens
    // of derivative tokens (snake_case, CamelCase, alias groups) that
    // dilute the coverage ratio below any reasonable threshold — the
    // 4-arm pilot that measured zero effect used the expanded query
    // and confirmed this dilution. Running the pass here (after
    // `get_ranked_context_cached` + `merge_semantic_ranked_entries`)
    // also keeps the engine layer free of query-semantics knowledge —
    // the engine ranks, the MCP layer decides what "the query" means.
    if codelens_engine::sparse_weighting_enabled() {
        let query_lower_for_sparse = query.to_lowercase();
        let mut changed = false;
        for entry in result.symbols.iter_mut() {
            let bonus = codelens_engine::sparse_coverage_bonus_from_fields(
                &query_lower_for_sparse,
                &entry.name,
                &entry.name, // no name_path on RankedContextEntry; reuse name
                &entry.signature,
                &entry.file,
            );
            if bonus > 0.0 {
                entry.relevance_score = entry.relevance_score.saturating_add(bonus as i32);
                changed = true;
            }
        }
        if changed {
            result
                .symbols
                .sort_unstable_by_key(|b| std::cmp::Reverse(b.relevance_score));
        }
    }

    let semantic_evidence = if effective_disable_semantic {
        Vec::new()
    } else {
        compact_semantic_evidence(&result, &semantic_results, 5)
    };
    let sparse_evidence = if use_sparse_in_core {
        compact_sparse_evidence(&result, &sparse_results, 5)
    } else {
        Vec::new()
    };
    let mut payload =
        serde_json::to_value(&result).map_err(|e| CodeLensError::Internal(e.into()))?;
    annotate_ranked_context_provenance(
        &mut payload,
        &structural_keys,
        &semantic_results,
        &sparse_results,
    );
    let preferred_lane = if use_sparse_in_core && !effective_disable_semantic {
        "hybrid_semantic_sparse"
    } else if use_sparse_in_core {
        "sparse_bm25f"
    } else if effective_disable_semantic {
        "structural_lexical"
    } else {
        "hybrid_semantic"
    };
    let query_type = if query_analysis.prefer_lexical_only {
        "identifier"
    } else if query_analysis.natural_language {
        "natural_language"
    } else {
        "short_phrase"
    };
    let retrieval = json!({
        "semantic_enabled": !effective_disable_semantic,
        "semantic_used_in_core": use_semantic_in_core,
        "sparse_used_in_core": use_sparse_in_core,
        "preferred_lane": preferred_lane,
        "sparse_lane_recommended": query_analysis.prefer_sparse_symbol_search,
        "query_type": query_type,
        "lexical_query": query_analysis.expanded_query,
        "semantic_query": query_analysis.semantic_query,
    });
    let backend = if result.symbols.iter().any(|s| s.relevance_score > 0) {
        BackendKind::TreeSitter
    } else {
        BackendKind::Semantic
    };
    let meta = success_meta(backend, 0.91);
    let evidence = tool_evidence::tool_evidence(
        "retrieval",
        &meta,
        preferred_lane,
        json!({
            "preferred_lane": preferred_lane,
            "query_type": query_type,
            "semantic_enabled": !effective_disable_semantic,
            "semantic_used_in_core": use_semantic_in_core,
            "sparse_used_in_core": use_sparse_in_core,
            "semantic_evidence_count": semantic_evidence.len(),
            "sparse_evidence_count": sparse_evidence.len(),
            "precise_available": false,
            "precise_used": false,
            "precise_source": null,
            "fallback_source": preferred_lane,
            "precise_result_count": 0,
        }),
    );
    if let Some(map) = payload.as_object_mut() {
        map.insert("retrieval".to_owned(), retrieval);
        if !semantic_evidence.is_empty() {
            map.insert("semantic_evidence".to_owned(), json!(semantic_evidence));
        }
        if !sparse_evidence.is_empty() {
            map.insert("sparse_evidence".to_owned(), json!(sparse_evidence));
        }
        map.insert("evidence".to_owned(), evidence);
        if !unknown_args.is_empty() {
            map.insert("unknown_args".to_owned(), json!(unknown_args));
        }
    }

    Ok((payload, meta))
}

// Stage 4 helpers moved to `super::rank_fusion` in PR-H. This file
// owns stage orchestration only.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_corpus::SymbolDocument;
    use crate::symbol_retrieval::ScoredSymbol;
    use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};

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
