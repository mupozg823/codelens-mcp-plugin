use super::super::super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::{RetrievalQueryAnalysis, analyze_retrieval_query},
    required_string, success_meta,
};
use super::super::{
    analyzer::{
        annotate_ranked_context_provenance, compact_semantic_evidence, compact_sparse_evidence,
        merge_semantic_ranked_entries, merge_sparse_ranked_entries, semantic_results_for_query,
        semantic_scores_for_query,
    },
    formatter::{compact_symbol_bodies, count_branches},
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{ScoredSymbol, search_symbols_bm25f, unique_query_terms};
use codelens_engine::{SymbolInfo, SymbolKind, read_file, search_symbols_hybrid_with_semantic};
use serde_json::{Value, json};

use super::bm25::{adapt_budget_to_context_window, sparse_symbol_hits_for_query};
use super::path_args::{insert_response_annotations, resolve_path_argument};

pub fn get_ranked_context(state: &AppState, arguments: &Value) -> ToolResult {
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
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
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
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
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
    let evidence = crate::tool_evidence::tool_evidence(
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
