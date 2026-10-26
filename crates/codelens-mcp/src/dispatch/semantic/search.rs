use crate::{
    AppState,
    error::CodeLensError,
    protocol::BackendKind,
    tools::{self, ToolResult},
};
use serde_json::json;

pub(in crate::dispatch) fn semantic_search_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let query = tools::required_string(arguments, "query")?;
    const SEMANTIC_SEARCH_KNOWN_ARGS: &[&str] =
        &["query", "max_results", "limit", "top_k", "path_hint"];
    let max_results = crate::tool_runtime::optional_usize_with_aliases(
        arguments,
        "max_results",
        &["limit", "top_k"],
        20,
    );
    let path_hint = tools::optional_string(arguments, "path_hint");
    let unknown_args =
        crate::tool_runtime::collect_unknown_args(arguments, SEMANTIC_SEARCH_KNOWN_ARGS);

    let project = state.project();
    let normalized_path_hint = crate::tools::symbol_query::retrieval_scope::normalize_path_scope(
        project.as_path(),
        path_hint,
    );
    let guard = state.embedding_engine();
    let engine = guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Embedding engine not available. Build with --features semantic")
    })?;

    if !engine.is_indexed() {
        return Err(CodeLensError::FeatureUnavailable(
            "Embedding index is empty. Call index_embeddings first to build the semantic index."
                .into(),
        ));
    }

    let query_analysis = crate::tools::query_analysis::analyze_retrieval_query(query);
    let candidate_limit = if normalized_path_hint.is_some() {
        max_results.saturating_mul(8).clamp(max_results, 200)
    } else {
        max_results.saturating_mul(4).clamp(max_results, 80)
    };
    // Both lanes used to fail silently (`unwrap_or_default`), so a failed
    // lane looked like "no matches". A failing scoped embedding query left
    // serde-json's `path_hint: "src"` searches ranked by lexical scores alone;
    // say which lane failed when it happens.
    let (mut lexical_candidates, lexical_lane_error) =
        match codelens_engine::search::search_symbols_hybrid(
            &project,
            &query_analysis.expanded_query,
            candidate_limit,
            0.7,
        ) {
            Ok(candidates) => (candidates, None),
            Err(error) => {
                let message = format!("{error:#}");
                tracing::warn!(error = %message, "semantic_search lexical lane failed");
                (Vec::new(), Some(message))
            }
        };
    lexical_candidates.retain(|result| {
        crate::tools::symbol_query::retrieval_scope::file_matches_scope(
            &result.file,
            normalized_path_hint.as_deref(),
        )
    });
    let structural_names: std::collections::HashSet<String> = lexical_candidates
        .iter()
        .map(|result| format!("{}:{}", result.file, result.name))
        .collect();
    let semantic_lane = crate::tools::semantic_retriever::semantic_lane_for_query(
        state,
        query,
        candidate_limit,
        false,
        normalized_path_hint.as_deref(),
    );
    let semantic_lane_error = semantic_lane.error;
    let mut results = semantic_lane.results;

    // Which lane produced each result, keyed like `seen` below, so the
    // provenance survives the rerank instead of calling every hit semantic.
    let mut sources: std::collections::HashMap<String, &'static str> =
        std::collections::HashMap::new();
    for result in &mut results {
        let key = format!("{}:{}", result.file_path, result.symbol_name);
        let source = if structural_names.contains(&key) {
            result.score += 0.06;
            "semantic_lexical"
        } else {
            "semantic"
        };
        sources.insert(result_key(result), source);
    }

    let top_semantic = results.iter().map(|r| r.score).fold(0.0_f64, f64::max);
    let mut seen: std::collections::HashSet<String> = results.iter().map(result_key).collect();
    for hr in lexical_candidates {
        let key = format!("{}:{}:{}", hr.file, hr.name, hr.line);
        if seen.insert(key.clone()) {
            sources.insert(key, "lexical");
            results.push(codelens_engine::SemanticMatch {
                file_path: hr.file,
                symbol_name: hr.name,
                kind: hr.kind,
                line: hr.line,
                signature: hr.signature,
                name_path: hr.name_path,
                score: lexical_only_score(hr.score, top_semantic),
            });
        }
    }

    results.retain(|r| project.as_path().join(&r.file_path).exists());
    results = crate::tools::query_analysis::rerank_semantic_matches(query, results, max_results);

    let result_scores = results
        .iter()
        .map(|result| {
            let (prior_delta, adjusted_score) =
                crate::tools::query_analysis::semantic_adjusted_score_parts(query, result);
            ResultProvenance {
                source: sources
                    .get(&result_key(result))
                    .copied()
                    .unwrap_or("semantic"),
                prior_delta: (prior_delta * 1000.0).round() / 1000.0,
                adjusted_score: (adjusted_score * 1000.0).round() / 1000.0,
            }
        })
        .collect::<Vec<_>>();
    let mut payload = json!({
        "query": query,
        "results": results,
        "count": results.len(),
        "retrieval": {
            "semantic_enabled": true,
            "requested_query": query,
            "semantic_query": query_analysis.semantic_query,
            "path_hint": normalized_path_hint,
        }
    });
    annotate_provenance(&mut payload, &result_scores);
    add_unknown_args_hint(&mut payload, &unknown_args, SEMANTIC_SEARCH_KNOWN_ARGS);
    let lane_state = |error: &Option<String>| if error.is_some() { "failed" } else { "ok" };
    payload["retrieval"]["semantic_lane"] = json!(lane_state(&semantic_lane_error));
    payload["retrieval"]["lexical_lane"] = json!(lane_state(&lexical_lane_error));
    let mut failures = Vec::new();
    if let Some(error) = semantic_lane_error {
        failures.push(format!(
            "embedding lane failed; results are lexical-only: {error}"
        ));
    }
    if let Some(error) = lexical_lane_error {
        failures.push(format!(
            "lexical lane failed; results are embedding-only: {error}"
        ));
    }
    if !failures.is_empty() {
        let reason = failures.join("; ");
        payload["degraded_reason"] = json!(reason);
        return Ok((
            payload,
            crate::tool_runtime::degraded_meta(BackendKind::Semantic, 0.6, &reason),
        ));
    }
    Ok((payload, tools::success_meta(BackendKind::Semantic, 0.85)))
}

