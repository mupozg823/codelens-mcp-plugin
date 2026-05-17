//! Stage 4 of the ranked-context pipeline: fuse semantic + sparse
//! retrieval lanes back into the structural ranking.
//!
//! `RankFusionPolicy` keeps the per-query thresholds (insertion floor,
//! per-lane score cap, max merged entries) in one place so the
//! merge functions don't have to re-derive them. Policies are tuned
//! by query word count today; future query-shape signals can extend
//! the match arms in `rank_fusion_policy` without touching the
//! merge logic.
//!
//! Visibility: every export here is `pub(super)`. `ranked_context.rs`
//! is the only legitimate caller — these helpers don't make sense in
//! isolation from the pipeline's stage ordering.

use crate::symbol_corpus::SymbolDocument as _SymbolDocument;
use crate::symbol_retrieval::ScoredSymbol;
use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};
use serde_json::{Value, json};

// Used in test fixtures (`mod tests` in ranked_context.rs).
#[allow(unused_imports)]
use _SymbolDocument as SymbolDocument;

#[derive(Debug, Clone, Copy)]
pub(super) struct RankFusionPolicy {
    pub(super) semantic_limit: usize,
    pub(super) semantic_insertion_floor: f64,
    pub(super) semantic_added_score_cap: i32,
    pub(super) semantic_boosted_score_cap: i32,
    pub(super) sparse_limit: usize,
    pub(super) sparse_insertion_floor: i32,
}

pub(super) fn rank_fusion_policy(
    query: &str,
    max_semantic: usize,
    max_sparse: usize,
) -> RankFusionPolicy {
    let word_count = query.split_whitespace().count();
    if word_count >= 4 {
        return RankFusionPolicy {
            semantic_limit: max_semantic.min(6),
            semantic_insertion_floor: 0.10,
            semantic_added_score_cap: 86,
            semantic_boosted_score_cap: 96,
            sparse_limit: max_sparse.min(4),
            sparse_insertion_floor: 28,
        };
    }
    if word_count >= 2 {
        return RankFusionPolicy {
            semantic_limit: max_semantic.min(2),
            semantic_insertion_floor: 0.18,
            semantic_added_score_cap: 82,
            semantic_boosted_score_cap: 92,
            sparse_limit: max_sparse.min(3),
            sparse_insertion_floor: 35,
        };
    }
    RankFusionPolicy {
        semantic_limit: max_semantic.min(3),
        semantic_insertion_floor: 0.12,
        semantic_added_score_cap: 80,
        semantic_boosted_score_cap: 90,
        sparse_limit: max_sparse.min(2),
        sparse_insertion_floor: 35,
    }
}

pub(super) fn merge_semantic_ranked_entries(
    query: &str,
    result: &mut RankedContextResult,
    semantic_results: Vec<SemanticMatch>,
    max_semantic_entries: usize,
) {
    if semantic_results.is_empty() {
        return;
    }

    let mut index_by_key = std::collections::HashMap::new();
    for (idx, entry) in result.symbols.iter().enumerate() {
        index_by_key.insert(format!("{}:{}", entry.file, entry.name), idx);
    }

    let policy = rank_fusion_policy(query, max_semantic_entries, 0);
    let query_word_count = query.split_whitespace().count();
    let is_short_phrase = (2..4).contains(&query_word_count);
    let semantic_max = semantic_results
        .iter()
        .map(|sem| sem.score)
        .fold(0.0_f64, f64::max)
        .max(0.05);

    for (rank_idx, sem) in semantic_results
        .into_iter()
        .take(policy.semantic_limit)
        .enumerate()
    {
        if sem.score < 0.05 {
            continue;
        }
        let key = format!("{}:{}", sem.file_path, sem.symbol_name);
        let normalized_semantic = ((sem.score / semantic_max) * 100.0).clamp(1.0, 100.0) as i32;
        let semantic_score = (normalized_semantic - (rank_idx as i32 * 8)).clamp(1, 100);
        if let Some(idx) = index_by_key.get(&key).copied() {
            let semantic_score = semantic_score.min(policy.semantic_boosted_score_cap);
            result.symbols[idx].relevance_score =
                result.symbols[idx].relevance_score.max(semantic_score);
            continue;
        }
        if sem.score < policy.semantic_insertion_floor {
            continue;
        }
        if is_short_phrase && rank_idx > 0 {
            continue;
        }

        let idx = result.symbols.len();
        result.symbols.push(RankedContextEntry {
            name: sem.symbol_name,
            kind: sem.kind,
            file: sem.file_path,
            line: sem.line,
            signature: sem.signature,
            body: None,
            relevance_score: semantic_score.min(policy.semantic_added_score_cap),
        });
        index_by_key.insert(key, idx);
    }

    result
        .symbols
        .sort_unstable_by_key(|b| std::cmp::Reverse(b.relevance_score));
    result.count = result.symbols.len();
}

pub(super) fn compact_semantic_evidence(
    result: &RankedContextResult,
    semantic_results: &[SemanticMatch],
    limit: usize,
) -> Vec<Value> {
    let mut final_ranks = std::collections::HashMap::new();
    for (idx, entry) in result.symbols.iter().enumerate() {
        final_ranks.insert(format!("{}:{}", entry.file, entry.name), idx + 1);
    }

    semantic_results
        .iter()
        .take(limit)
        .map(|item| {
            let key = format!("{}:{}", item.file_path, item.symbol_name);
            let final_rank = final_ranks.get(&key).copied();
            json!({
                "symbol": item.symbol_name,
                "file": item.file_path,
                "score": (item.score * 1000.0).round() / 1000.0,
                "selected": final_rank.is_some(),
                "final_rank": final_rank,
            })
        })
        .collect()
}

