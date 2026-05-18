//! Sparse retrieval helpers shared between the symbol-query pipeline
//! and the legacy `bm25_symbol_search` / `get_complexity` handlers.
//!
//! Moved here from `tools/symbols/handlers.rs` to break the
//! `symbol_query ↔ symbols` cycle reported by `review_architecture`:
//! the pipeline used to import these helpers upward from
//! `handlers.rs`, while `handlers.rs` re-entered the pipeline through
//! its 3-line tool entries. The dependency now flows one way —
//! `symbols::handlers` → `symbol_query::sparse_retriever` — so the
//! pipeline owns its retrieval primitives.

use crate::AppState;
use crate::error::CodeLensError;
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{ScoredSymbol, search_symbols_bm25f};
use crate::tools::query_analysis::RetrievalQueryAnalysis;
use codelens_engine::SymbolInfo;

pub(crate) fn sparse_symbol_hits_for_query(
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

/// Scale a base token budget to the host's advertised model context window.
///
/// Returns the smaller of (base × multiplier) and a per-tier ceiling so a
/// 1M-context host doesn't end up with a budget larger than reasonably
/// retrievable evidence, while a 32K host doesn't get pushed over its head.
pub(crate) fn adapt_budget_to_context_window(base: usize, context_window: usize) -> usize {
    let (multiplier, cap) = match context_window {
        n if n >= 1_000_000 => (4.0_f64, 131_072_usize),
        n if n >= 200_000 => (2.0_f64, 65_536_usize),
        n if n >= 32_000 => (1.0_f64, 32_768_usize),
        _ => (0.5_f64, 16_384_usize),
    };
    ((base as f64 * multiplier).round() as usize).min(cap)
}

pub fn flatten_symbols(symbols: &[SymbolInfo]) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    let mut stack = symbols.to_vec();
    while let Some(mut symbol) = stack.pop() {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        stack.extend(children);
    }
    flat
}

#[cfg(test)]
mod adapt_budget_tests {
    use super::adapt_budget_to_context_window;

    #[test]
    fn small_window_halves_budget_capped_at_16k() {
        assert_eq!(adapt_budget_to_context_window(32_768, 8_000), 16_384);
        assert_eq!(adapt_budget_to_context_window(8_000, 16_000), 4_000);
    }

    #[test]
    fn standard_window_passes_base_capped_at_32k() {
        assert_eq!(adapt_budget_to_context_window(16_384, 64_000), 16_384);
        assert_eq!(adapt_budget_to_context_window(40_000, 64_000), 32_768);
    }

    #[test]
    fn large_window_doubles_budget_capped_at_64k() {
        assert_eq!(adapt_budget_to_context_window(16_384, 200_000), 32_768);
        assert_eq!(adapt_budget_to_context_window(50_000, 200_000), 65_536);
    }

    #[test]
    fn xl_window_quadruples_budget_capped_at_128k() {
        assert_eq!(adapt_budget_to_context_window(16_384, 1_000_000), 65_536);
        assert_eq!(adapt_budget_to_context_window(40_000, 1_000_000), 131_072);
    }

    #[test]
    fn boundary_at_32k_uses_standard_tier() {
        assert_eq!(adapt_budget_to_context_window(16_384, 32_000), 16_384);
    }

    #[test]
    fn boundary_at_200k_uses_large_tier() {
        assert_eq!(adapt_budget_to_context_window(16_384, 200_000), 32_768);
    }
}