/// A perfect lexical-only hit is worth this share of the query's best
/// embedding hit. Cosine scores run near 0.05 on one repo and 0.5 on
/// another, so the old fixed 0.35 scale let one-term matches bury every
/// semantic hit wherever the model scores low. Self-retrieval (112 queries):
/// MRR 0.688 -> 0.707, recall unchanged; 0.5 measured 0.697.
const LEXICAL_ONLY_SHARE: f64 = 0.8;
/// Scale used when the embedding lane has no positive score to anchor to.
const LEXICAL_ONLY_FALLBACK_SCALE: f64 = 0.35;

/// `lexical_score` is the 0-100 hybrid search score.
fn lexical_only_score(lexical_score: f64, top_semantic: f64) -> f64 {
    let scale = if top_semantic > 0.0 {
        LEXICAL_ONLY_SHARE * top_semantic
    } else {
        LEXICAL_ONLY_FALLBACK_SCALE
    };
    (lexical_score / 100.0) * scale
}

fn result_key(result: &codelens_engine::SemanticMatch) -> String {
    format!(
        "{}:{}:{}",
        result.file_path, result.symbol_name, result.line
    )
}

/// `source`: `semantic` (embedding lane only), `lexical` (lexical lane only)
/// or `semantic_lexical` (both lanes returned the symbol).
#[derive(Clone, Copy)]
struct ResultProvenance {
    source: &'static str,
    prior_delta: f64,
    adjusted_score: f64,
}

fn annotate_provenance(payload: &mut serde_json::Value, scores: &[ResultProvenance]) {
    let Some(entries) = payload
        .get_mut("results")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (idx, entry) in entries.iter_mut().enumerate() {
        if let Some(map) = entry.as_object_mut() {
            let provenance = scores.get(idx).copied().unwrap_or(ResultProvenance {
                source: "semantic",
                prior_delta: 0.0,
                adjusted_score: 0.0,
            });
            map.insert(
                "provenance".to_owned(),
                json!({
                    "source": provenance.source,
                    "retrieval_rank": idx + 1,
                    "prior_delta": provenance.prior_delta,
                    "adjusted_score": provenance.adjusted_score,
                }),
            );
        }
    }
}

fn add_unknown_args_hint(
    payload: &mut serde_json::Value,
    unknown_args: &[String],
    known_args: &[&str],
) {
    if unknown_args.is_empty() {
        return;
    }
    if let Some(map) = payload.as_object_mut() {
        map.insert("unknown_args".to_owned(), json!(unknown_args));
        map.insert(
            "unknown_args_hint".to_owned(),
            json!(format!(
                "ignored unknown argument(s): {}. valid args: {}",
                unknown_args.join(", "),
                known_args.join(", ")
            )),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::lexical_only_score;

    #[test]
    fn lexical_only_hits_scale_with_the_best_semantic_hit() {
        // The same best FTS hit (80) stays below the top embedding hit
        // whether the model scores this repo near 0.07 or near 0.5.
        for top_semantic in [0.07, 0.5] {
            let score = lexical_only_score(80.0, top_semantic);
            assert!(score < top_semantic, "{score} vs {top_semantic}");
            assert!((score - 0.64 * top_semantic).abs() < 1e-12);
        }
        assert!(lexical_only_score(80.0, 0.3) > lexical_only_score(40.0, 0.3));
    }

    #[test]
    fn lexical_only_hits_keep_their_order_without_a_semantic_anchor() {
        // A failed or empty embedding lane must not flatten lexical hits to 0.
        for top_semantic in [0.0, -0.03] {
            assert!((lexical_only_score(100.0, top_semantic) - 0.35).abs() < 1e-12);
            assert!(
                lexical_only_score(80.0, top_semantic) > lexical_only_score(40.0, top_semantic)
            );
        }
    }
}