pub(super) fn merge_sparse_ranked_entries(
    query: &str,
    result: &mut RankedContextResult,
    sparse_results: Vec<ScoredSymbol>,
    max_sparse_entries: usize,
) {
    if sparse_results.is_empty() {
        return;
    }

    let mut index_by_key = std::collections::HashMap::new();
    for (idx, entry) in result.symbols.iter().enumerate() {
        index_by_key.insert(format!("{}:{}", entry.file, entry.name), idx);
    }

    let policy = rank_fusion_policy(query, 0, max_sparse_entries);
    let query_word_count = query.split_whitespace().count();
    let sparse_max = sparse_results
        .iter()
        .map(|hit| hit.score)
        .fold(0.0_f64, f64::max)
        .max(0.01);

    for (rank_idx, hit) in sparse_results
        .into_iter()
        .take(policy.sparse_limit)
        .enumerate()
    {
        let key = format!("{}:{}", hit.document.file_path, hit.document.name);
        let normalized_sparse = ((hit.score / sparse_max) * 100.0).clamp(1.0, 100.0) as i32;
        let sparse_score = (normalized_sparse - (rank_idx as i32 * 6)).clamp(1, 100);
        if let Some(idx) = index_by_key.get(&key).copied() {
            result.symbols[idx].relevance_score =
                result.symbols[idx].relevance_score.max(sparse_score);
            continue;
        }
        if sparse_score < policy.sparse_insertion_floor {
            continue;
        }
        if query_word_count < 3 && rank_idx > 0 {
            continue;
        }

        let idx = result.symbols.len();
        result.symbols.push(RankedContextEntry {
            name: hit.document.name,
            kind: hit.document.kind,
            file: hit.document.file_path,
            line: hit.document.line_start,
            signature: hit.document.signature,
            body: None,
            relevance_score: sparse_score,
        });
        index_by_key.insert(key, idx);
    }

    result
        .symbols
        .sort_unstable_by_key(|b| std::cmp::Reverse(b.relevance_score));
    result.count = result.symbols.len();
}

pub(super) fn compact_sparse_evidence(
    result: &RankedContextResult,
    sparse_results: &[ScoredSymbol],
    limit: usize,
) -> Vec<Value> {
    let mut final_ranks = std::collections::HashMap::new();
    for (idx, entry) in result.symbols.iter().enumerate() {
        final_ranks.insert(format!("{}:{}", entry.file, entry.name), idx + 1);
    }

    sparse_results
        .iter()
        .take(limit)
        .map(|item| {
            let key = format!("{}:{}", item.document.file_path, item.document.name);
            let final_rank = final_ranks.get(&key).copied();
            json!({
                "symbol": item.document.name,
                "file": item.document.file_path,
                "score": (item.score * 1000.0).round() / 1000.0,
                "matched_terms": item.matched_terms,
                "selected": final_rank.is_some(),
                "final_rank": final_rank,
            })
        })
        .collect()
}

pub(super) fn annotate_ranked_context_provenance(
    payload: &mut Value,
    structural_keys: &std::collections::HashSet<String>,
    semantic_results: &[SemanticMatch],
    sparse_results: &[ScoredSymbol],
) {
    let semantic_scores = semantic_results
        .iter()
        .map(|item| {
            (
                format!("{}:{}", item.file_path, item.symbol_name),
                (item.score * 1000.0).round() / 1000.0,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let sparse_scores = sparse_results
        .iter()
        .map(|item| {
            (
                format!("{}:{}", item.document.file_path, item.document.name),
                (item.score * 1000.0).round() / 1000.0,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();

    let Some(symbols) = payload.get_mut("symbols").and_then(Value::as_array_mut) else {
        return;
    };

    for entry in symbols {
        let Some(map) = entry.as_object_mut() else {
            continue;
        };
        let Some(file) = map.get("file").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = map.get("name").and_then(Value::as_str) else {
            continue;
        };

        let key = format!("{file}:{name}");
        let semantic_score = semantic_scores.get(&key).copied();
        let sparse_score = sparse_scores.get(&key).copied();
        let structural_candidate = structural_keys.contains(&key);
        let source = match (semantic_score, sparse_score, structural_candidate) {
            (Some(_), _, true) => "semantic_boosted",
            (Some(_), _, false) => "semantic_added",
            (None, Some(_), true) => "sparse_boosted",
            (None, Some(_), false) => "sparse_added",
            (None, None, _) => "structural",
        };
        let confidence = match source {
            "semantic_added" => "medium",
            "sparse_added" => "medium_high",
            "semantic_boosted" | "sparse_boosted" => "high",
            _ => "medium",
        };
        map.insert(
            "provenance".to_owned(),
            json!({
                "source": source,
                "confidence": confidence,
                "corroborated": structural_candidate && (semantic_score.is_some() || sparse_score.is_some()),
                "structural_candidate": structural_candidate,
                "semantic_score": semantic_score,
                "sparse_score": sparse_score,
            }),
        );
    }
}
