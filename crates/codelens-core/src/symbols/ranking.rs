use super::parser::slice_source;
use super::scoring::score_symbol_with_lower;
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
    /// Weights are auto-tuned based on query characteristics.
    pub fn with_pagerank_and_semantic(
        query: &str,
        pagerank: HashMap<String, f64>,
        semantic_scores: HashMap<String, f64>,
    ) -> Self {
        let weights = auto_weights(query);
        Self {
            pagerank,
            recent_files: HashMap::new(),
            semantic_scores,
            weights,
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

/// Determine weights based on query characteristics.
/// - Symbol-like queries (snake_case, CamelCase, short): text-heavy
/// - Natural language queries (spaces, long): semantic-heavy
fn auto_weights(query: &str) -> RankWeights {
    let words: Vec<&str> = query.split_whitespace().collect();
    let has_spaces = words.len() > 1;
    let has_underscore = query.contains('_');
    let is_camel = query.chars().any(|c| c.is_uppercase()) && !has_spaces;
    let is_short = query.len() <= 30;

    // Single identifier (prune_to_budget, BackendKind, dispatch_tool)
    if !has_spaces && (has_underscore || is_camel) && is_short {
        return RankWeights {
            text: 0.70,
            pagerank: 0.15,
            recency: 0.05,
            semantic: 0.10,
        };
    }

    // Natural language (how does file watcher invalidate graph cache)
    if has_spaces && words.len() >= 4 {
        return RankWeights {
            text: 0.35,
            pagerank: 0.10,
            recency: 0.05,
            semantic: 0.50,
        };
    }

    // Short phrase (dispatch tool, rename symbol)
    RankWeights {
        text: 0.45,
        pagerank: 0.10,
        recency: 0.05,
        semantic: 0.40,
    }
}

/// Score and rank a list of symbols against a query, using multiple signals.
/// Returns (symbol, blended_score) pairs sorted by score descending.
///
/// Symbols qualify if they have EITHER a text match OR a semantic match above
/// threshold. This ensures semantic-only discoveries aren't dropped.
pub(crate) fn rank_symbols(
    query: &str,
    symbols: Vec<SymbolInfo>,
    ctx: &RankingContext,
) -> Vec<(SymbolInfo, i32)> {
    let pr_count = ctx.pagerank.len().max(1) as f64;
    let has_semantic = !ctx.semantic_scores.is_empty();
    let query_lower = query.to_lowercase();

    // Normalize semantic scores to use the full 0-100 range.
    // Raw cosine similarity typically clusters in 0.3-0.85 — rescale so the
    // best match maps to ~100 and the threshold (0.2) maps to ~0.
    let sem_max = if has_semantic {
        ctx.semantic_scores
            .values()
            .copied()
            .fold(0.0f64, f64::max)
            .max(0.01) // avoid division by zero
    } else {
        1.0
    };

    // Reusable key buffer to avoid per-symbol format! allocation
    let mut sem_key_buf = String::with_capacity(128);

    let mut scored: Vec<(SymbolInfo, i32)> = symbols
        .into_iter()
        .filter_map(|symbol| {
            let text_score = score_symbol_with_lower(query, &query_lower, &symbol).unwrap_or(0);

            // Semantic: cosine similarity via reusable buffer (no format! alloc)
            let sem_score = if has_semantic {
                sem_key_buf.clear();
                sem_key_buf.push_str(&symbol.file_path);
                sem_key_buf.push(':');
                sem_key_buf.push_str(&symbol.name);
                ctx.semantic_scores
                    .get(sem_key_buf.as_str())
                    .copied()
                    .unwrap_or(0.0)
            } else {
                0.0
            };

            // Gate: include if text matched OR semantic score is significant
            if text_score == 0 && (!has_semantic || sem_score < 0.3) {
                return None;
            }

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

            // Semantic: normalize to 0-100 using max-relative scaling.
            // This stretches the typical 0.3-0.85 range to use the full 0-100 scale,
            // making semantic scores comparable to text scores (0-100).
            let sem_normalized = (sem_score / sem_max * 100.0).min(100.0);
            let semantic_component = sem_normalized * ctx.weights.semantic;

            let blended =
                (text_component + pr_component + recency_component + semantic_component) as i32;
            Some((symbol, blended.max(1)))
        })
        .collect();

    // Partial sort: only guarantee top-K ordering when result set is large.
    // prune_to_budget typically selects 20-50 entries, so K=100 is safe margin.
    const PARTIAL_SORT_K: usize = 100;
    if scored.len() > PARTIAL_SORT_K * 2 {
        scored.select_nth_unstable_by(PARTIAL_SORT_K, |a, b| b.1.cmp(&a.1));
        scored.truncate(PARTIAL_SORT_K);
        scored.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    } else {
        scored.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    }
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
    // Dynamic file cache limit: scale with token budget, cap at 128
    let file_cache_limit = (max_tokens / 200).clamp(32, 128);
    let char_budget = max_tokens.saturating_mul(4);
    let mut remaining = char_budget;
    let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut selected = Vec::new();

    for (symbol, score) in scored {
        let body = if include_body && symbol.end_byte > symbol.start_byte {
            let cache_full = file_cache.len() >= file_cache_limit;
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
