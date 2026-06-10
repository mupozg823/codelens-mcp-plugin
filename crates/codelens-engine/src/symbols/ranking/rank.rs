use super::super::parser::slice_source;
use super::super::scoring::score_symbol_with_lower;
use super::super::types::{RankedContextEntry, SymbolInfo};
use super::priors::{file_path_prior, symbol_kind_prior};
use super::weights::RankingContext;
use std::collections::HashMap;
use std::path::Path;

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
    // Raw cosine similarity for the bundled CodeSearchNet model typically
    // clusters much lower than classic sentence embeddings, often around
    // 0.08-0.35 for useful matches. Rescale the observed max to ~100.
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

    // Pre-compute the snake_case form of the query once — `joined_snake`
    // is used by score_symbol_with_lower for identifier matching (e.g.
    // "rename symbol" → "rename_symbol"). It is query-derived and
    // identical for every candidate, so hoisting it here eliminates one
    // String allocation per candidate in the hot loop.
    let joined_snake = query_lower.replace(|c: char| c.is_whitespace() || c == '-', "_");

    let mut scored: Vec<(SymbolInfo, i32)> = symbols
        .into_iter()
        .filter_map(|symbol| {
            let text_score =
                score_symbol_with_lower(query, &query_lower, &joined_snake, &symbol).unwrap_or(0);

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
            if text_score == 0 && (!has_semantic || sem_score < 0.08) {
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

            let blended = (text_component
                + pr_component
                + recency_component
                + semantic_component
                + symbol_kind_prior(&query_lower, &symbol)
                + file_path_prior(&query_lower, &symbol.file_path))
                as i32;
            Some((symbol, blended.max(1)))
        })
        .collect();

    // Partial sort: only guarantee top-K ordering when result set is large.
    // prune_to_budget typically selects 20-50 entries, so K=100 is safe margin.
    const PARTIAL_SORT_K: usize = 100;
    if scored.len() > PARTIAL_SORT_K * 2 {
        scored.select_nth_unstable_by(PARTIAL_SORT_K, |a, b| b.1.cmp(&a.1));
        scored.truncate(PARTIAL_SORT_K);
        scored.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
    } else {
        scored.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
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
        // Estimate entry size from field lengths directly instead of
        // serializing to JSON and measuring the string. This avoids one
        // full serde_json::to_string round-trip per selected entry
        // (~50 entries × ~300 bytes each = ~15 KB of wasted JSON work).
        // The constant 80 covers JSON keys, braces, commas, and the
        // integer relevance_score field. This is a budget-stopping
        // heuristic, not an exact measurement — a ±20% error is fine.
        let entry_size = entry.name.len()
            + entry.kind.len()
            + entry.file.len()
            + entry.signature.len()
            + entry.body.as_ref().map(|b| b.len()).unwrap_or(0)
            + 80;
        if remaining < entry_size && !selected.is_empty() {
            break;
        }
        remaining = remaining.saturating_sub(entry_size);
        selected.push(entry);
    }

    let chars_used = char_budget.saturating_sub(remaining);
    (selected, chars_used)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::super::priors::symbol_kind_prior;
    use super::super::weights::auto_weights_with_semantic_count;
    use crate::{SymbolInfo, SymbolKind, SymbolProvenance};

    #[test]
    fn short_phrase_prefers_text_over_semantic_even_with_rich_signal() {
        let weights = auto_weights_with_semantic_count("change function parameters", 8);
        assert!(weights.text > weights.semantic);
        assert_eq!(weights.text, 0.50);
        assert_eq!(weights.semantic, 0.30);
    }

    #[test]
    fn natural_language_kind_prior_prefers_functions_over_types() {
        let function_symbol = SymbolInfo {
            name: "dispatch_tool".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-mcp/src/dispatch.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "dispatch_tool".into(),
            id: "id".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let type_symbol = SymbolInfo {
            name: "ToolHandler".into(),
            kind: SymbolKind::Class,
            file_path: "crates/codelens-mcp/src/tools/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "ToolHandler".into(),
            id: "id2".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "route an incoming tool request to the right handler";
        assert!(
            symbol_kind_prior(query, &function_symbol) > symbol_kind_prior(query, &type_symbol)
        );
    }

    #[test]
    fn short_entrypoint_phrase_prefers_functions_over_edit_types() {
        let function_symbol = SymbolInfo {
            name: "move_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/move_symbol.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "move_symbol".into(),
            id: "fn".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let type_symbol = SymbolInfo {
            name: "MoveEdit".into(),
            kind: SymbolKind::TypeAlias,
            file_path: "crates/codelens-engine/src/move_symbol.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "MoveEdit".into(),
            id: "type".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "primary move handler";
        assert!(
            symbol_kind_prior(query, &function_symbol) > symbol_kind_prior(query, &type_symbol)
        );
    }

    #[test]
    fn inline_target_beats_generic_entrypoint_helpers() {
        let inline_symbol = SymbolInfo {
            name: "inline_function".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/inline.rs".into(),
            line: 22,
            column: 1,
            signature: String::new(),
            name_path: "inline_function".into(),
            id: "inline".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let helper_symbol = SymbolInfo {
            name: "is_entry_point_file".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/import_graph/dead_code.rs".into(),
            line: 22,
            column: 1,
            signature: String::new(),
            name_path: "is_entry_point_file".into(),
            id: "entry".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which entrypoint handles inline";
        assert!(
            symbol_kind_prior(query, &inline_symbol) > symbol_kind_prior(query, &helper_symbol)
        );
    }

    #[test]
    fn find_symbol_target_beats_generic_finders() {
        let target = SymbolInfo {
            name: "find_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/symbols/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_symbol".into(),
            id: "find_symbol".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "find_files".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/file_ops/reader.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_files".into(),
            id: "find_files".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which helper implements find";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn embedding_text_target_beats_generic_embedding_symbols() {
        let target = SymbolInfo {
            name: "build_embedding_text".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_embedding_text".into(),
            id: "build_embedding_text".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "EmbeddingEngine".into(),
            kind: SymbolKind::Class,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "EmbeddingEngine".into(),
            id: "EmbeddingEngine".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which builder creates build embedding text";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn embedding_text_target_beats_other_build_helpers() {
        let target = SymbolInfo {
            name: "build_embedding_text".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_embedding_text".into(),
            id: "build_embedding_text".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "build_coreml_execution_provider".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_coreml_execution_provider".into(),
            id: "build_coreml_execution_provider".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which builder creates build embedding text";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn embedding_text_target_beats_embed_texts_cached() {
        let target = SymbolInfo {
            name: "build_embedding_text".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_embedding_text".into(),
            id: "build_embedding_text".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "embed_texts_cached".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "embed_texts_cached".into(),
            id: "embed_texts_cached".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which builder creates build embedding text";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn exact_word_match_target_beats_generic_find() {
        let exact = SymbolInfo {
            name: "find_all_word_matches".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/rename.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_all_word_matches".into(),
            id: "find_all_word_matches".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "find_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/symbols/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_symbol".into(),
            id: "find_symbol".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which helper implements find all word matches";
        assert!(symbol_kind_prior(query, &exact) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn file_scoped_word_match_target_beats_broader_helper() {
        let exact = SymbolInfo {
            name: "find_word_matches_in_files".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/rename.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_word_matches_in_files".into(),
            id: "find_word_matches_in_files".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let broader = SymbolInfo {
            name: "find_all_word_matches".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/rename.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_all_word_matches".into(),
            id: "find_all_word_matches".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which helper implements find word matches in files";
        assert!(symbol_kind_prior(query, &exact) > symbol_kind_prior(query, &broader));
    }
}
