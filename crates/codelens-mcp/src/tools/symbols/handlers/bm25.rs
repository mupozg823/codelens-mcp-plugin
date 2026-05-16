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

use super::confidence::confidence_tier;
use super::follow_up::suggested_follow_up;
use super::housekeeping::flatten_symbols;
use super::path_args::{insert_response_annotations, resolve_path_argument};

pub(super) fn sparse_symbol_hits_for_query(
    state: &AppState,
    query_analysis: &RetrievalQueryAnalysis,
    max_results: usize,
    include_tests: bool,
    include_generated: bool,
    session: &crate::session_context::SessionRequestContext,
) -> Result<Vec<ScoredSymbol>, CodeLensError> {
    let mut all_symbols = Vec::new();
    for path in state.symbol_index().indexed_file_paths()? {
        if let Ok(symbols) = state.symbol_index().get_symbols_overview_cached(&path, 3) {
            all_symbols.extend(flatten_symbols(&symbols));
        }
    }

    let corpus = build_symbol_corpus(&all_symbols);
    let mut scored = search_symbols_bm25f(
        &corpus,
        &query_analysis.expanded_query,
        max_results.saturating_mul(3).max(max_results),
        include_tests,
        include_generated,
    );

    let recent_files = state.recent_file_paths_for_session(session);
    if !recent_files.is_empty() {
        for hit in &mut scored {
            if recent_files
                .iter()
                .any(|path| hit.document.file_path.starts_with(path))
            {
                hit.score *= 1.08;
            }
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    scored.truncate(max_results);
    Ok(scored)
}

pub fn bm25_symbol_search(state: &AppState, arguments: &Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let query_analysis = analyze_retrieval_query(query);
    let max_results = optional_usize(arguments, "max_results", 10);
    let include_tests = optional_bool(arguments, "include_tests", false);
    let include_generated = optional_bool(arguments, "include_generated", false);
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let scored = sparse_symbol_hits_for_query(
        state,
        &query_analysis,
        max_results,
        include_tests,
        include_generated,
        &session,
    )?;

    let total_query_terms = unique_query_terms(&query_analysis.expanded_query).len();
    let payload_results: Vec<Value> = scored
        .into_iter()
        .enumerate()
        .map(|(idx, hit)| {
            let follow_up = suggested_follow_up(&hit.document.kind, hit.document.exported);
            let confidence = confidence_tier(
                &hit.matched_terms,
                total_query_terms,
                &hit.document.name,
                &hit.document.name_path,
            );
            json!({
                "symbol_id": hit.document.symbol_id,
                "name": hit.document.name,
                "name_path": hit.document.name_path,
                "kind": hit.document.kind,
                "file_path": hit.document.file_path,
                "module_path": hit.document.module_path,
                "signature": hit.document.signature,
                "language": hit.document.language,
                "line": hit.document.line_start,
                "score": ((hit.score * 1000.0).round() / 1000.0),
                "why_matched": hit.matched_terms,
                "flags": {
                    "is_test": hit.document.is_test,
                    "is_generated": hit.document.is_generated,
                    "exported": hit.document.exported,
                },
                "provenance": {
                    "source": "sparse_bm25f",
                    "retrieval_rank": idx + 1,
                },
                "suggested_follow_up": follow_up,
                "confidence": confidence,
            })
        })
        .collect();

    let query_type = if query_analysis.prefer_lexical_only {
        "identifier"
    } else if query_analysis.natural_language {
        "natural_language"
    } else {
        "short_phrase"
    };
    let retrieval = json!({
        "lane": "sparse_bm25f",
        "query_type": query_type,
        "recommended": query_analysis.prefer_sparse_symbol_search,
        "lexical_query": query_analysis.expanded_query,
        "semantic_query": query_analysis.semantic_query,
    });
    let meta = success_meta(BackendKind::Sqlite, 0.88);
    let evidence = crate::tool_evidence::tool_evidence(
        "retrieval",
        &meta,
        "sparse_bm25f",
        json!({
            "preferred_lane": "sparse_bm25f",
            "query_type": query_type,
            "semantic_enabled": false,
            "semantic_used_in_core": false,
            "sparse_used_in_core": true,
            "semantic_evidence_count": 0,
            "sparse_evidence_count": payload_results.len(),
            "precise_available": false,
            "precise_used": false,
            "precise_source": null,
            "fallback_source": "sparse_bm25f",
            "precise_result_count": 0,
        }),
    );

    Ok((
        json!({
            "query": query,
            "results": payload_results,
            "count": payload_results.len(),
            "retrieval": retrieval,
            "evidence": evidence,
        }),
        meta,
    ))
}

/// Scale a base token budget to the host's advertised model context window.
///
/// Returns the smaller of (base × multiplier) and a per-tier ceiling so a
/// 1M-context host doesn't end up with a budget larger than reasonably
/// retrievable evidence, while a 32K host doesn't get pushed over its head.
///
/// Tiers are conservative on purpose. The intent is to widen room when there
/// is room, not to fill the host's window — the host still owns the response
/// and may apply its own truncation downstream.
pub(crate) fn adapt_budget_to_context_window(base: usize, context_window: usize) -> usize {
    let (multiplier, cap) = match context_window {
        n if n >= 1_000_000 => (4.0_f64, 131_072_usize),
        n if n >= 200_000 => (2.0_f64, 65_536_usize),
        n if n >= 32_000 => (1.0_f64, 32_768_usize),
        _ => (0.5_f64, 16_384_usize),
    };
    ((base as f64 * multiplier).round() as usize).min(cap)
}
