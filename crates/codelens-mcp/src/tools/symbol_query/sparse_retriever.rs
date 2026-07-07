//! Sparse retrieval helpers shared by ranked-context fusion,
//! `bm25_symbol_search`, and `get_complexity`.
//!
//! The dependency flows one way: `symbols::*` can call into these
//! retrieval primitives, while `symbol_query::*` does not import from
//! `symbols::*`.

use crate::AppState;
use crate::error::CodeLensError;
use crate::sparse_symbol_cache::{SparseSymbolCacheKey, SparseSymbolIndexFingerprint};
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{ScoredSymbol, SparseSymbolIndex, search_symbols_bm25f_index};
use crate::tools::query_analysis::RetrievalQueryAnalysis;
use codelens_engine::SymbolInfo;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

use super::retrieval_scope::{
    file_matches_scope, markup_config_penalty_multiplier, normalize_path_scope,
};

pub(crate) struct SparseRetrievalResult {
    pub(crate) hits: Vec<ScoredSymbol>,
    pub(crate) diagnostics: SparseRetrievalDiagnostics,
}

#[derive(Debug, Clone)]
pub(crate) struct SparseRetrievalDiagnostics {
    cache_hit: bool,
    indexed_files: usize,
    symbol_count: usize,
    max_indexed_at: Option<i64>,
    corpus_build_ms: u128,
    search_ms: u128,
    total_ms: u128,
}

impl SparseRetrievalDiagnostics {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "cache_hit": self.cache_hit,
            "indexed_files": self.indexed_files,
            "symbol_count": self.symbol_count,
            "max_indexed_at": self.max_indexed_at,
            "corpus_build_ms": self.corpus_build_ms,
            "search_ms": self.search_ms,
            "total_ms": self.total_ms,
        })
    }
}

pub(crate) fn sparse_symbol_hits_for_query_with_diagnostics(
    state: &AppState,
    query_analysis: &RetrievalQueryAnalysis,
    max_results: usize,
    include_tests: bool,
    include_generated: bool,
    session: &crate::session_context::SessionRequestContext,
    path_scope: Option<&str>,
) -> Result<SparseRetrievalResult, CodeLensError> {
    let total_start = Instant::now();
    let normalized_path_scope = normalize_path_scope(state.project().as_path(), path_scope);
    let symbol_index = state.symbol_index();
    let fingerprint = SparseSymbolIndexFingerprint::from_symbol_index(symbol_index.as_ref())?;
    let cache_key =
        SparseSymbolCacheKey::new(state.current_project_scope(), normalized_path_scope.clone());
    let cache = state.sparse_symbol_cache();
    let cache_lookup = cache.get(&cache_key, fingerprint);
    let cache_hit = cache_lookup.is_some();
    let build_start = Instant::now();
    let sparse_index = match cache_lookup {
        Some(index) => index,
        None => {
            let index = Arc::new(build_sparse_symbol_index(
                state,
                normalized_path_scope.as_deref(),
            )?);
            cache.store(cache_key, fingerprint, Arc::clone(&index));
            index
        }
    };
    let corpus_build_ms = if cache_hit {
        0
    } else {
        build_start.elapsed().as_millis()
    };

    let search_start = Instant::now();
    let mut scored = search_symbols_bm25f_index(
        &sparse_index,
        &query_analysis.expanded_query,
        max_results.saturating_mul(5).max(max_results),
        include_tests,
        include_generated,
    );

    let recent_files = state.recent_file_paths_for_session(session);
    for hit in &mut scored {
        hit.score *= markup_config_penalty_multiplier(
            &query_analysis.original_query,
            &hit.document.file_path,
        );
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
    scored.truncate(max_results);
    let search_ms = search_start.elapsed().as_millis();

    Ok(SparseRetrievalResult {
        hits: scored,
        diagnostics: SparseRetrievalDiagnostics {
            cache_hit,
            indexed_files: fingerprint.file_count(),
            symbol_count: sparse_index.len(),
            max_indexed_at: fingerprint.max_indexed_at(),
            corpus_build_ms,
            search_ms,
            total_ms: total_start.elapsed().as_millis(),
        },
    })
}

fn build_sparse_symbol_index(
    state: &AppState,
    normalized_path_scope: Option<&str>,
) -> Result<SparseSymbolIndex, CodeLensError> {
    let symbol_index = state.symbol_index();
    let mut all_symbols = Vec::new();
    for path in symbol_index.indexed_file_paths()? {
        if !file_matches_scope(&path, normalized_path_scope) {
            continue;
        }
        if let Ok(symbols) = symbol_index.get_symbols_overview_cached(&path, 3) {
            all_symbols.extend(flatten_symbols(&symbols));
        }
    }

    let corpus = build_symbol_corpus(&all_symbols);
    Ok(SparseSymbolIndex::new(corpus))
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
    use super::{adapt_budget_to_context_window, markup_config_penalty_multiplier};

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

    #[test]
    fn markup_config_penalty_keeps_explicit_markup_queries_unpenalized() {
        assert!(
            markup_config_penalty_multiplier(
                "block resolume advanced actions unless pro license feature enabled",
                "tuanbo-controller/src/popup.css"
            ) < 1.0
        );
        assert_eq!(
            markup_config_penalty_multiplier(
                "css popup advanced action styles",
                "tuanbo-controller/src/popup.css"
            ),
            1.0
        );
    }
}
