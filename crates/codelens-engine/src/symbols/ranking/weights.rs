use std::collections::HashMap;

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
    /// Weights are auto-tuned based on query characteristics and semantic signal richness.
    pub fn with_pagerank_and_semantic(
        query: &str,
        pagerank: HashMap<String, f64>,
        semantic_scores: HashMap<String, f64>,
    ) -> Self {
        let semantic_count = semantic_scores.len();
        let weights = auto_weights_with_semantic_count(query, semantic_count);
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

/// Determine weights based on query characteristics and available signals.
/// - Symbol-like queries (snake_case, CamelCase, short): text-heavy
/// - Natural language queries (spaces, long): semantic-heavy
/// - When semantic scores are available and rich: boost semantic weight
pub(crate) fn auto_weights_with_semantic_count(query: &str, semantic_count: usize) -> RankWeights {
    let words: Vec<&str> = query.split_whitespace().collect();
    let has_spaces = words.len() > 1;
    let has_underscore = query.contains('_');
    let is_camel = query.chars().any(|c| c.is_uppercase()) && !has_spaces;
    let is_short = query.len() <= 30;

    // Rich semantic signals available (embedding index active with matches)
    let has_rich_semantic = semantic_count >= 5;

    // Single identifier (prune_to_budget, BackendKind, dispatch_tool)
    if !has_spaces && (has_underscore || is_camel) && is_short {
        return RankWeights {
            text: 0.65,
            pagerank: 0.10,
            recency: 0.05,
            semantic: if has_rich_semantic { 0.20 } else { 0.10 },
        };
    }

    // Natural language (how does file watcher invalidate graph cache)
    if has_spaces && words.len() >= 4 {
        return if has_rich_semantic {
            RankWeights {
                text: 0.20,
                pagerank: 0.05,
                recency: 0.05,
                semantic: 0.70,
            }
        } else {
            RankWeights {
                text: 0.60,
                pagerank: 0.20,
                recency: 0.10,
                semantic: 0.10,
            }
        };
    }

    // Short phrase (dispatch tool, rename symbol, file watcher)
    // Keep lexical matching in front and use semantic as a targeted boost only.
    // Bundled CodeSearchNet embeddings help land the best top hit, but overly
    // semantic-heavy blending pushes weak semantic neighbors into the top-3/5.
    if has_rich_semantic {
        RankWeights {
            text: 0.50,
            pagerank: 0.10,
            recency: 0.10,
            semantic: 0.30,
        }
    } else {
        RankWeights {
            text: 0.60,
            pagerank: 0.15,
            recency: 0.10,
            semantic: 0.15,
        }
    }
}

pub(crate) fn is_natural_language_query(query_lower: &str) -> bool {
    query_lower.split_whitespace().count() >= 4
}

pub(crate) fn query_targets_entrypoint_impl(query_lower: &str) -> bool {
    query_lower.contains("entrypoint")
        || query_lower.contains(" handler")
        || query_lower.starts_with("handler ")
        || query_lower.contains("primary implementation")
}

pub(crate) fn query_targets_helper_impl(query_lower: &str) -> bool {
    query_lower.contains("helper") || query_lower.contains("internal helper")
}

pub(crate) fn query_targets_builder_impl(query_lower: &str) -> bool {
    query_lower.contains("builder")
        || query_lower.contains("build ")
        || query_lower.contains(" construction")
}

pub(crate) fn mentions_any(query_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| query_lower.contains(needle))
}

/// Returns ranking weights tuned for the detected query type.
pub fn weights_for_query_type(query_type: &str) -> RankWeights {
    match query_type {
        "identifier" => RankWeights {
            text: 0.70,
            pagerank: 0.15,
            recency: 0.05,
            semantic: 0.10,
        },
        "natural_language" => RankWeights {
            text: 0.25,
            pagerank: 0.15,
            recency: 0.15,
            semantic: 0.45,
        },
        "short_phrase" => RankWeights {
            text: 0.35,
            pagerank: 0.15,
            recency: 0.15,
            semantic: 0.35,
        },
        _ => RankWeights::default(),
    }
}
