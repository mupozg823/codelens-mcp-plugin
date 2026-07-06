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
    let mut lexical_candidates = codelens_engine::search::search_symbols_hybrid(
        &project,
        &query_analysis.expanded_query,
        candidate_limit,
        0.7,
    )
    .unwrap_or_default();
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
    let mut results = crate::tools::semantic_retriever::semantic_results_for_query(
        state,
        query,
        candidate_limit,
        false,
        normalized_path_hint.as_deref(),
    );

    for result in &mut results {
        let key = format!("{}:{}", result.file_path, result.symbol_name);
        if structural_names.contains(&key) {
            result.score += 0.06;
        }
    }

    let mut seen: std::collections::HashSet<String> = results
        .iter()
        .map(|r| format!("{}:{}:{}", r.file_path, r.symbol_name, r.line))
        .collect();
    for hr in lexical_candidates {
        let key = format!("{}:{}:{}", hr.file, hr.name, hr.line);
        if seen.insert(key) {
            results.push(codelens_engine::SemanticMatch {
                file_path: hr.file,
                symbol_name: hr.name,
                kind: hr.kind,
                line: hr.line,
                signature: hr.signature,
                name_path: hr.name_path,
                score: (hr.score / 100.0) * 0.35,
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
            (
                (prior_delta * 1000.0).round() / 1000.0,
                (adjusted_score * 1000.0).round() / 1000.0,
            )
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
    Ok((payload, tools::success_meta(BackendKind::Semantic, 0.85)))
}

fn annotate_provenance(payload: &mut serde_json::Value, scores: &[(f64, f64)]) {
    let Some(entries) = payload
        .get_mut("results")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (idx, entry) in entries.iter_mut().enumerate() {
        if let Some(map) = entry.as_object_mut() {
            let (prior_delta, adjusted_score) = scores.get(idx).copied().unwrap_or((0.0, 0.0));
            map.insert(
                "provenance".to_owned(),
                json!({
                    "source": "semantic",
                    "retrieval_rank": idx + 1,
                    "prior_delta": prior_delta,
                    "adjusted_score": adjusted_score,
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
