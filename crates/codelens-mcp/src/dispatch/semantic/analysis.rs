use crate::{
    AppState,
    error::CodeLensError,
    protocol::BackendKind,
    tools::{self, ToolResult},
};
use serde_json::json;

pub(in crate::dispatch) fn find_similar_code_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let file_path = tools::required_string(arguments, "file_path")?;
    let symbol_name = tools::required_string(arguments, "symbol_name")?;
    let max_results = tools::optional_usize(arguments, "max_results", 10);
    let min_similarity = arguments
        .get("min_similarity")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.3);

    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let fetch_limit = max_results.saturating_mul(2).clamp(max_results, 40);
    let raw_results = engine.find_similar_code(file_path, symbol_name, fetch_limit)?;
    let results: Vec<_> = raw_results
        .into_iter()
        .filter(|r| r.score >= min_similarity)
        .take(max_results)
        .collect();
    Ok((
        json!({
            "query_symbol": symbol_name,
            "file": file_path,
            "min_similarity": min_similarity,
            "similar": results,
            "count": results.len()
        }),
        tools::success_meta(BackendKind::Semantic, 0.80),
    ))
}

pub(in crate::dispatch) fn find_code_duplicates_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let threshold = arguments
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.85);
    let max_pairs = arguments
        .get("max_pairs")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let pairs = engine.find_duplicates(threshold, max_pairs)?;
    Ok((
        json!({"threshold": threshold, "duplicates": pairs, "count": pairs.len()}),
        tools::success_meta(BackendKind::Semantic, 0.80),
    ))
}

pub(in crate::dispatch) fn classify_symbol_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let file_path = tools::required_string(arguments, "file_path")?;
    let symbol_name = tools::required_string(arguments, "symbol_name")?;
    let categories = arguments
        .get("categories")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CodeLensError::MissingParam("categories".into()))?;
    let cat_strs: Vec<&str> = categories.iter().filter_map(|v| v.as_str()).collect();

    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let scores = engine.classify_symbol(file_path, symbol_name, &cat_strs)?;
    Ok((
        json!({"symbol": symbol_name, "file": file_path, "classifications": scores}),
        tools::success_meta(BackendKind::Semantic, 0.75),
    ))
}

pub(in crate::dispatch) fn find_misplaced_code_handler(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let outliers = engine.find_misplaced_code(max_results)?;
    Ok((
        json!({"outliers": outliers, "count": outliers.len()}),
        tools::success_meta(BackendKind::Semantic, 0.70),
    ))
}
