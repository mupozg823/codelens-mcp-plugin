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
    // F17: mirror cleanup_duplicate_logic's G6.1 config-noise suppression so
    // CI-YAML structural-key pairs don't dominate the top results. Opt back
    // into the original unfiltered behavior with include_config_code_pairs.
    let include_config_code_pairs = arguments
        .get("include_config_code_pairs")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    // Over-fetch before suppression so the max_pairs budget still fills with
    // real code duplicates that would otherwise sit below the cutoff once the
    // config pairs are removed.
    let scan_limit = crate::tools::workflows::duplicate_cleanup::duplicate_quality_scan_limit(
        include_config_code_pairs,
        true,
        max_pairs,
    );
    let raw_pairs = engine.find_duplicates(threshold, scan_limit)?;
    let pairs = crate::tools::workflows::duplicate_cleanup::filter_find_code_duplicate_pairs(
        &state.project(),
        raw_pairs,
        max_pairs,
        include_config_code_pairs,
    );
    Ok((
        json!({
            "threshold": threshold,
            "include_config_code_pairs": include_config_code_pairs,
            "duplicates": pairs,
            "count": pairs.len()
        }),
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
