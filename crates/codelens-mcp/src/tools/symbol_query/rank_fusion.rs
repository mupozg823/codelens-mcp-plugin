//! Stage 4 of the ranked-context pipeline: fuse semantic + sparse
//! retrieval lanes back into the structural ranking.
//!
//! `RankFusionPolicy` keeps the per-query retrieval lane limits in one
//! place so weighted RRF doesn't have to re-derive them. Policies are
//! tuned by query word count today; future query-shape signals can
//! extend the match arms in `rank_fusion_policy` without touching the
//! fusion logic.
//!
//! Visibility: every export here is `pub(super)`. `ranked_context.rs`
//! is the only legitimate caller — these helpers don't make sense in
//! isolation from the pipeline's stage ordering.

use crate::symbol_retrieval::ScoredSymbol;
use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
pub(super) struct RankFusionPolicy {
    pub(super) semantic_limit: usize,
    pub(super) sparse_limit: usize,
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
            sparse_limit: max_sparse.min(4),
        };
    }
    if word_count >= 2 {
        return RankFusionPolicy {
            semantic_limit: max_semantic.min(2),
            sparse_limit: max_sparse.min(3),
        };
    }
    RankFusionPolicy {
        semantic_limit: max_semantic.min(3),
        sparse_limit: max_sparse.min(2),
    }
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

struct RankEntry {
    name: String,
    kind: String,
    file: String,
    line: usize,
    signature: String,
    structural_rank: Option<usize>,
    semantic_rank: Option<usize>,
    sparse_rank: Option<usize>,
    user_context_rank: Option<usize>,
}

pub(super) fn fuse_ranked_entries_weighted_rrf(
    query: &str,
    result: &mut RankedContextResult,
    semantic_results: Vec<SemanticMatch>,
    sparse_results: Vec<ScoredSymbol>,
    max_semantic_entries: usize,
    max_sparse_entries: usize,
    user_context_scores: Option<&std::collections::HashMap<String, f64>>,
) {
    let policy = rank_fusion_policy(query, max_semantic_entries, max_sparse_entries);
    let mut entries_map = std::collections::HashMap::new();

    // 1) Structural lane.
    for (idx, item) in result.symbols.iter().enumerate() {
        let key = format!("{}:{}", item.file, item.name);
        entries_map.insert(
            key,
            RankEntry {
                name: item.name.clone(),
                kind: item.kind.clone(),
                file: item.file.clone(),
                line: item.line,
                signature: item.signature.clone(),
                structural_rank: Some(idx + 1),
                semantic_rank: None,
                sparse_rank: None,
                user_context_rank: None,
            },
        );
    }

    // 2) Semantic lane, capped by policy.
    for (idx, item) in semantic_results
        .into_iter()
        .take(policy.semantic_limit)
        .enumerate()
    {
        let key = format!("{}:{}", item.file_path, item.symbol_name);
        if let Some(entry) = entries_map.get_mut(&key) {
            entry.semantic_rank = Some(idx + 1);
        } else {
            entries_map.insert(
                key,
                RankEntry {
                    name: item.symbol_name,
                    kind: item.kind,
                    file: item.file_path,
                    line: item.line,
                    signature: item.signature,
                    structural_rank: None,
                    semantic_rank: Some(idx + 1),
                    sparse_rank: None,
                    user_context_rank: None,
                },
            );
        }
    }

    // 3) Sparse lane, capped by policy.
    for (idx, item) in sparse_results
        .into_iter()
        .take(policy.sparse_limit)
        .enumerate()
    {
        let key = format!("{}:{}", item.document.file_path, item.document.name);
        if let Some(entry) = entries_map.get_mut(&key) {
            entry.sparse_rank = Some(idx + 1);
        } else {
            entries_map.insert(
                key,
                RankEntry {
                    name: item.document.name,
                    kind: item.document.kind,
                    file: item.document.file_path,
                    line: item.document.line_start,
                    signature: item.document.signature,
                    structural_rank: None,
                    semantic_rank: None,
                    sparse_rank: Some(idx + 1),
                    user_context_rank: None,
                },
            );
        }
    }

    // 4) User context lane.
    if let Some(uc_scores) = user_context_scores {
        let mut uc_sorted: Vec<(&String, &f64)> = uc_scores.iter().collect();
        uc_sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (idx, (file_key, _)) in uc_sorted.into_iter().enumerate().take(5) {
            for entry in entries_map.values_mut() {
                if &entry.file == file_key {
                    entry.user_context_rank = Some(idx + 1);
                }
            }
        }
    }

    // Weighted reciprocal-rank fusion.
    let k = 60.0;
    let w_struct = 1.0;
    let w_sem = 1.0;
    let w_sparse = 0.8;
    let w_user = 0.6;

    let mut scored_entries = Vec::new();
    for (_, entry) in entries_map {
        let mut rrf_score = 0.0;
        if let Some(r) = entry.structural_rank {
            rrf_score += w_struct / (k + r as f64);
        }
        if let Some(r) = entry.semantic_rank {
            rrf_score += w_sem / (k + r as f64);
        }
        if let Some(r) = entry.sparse_rank {
            rrf_score += w_sparse / (k + r as f64);
        }
        if let Some(r) = entry.user_context_rank {
            rrf_score += w_user / (k + r as f64);
        }
        scored_entries.push((entry, rrf_score));
    }

    // Sort by descending RRF score.
    scored_entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if scored_entries.is_empty() {
        result.symbols.clear();
        result.count = 0;
    } else if scored_entries.len() == 1 {
        let (entry, _) = scored_entries.remove(0);
        result.symbols = vec![RankedContextEntry {
            name: entry.name,
            kind: entry.kind,
            file: entry.file,
            line: entry.line,
            signature: entry.signature,
            body: None,
            relevance_score: 100,
        }];
        result.count = 1;
    } else {
        let max_rrf = scored_entries.first().map(|x| x.1).unwrap_or(0.0);
        let min_rrf = scored_entries.last().map(|x| x.1).unwrap_or(0.0);
        let diff = max_rrf - min_rrf;

        let mut final_symbols = Vec::new();
        for (entry, rrf_score) in scored_entries {
            let relevance_score = if diff > 1e-9 {
                let norm = (rrf_score - min_rrf) / diff;
                (norm * 99.0 + 1.0).round() as i32
            } else {
                100
            };

            final_symbols.push(RankedContextEntry {
                name: entry.name,
                kind: entry.kind,
                file: entry.file,
                line: entry.line,
                signature: entry.signature,
                body: None,
                relevance_score,
            });
        }

        result.symbols = final_symbols;
        result.count = result.symbols.len();
    }
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
