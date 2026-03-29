use super::parser::slice_source;
use super::scoring::score_symbol;
use super::types::{RankedContextEntry, SymbolInfo};
use std::collections::HashMap;
use std::path::Path;

/// Weights for blending multiple relevance signals.
pub(crate) struct RankWeights {
    pub text: f64,
    pub pagerank: f64,
    pub recency: f64,
    pub semantic: f64,
}

impl Default for RankWeights {
    fn default() -> Self {
        Self {
            text: 0.55,
            pagerank: 0.15,
            recency: 0.10,
            semantic: 0.20,
        }
    }
}

/// Context for ranking: external signals that augment text relevance.
pub(crate) struct RankingContext {
    /// PageRank scores by file path (0.0..1.0 range, unscaled).
    pub pagerank: HashMap<String, f64>,
    /// Recently changed files get a boost.
    pub recent_files: HashMap<String, f64>,
    /// Semantic similarity scores by "file_path:symbol_name" key.
    pub semantic_scores: HashMap<String, f64>,
    /// Blending weights.
    pub weights: RankWeights,
}

impl RankingContext {
    /// Create a ranking context with PageRank scores only.
    pub fn with_pagerank(pagerank: HashMap<String, f64>) -> Self {
        Self {
            pagerank,
            recent_files: HashMap::new(),
            semantic_scores: HashMap::new(),
            weights: RankWeights {
                text: 0.70,
                pagerank: 0.20,
                recency: 0.10,
                semantic: 0.0,
            },
        }
    }

    /// Create a ranking context with PageRank + semantic scores.
    #[allow(dead_code)]
    pub fn with_pagerank_and_semantic(
        pagerank: HashMap<String, f64>,
        semantic_scores: HashMap<String, f64>,
    ) -> Self {
        Self {
            pagerank,
            recent_files: HashMap::new(),
            semantic_scores,
            weights: RankWeights::default(),
        }
    }

    /// Create an empty context (text-only ranking).
    pub fn text_only() -> Self {
        Self {
            pagerank: HashMap::new(),
            recent_files: HashMap::new(),
            semantic_scores: HashMap::new(),
            weights: RankWeights {
                text: 1.0,
                pagerank: 0.0,
                recency: 0.0,
                semantic: 0.0,
            },
        }
    }
}

/// Score and rank a list of symbols against a query, using multiple signals.
/// Returns (symbol, blended_score) pairs sorted by score descending.
pub(crate) fn rank_symbols(
    query: &str,
    symbols: Vec<SymbolInfo>,
    ctx: &RankingContext,
) -> Vec<(SymbolInfo, i32)> {
    let pr_count = ctx.pagerank.len().max(1) as f64;

    let mut scored: Vec<(SymbolInfo, i32)> = symbols
        .into_iter()
        .filter_map(|symbol| {
            score_symbol(query, &symbol).map(|text_score| {
                let text_component = text_score as f64 * ctx.weights.text;

                // PageRank: scale raw score to 0-100 range
                let pr = ctx.pagerank.get(&symbol.file_path).copied().unwrap_or(0.0);
                let pr_scaled = (pr * 100.0 * pr_count).min(100.0);
                let pr_component = pr_scaled * ctx.weights.pagerank;

                // Recency: boost for recently changed files
                let recency = ctx
                    .recent_files
                    .get(&symbol.file_path)
                    .copied()
                    .unwrap_or(0.0);
                let recency_component = (recency * 100.0).min(100.0) * ctx.weights.recency;

                // Semantic: cosine similarity from vector search (0.0..1.0)
                let sem_key = format!("{}:{}", symbol.file_path, symbol.name);
                let sem_score = ctx.semantic_scores.get(&sem_key).copied().unwrap_or(0.0);
                let semantic_component = (sem_score * 100.0) * ctx.weights.semantic;

                let blended =
                    (text_component + pr_component + recency_component + semantic_component) as i32;
                (symbol, blended.max(1))
            })
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored
}

/// Budget-aware pruning: take ranked symbols, extract bodies, stop when budget exhausted.
/// Returns (selected_entries, chars_used).
pub(crate) fn prune_to_budget(
    scored: Vec<(SymbolInfo, i32)>,
    max_tokens: usize,
    include_body: bool,
    project_root: &Path,
) -> (Vec<RankedContextEntry>, usize) {
    const FILE_CACHE_LIMIT: usize = 32;
    let char_budget = max_tokens.saturating_mul(4);
    let mut remaining = char_budget;
    let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut selected = Vec::new();

    for (symbol, score) in scored {
        let body = if include_body && symbol.end_byte > symbol.start_byte {
            let cache_full = file_cache.len() >= FILE_CACHE_LIMIT;
            let source = file_cache
                .entry(symbol.file_path.clone())
                .or_insert_with(|| {
                    if cache_full {
                        return None;
                    }
                    let abs = project_root.join(&symbol.file_path);
                    std::fs::read_to_string(&abs).ok()
                });
            source
                .as_deref()
                .map(|s| slice_source(s, symbol.start_byte, symbol.end_byte))
        } else {
            None
        };

        let entry = RankedContextEntry {
            name: symbol.name,
            kind: symbol.kind.as_label().to_owned(),
            file: symbol.file_path,
            line: symbol.line,
            signature: symbol.signature,
            body,
            relevance_score: score,
        };
        // serde_json::to_string should not fail for this struct, but handle gracefully
        let entry_size = serde_json::to_string(&entry).map(|s| s.len()).unwrap_or(0);
        if remaining < entry_size && !selected.is_empty() {
            break;
        }
        remaining = remaining.saturating_sub(entry_size);
        selected.push(entry);
    }

    let chars_used = char_budget.saturating_sub(remaining);
    (selected, chars_used)
}
