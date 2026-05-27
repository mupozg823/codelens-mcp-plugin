//! `search_symbols_fuzzy` — hybrid fuzzy symbol search with optional
//! semantic boost. Distinct from the BM25 lane and ranked-context
//! fusion: this lane accepts a `fuzzy_threshold` parameter and routes
//! through `search_symbols_hybrid_with_semantic` in `codelens_engine`.
//! Kept as its own seam so callers can pin the fuzzy behaviour.

use super::super::{
    AppState, ToolResult, optional_bool, optional_usize, required_string, success_meta,
};
use super::analyzer::semantic_scores_for_query;
use crate::protocol::BackendKind;
use codelens_engine::search_symbols_hybrid_with_semantic;
use serde_json::{Value, json};

pub fn search_symbols_fuzzy(state: &AppState, arguments: &Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let max_results = optional_usize(arguments, "max_results", 30);
    let fuzzy_threshold = arguments
        .get("fuzzy_threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.6);
    let disable_semantic = optional_bool(arguments, "disable_semantic", false);
    // Build semantic scores if embeddings are available (same pattern as get_ranked_context)
    let semantic_scores = semantic_scores_for_query(state, query, 50, disable_semantic);

    let sem_ref = if semantic_scores.is_empty() {
        None
    } else {
        Some(&semantic_scores)
    };

    let backend = if sem_ref.is_some() {
        BackendKind::Hybrid
    } else {
        BackendKind::Sqlite
    };

    let pagerank_scores = state.graph_cache().file_pagerank_scores(&state.project());
    let pagerank_ref = if pagerank_scores.is_empty() {
        None
    } else {
        Some(pagerank_scores.as_ref())
    };

    Ok(search_symbols_hybrid_with_semantic(
        &state.project(),
        query,
        max_results,
        fuzzy_threshold,
        sem_ref,
        pagerank_ref,
    )
    .map(|value| {
        (
            json!({ "results": value, "count": value.len() }),
            success_meta(backend, 0.9),
        )
    })?)
}
