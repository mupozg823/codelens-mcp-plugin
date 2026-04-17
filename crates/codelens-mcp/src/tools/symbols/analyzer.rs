use super::super::AppState;
#[cfg(feature = "semantic")]
use super::super::query_analysis::analyze_retrieval_query;
use codelens_engine::{RankedContextResult, SemanticMatch};
use serde_json::{Value, json};

#[cfg(feature = "semantic")]
use super::super::query_analysis::semantic_query_for_embedding_search;

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
pub(crate) fn semantic_status(state: &AppState) -> Value {
    let configured_model = codelens_engine::configured_embedding_model_name();
    let indexed = codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
        .ok()
        .flatten();

    match indexed {
        Some(info) => json!({
            "status": "not_compiled",
            "model": info.model_name,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
            "reason": "semantic feature not compiled into this binary",
        }),
        None => json!({
            "status": "not_compiled",
            "model": configured_model,
            "loaded": false,
            "reason": "semantic feature not compiled into this binary",
        }),
    }
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
        return crate::tools::query_analysis::rerank_semantic_matches(
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

pub(super) fn semantic_scores_for_query(
    state: &AppState,
    query: &str,
    limit: usize,
    disable_semantic: bool,
) -> std::collections::HashMap<String, f64> {
    let mut scores = std::collections::HashMap::new();
    for r in semantic_results_for_query(state, query, limit, disable_semantic) {
        if r.score > 0.05 {
            let key = format!("{}:{}", r.file_path, r.symbol_name);
            scores.insert(key, r.score);
        }
    }
    scores
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

    let query_word_count = query.split_whitespace().count();
    let is_short_phrase = (2..4).contains(&query_word_count);
    let effective_limit = if query_word_count >= 4 {
        max_semantic_entries.min(6)
    } else if query_word_count >= 2 {
        max_semantic_entries.min(2)
    } else {
        max_semantic_entries.min(3)
    };
    let semantic_max = semantic_results
        .iter()
        .map(|sem| sem.score)
        .fold(0.0_f64, f64::max)
        .max(0.05);
    let insertion_floor = if query_word_count >= 4 {
        0.10
    } else if is_short_phrase {
        0.18
    } else {
        0.12
    };

    for (rank_idx, sem) in semantic_results
        .into_iter()
        .take(effective_limit)
        .enumerate()
    {
        if sem.score < 0.05 {
            continue;
        }
        let key = format!("{}:{}", sem.file_path, sem.symbol_name);
        let normalized_semantic = ((sem.score / semantic_max) * 100.0).clamp(1.0, 100.0) as i32;
        let semantic_score = (normalized_semantic - (rank_idx as i32 * 8)).clamp(1, 100);
        if let Some(idx) = index_by_key.get(&key).copied() {
            result.symbols[idx].relevance_score =
                result.symbols[idx].relevance_score.max(semantic_score);
            continue;
        }
        if sem.score < insertion_floor {
            continue;
        }
        if is_short_phrase && rank_idx > 0 {
            continue;
        }

        let idx = result.symbols.len();
        result.symbols.push(codelens_engine::RankedContextEntry {
            name: sem.symbol_name,
            kind: sem.kind,
            file: sem.file_path,
            line: sem.line,
            signature: sem.signature,
            body: None,
            relevance_score: semantic_score,
        });
        index_by_key.insert(key, idx);
    }

    result
        .symbols
        .sort_unstable_by(|a, b| b.relevance_score.cmp(&a.relevance_score));
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

pub(super) fn annotate_ranked_context_provenance(
    payload: &mut Value,
    structural_keys: &std::collections::HashSet<String>,
    semantic_results: &[SemanticMatch],
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
        let structural_candidate = structural_keys.contains(&key);
        let source = match (semantic_score, structural_candidate) {
            (Some(_), true) => "semantic_boosted",
            (Some(_), false) => "semantic_added",
            (None, _) => "structural",
        };
        map.insert(
            "provenance".to_owned(),
            json!({
                "source": source,
                "structural_candidate": structural_candidate,
                "semantic_score": semantic_score,
            }),
        );
    }
}
