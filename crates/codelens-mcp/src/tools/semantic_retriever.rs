//! Semantic retrieval seam — the deep module behind every tool that
//! asks the embedding index for symbol matches.
//!
//! Two responsibilities live here:
//!   - `semantic_status` reports whether the embedding engine is
//!     loaded, indexed, and aligned with the configured model.
//!   - `semantic_results_for_query` runs a single retrieval pass
//!     (analyse + embed + rerank) and returns the top-N matches.
//!
//! Both functions are consumed by the symbol-query path inside
//! `tools/symbols/` and by the impact-report family in
//! `tools/reports/impact_reports/`, so they cannot live inside
//! either module. Pulling them up to `tools::semantic_retriever`
//! establishes the seam where future callers compose semantic
//! retrieval without re-exporting a symbol-shaped facade.
//!
//! Behaviour is feature-gated on `semantic`; when the feature is
//! disabled both entry points degrade gracefully (status reports
//! `not_compiled`, retrieval returns an empty `Vec`). The handler
//! signature stays stable so callers do not branch on the cfg flag.

use super::AppState;
use codelens_engine::SemanticMatch;
use serde_json::{Value, json};

#[cfg(feature = "semantic")]
use super::query_analysis::{analyze_retrieval_query, semantic_query_for_embedding_search};

#[cfg(feature = "semantic")]
pub(crate) fn semantic_status(state: &AppState) -> Value {
    let configured_model = codelens_engine::configured_embedding_model_name();
    let guard = state.embedding_ref();
    if let Some(engine) = guard.as_ref() {
        let info = engine.index_info();
        return if info.indexed_symbols > 0 {
            json!({
                "status": "ready",
                "model": info.model_name,
                "indexed_symbols": info.indexed_symbols,
                "loaded": true,
            })
        } else {
            json!({
                "status": "unavailable",
                "model": info.model_name,
                "indexed_symbols": info.indexed_symbols,
                "loaded": true,
                "reason": "embedding index is empty; call index_embeddings",
            })
        };
    }
    drop(guard);

    match codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
        .ok()
        .flatten()
    {
        Some(info) if info.model_name == configured_model && info.indexed_symbols > 0 => json!({
            "status": "ready",
            "model": info.model_name,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
        }),
        Some(info) if info.model_name != configured_model => json!({
            "status": "unavailable",
            "model": info.model_name,
            "expected_model": configured_model,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
            "reason": "embedding index model mismatch; call index_embeddings to rebuild",
        }),
        Some(info) => json!({
            "status": "unavailable",
            "model": info.model_name,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
            "reason": "embedding index is empty; call index_embeddings",
        }),
        None => json!({
            "status": "unavailable",
            "model": configured_model,
            "loaded": false,
            "reason": "embedding index missing; call index_embeddings",
        }),
    }
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn semantic_status(_state: &AppState) -> Value {
    json!({
        "status": "not_compiled",
        "model": "disabled",
        "indexed_symbols": 0,
        "loaded": false,
        "reason": "semantic feature not compiled into this binary",
    })
}

#[cfg(feature = "semantic")]
pub(crate) fn semantic_results_for_query(
    state: &AppState,
    query: &str,
    limit: usize,
    disable_semantic: bool,
) -> Vec<SemanticMatch> {
    if disable_semantic {
        return Vec::new();
    }

    let query_analysis = analyze_retrieval_query(query);

    // Skip embedding lookup for short single-word identifiers where FTS is more accurate
    if query_analysis.prefer_lexical_only && query_analysis.original_query.len() <= 40 {
        return Vec::new();
    }

    if query_analysis.semantic_query.is_empty() {
        return Vec::new();
    }

    let guard = state.embedding_engine();
    if let Some(engine) = guard.as_ref()
        && engine.is_indexed()
    {
        let candidate_limit = limit.saturating_mul(4).clamp(limit, 80);
        let search_query =
            semantic_query_for_embedding_search(&query_analysis, Some(state.project().as_path()));
        let results = engine
            .search(&search_query, candidate_limit)
            .unwrap_or_default();
        return super::query_analysis::rerank_semantic_matches(
            &query_analysis.semantic_query,
            results,
            limit,
        );
    }
    Vec::new()
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn semantic_results_for_query(
    _state: &AppState,
    _query: &str,
    _limit: usize,
    _disable_semantic: bool,
) -> Vec<SemanticMatch> {
    Vec::new()
}
