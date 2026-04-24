//! Static dispatch table: structural tools + feature-gated semantic handler registrations.

use crate::tools::{self};
use std::collections::HashMap;
use std::sync::LazyLock;

#[cfg(feature = "semantic")]
use crate::{AppState, error::CodeLensError, protocol::BackendKind, tools::ToolResult};
#[cfg(feature = "semantic")]
use serde_json::json;

pub(crate) static DISPATCH_TABLE: LazyLock<
    HashMap<&'static str, std::sync::Arc<dyn crate::tool_defs::tool::McpTool>>,
> = LazyLock::new(|| {
    let m = tools::dispatch_table();
    #[cfg(feature = "semantic")]
    let mut m = m;
    #[cfg(feature = "semantic")]
    {
        use crate::tool_defs::tool::BuiltTool;
        m.insert(
            "semantic_search",
            std::sync::Arc::new(BuiltTool::new(semantic_search_handler)),
        );
        m.insert(
            "index_embeddings",
            std::sync::Arc::new(BuiltTool::new(index_embeddings_handler)),
        );
        m.insert(
            "find_similar_code",
            std::sync::Arc::new(BuiltTool::new(find_similar_code_handler)),
        );
        m.insert(
            "find_code_duplicates",
            std::sync::Arc::new(BuiltTool::new(find_code_duplicates_handler)),
        );
        m.insert(
            "classify_symbol",
            std::sync::Arc::new(BuiltTool::new(classify_symbol_handler)),
        );
        m.insert(
            "find_misplaced_code",
            std::sync::Arc::new(BuiltTool::new(find_misplaced_code_handler)),
        );
    }
    m
});

// ── Semantic handlers (feature-gated) ──────────────────────────────────

#[cfg(feature = "semantic")]
fn semantic_search_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = tools::required_string(arguments, "query")?;
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let project = state.project();
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

    let candidate_limit = max_results.saturating_mul(4).clamp(max_results, 80);
    let lexical_candidates = codelens_engine::search::search_symbols_hybrid(
        &project,
        &query_analysis.expanded_query,
        candidate_limit,
        0.7,
    )
    .unwrap_or_default();
    let structural_names: std::collections::HashSet<String> = lexical_candidates
        .iter()
        .map(|result| format!("{}:{}", result.file, result.name))
        .collect();
    let mut results =
        crate::tools::symbols::semantic_results_for_query(state, query, candidate_limit, false);

    // Apply structural boost: +0.06 for results that also appear in structural candidates
    for result in &mut results {
        let key = format!("{}:{}", result.file_path, result.symbol_name);
        if structural_names.contains(&key) {
            result.score += 0.06;
        }
    }

    // Merge hybrid search results: lexical/FTS/fuzzy catches symbols that
    // semantic embedding misses (e.g. "parse" → parse_symbols via FTS).
    // Convert hybrid SearchResults into SemanticMatch format and merge.
    {
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
                    // Scale hybrid scores to semantic range (0.1-0.3).
                    // Hybrid raw scores: exact=100, fts=40-80, fuzzy=50-90.
                    // We want them as supplementary candidates, not dominant.
                    score: (hr.score / 100.0) * 0.35,
                });
            }
        }
    }

    // Re-sort and truncate
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
        }
    });
    if let Some(entries) = payload
        .get_mut("results")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (idx, entry) in entries.iter_mut().enumerate() {
            if let Some(map) = entry.as_object_mut() {
                let (prior_delta, adjusted_score) =
                    result_scores.get(idx).copied().unwrap_or((0.0, 0.0));
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
    Ok((payload, tools::success_meta(BackendKind::Semantic, 0.85)))
}

#[cfg(feature = "semantic")]
fn index_embeddings_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let project = state.project();
    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let count = engine.index_from_project(&project)?;

    // Auto-generate project-specific NL→code bridges from docstrings.
    // Written to .codelens/bridges.json for semantic_query_for_embedding_search().
    let bridges_generated = match engine.generate_bridge_candidates(&project) {
        Ok(bridges) if !bridges.is_empty() => {
            let bridges_dir = project.as_path().join(".codelens");
            let _ = std::fs::create_dir_all(&bridges_dir);
            let json_entries: Vec<serde_json::Value> = bridges
                .iter()
                .map(|(nl, code)| json!({"nl": nl, "code": code}))
                .collect();
            let _ = std::fs::write(
                bridges_dir.join("bridges.json"),
                serde_json::to_string_pretty(&json_entries).unwrap_or_default(),
            );
            bridges.len()
        }
        _ => 0,
    };

    let prewarm_limit = arguments
        .get("prewarm_limit")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(128)
        .min(1024);
    let mut prewarm_queries = Vec::new();
    if prewarm_limit > 0
        && let Some(items) = arguments
            .get("prewarm_queries")
            .and_then(|value| value.as_array())
    {
        let mut seen = std::collections::HashSet::new();
        for query in items.iter().filter_map(|value| value.as_str()) {
            if prewarm_queries.len() >= prewarm_limit {
                break;
            }
            let query_analysis = crate::tools::query_analysis::analyze_retrieval_query(query);
            if query_analysis.semantic_query.is_empty() {
                continue;
            }
            let search_query = crate::tools::query_analysis::semantic_query_for_embedding_search(
                &query_analysis,
                Some(project.as_path()),
            );
            if seen.insert(search_query.clone()) {
                prewarm_queries.push(search_query);
            }
        }
    }
    let prewarmed = engine.prewarm_queries(&prewarm_queries)?;
    let query_cache = engine.query_cache_stats()?;

    Ok((
        json!({
            "indexed_symbols": count,
            "bridges_generated": bridges_generated,
            "status": "ok",
            "query_cache": {
                "enabled": query_cache.enabled,
                "entries": query_cache.entries,
                "max_entries": query_cache.max_entries,
                "prewarmed": prewarmed,
            }
        }),
        tools::success_meta(BackendKind::Semantic, 0.95),
    ))
}

#[cfg(feature = "semantic")]
fn find_similar_code_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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

    // Over-fetch then filter by minimum similarity threshold
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

#[cfg(feature = "semantic")]
fn find_code_duplicates_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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

#[cfg(feature = "semantic")]
fn classify_symbol_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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

#[cfg(feature = "semantic")]
fn find_misplaced_code_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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

#[cfg(all(test, feature = "semantic"))]
mod tests {
    use crate::tools::query_analysis::{analyze_retrieval_query, rerank_semantic_matches};
    use codelens_engine::SemanticMatch;

    fn semantic_match(file_path: &str, symbol_name: &str, kind: &str, score: f64) -> SemanticMatch {
        SemanticMatch {
            file_path: file_path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: kind.to_owned(),
            line: 1,
            signature: String::new(),
            name_path: symbol_name.to_owned(),
            score,
        }
    }

    #[test]
    fn prefers_extract_entrypoint_over_script_variables() {
        let reranked = rerank_semantic_matches(
            "extract lines of code into a new function",
            vec![
                semantic_match(
                    "scripts/finetune/build_codex_dataset.py",
                    "line",
                    "variable",
                    0.233,
                ),
                semantic_match(
                    "benchmarks/harness/task-bootstrap.py",
                    "lines",
                    "variable",
                    0.219,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/tools/composite.rs",
                    "refactor_extract_function",
                    "function",
                    0.184,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "refactor_extract_function");
    }

    #[test]
    fn prefers_dispatch_entrypoint_over_handler_types() {
        let reranked = rerank_semantic_matches(
            "route an incoming tool request to the right handler",
            vec![
                semantic_match(
                    "crates/codelens-mcp/src/tools/mod.rs",
                    "ToolHandler",
                    "unknown",
                    0.313,
                ),
                semantic_match(
                    "benchmarks/harness/harness_runner_common.py",
                    "tool_list",
                    "variable",
                    0.266,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/dispatch.rs",
                    "dispatch_tool",
                    "function",
                    0.224,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "dispatch_tool");
    }

    #[test]
    fn prefers_stdio_entrypoint_over_generic_read_helpers() {
        let reranked = rerank_semantic_matches(
            "read input from stdin line by line run_stdio stdio stdin",
            vec![
                semantic_match(
                    "crates/codelens-core/src/file_ops/mod.rs",
                    "read_line_at",
                    "function",
                    0.261,
                ),
                semantic_match(
                    "crates/codelens-core/src/file_ops/reader.rs",
                    "read_file",
                    "function",
                    0.258,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/server/transport_stdio.rs",
                    "run_stdio",
                    "function",
                    0.148,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "run_stdio");
    }

    #[test]
    fn prefers_mutation_gate_entrypoint_over_telemetry_helpers() {
        let reranked = rerank_semantic_matches(
            "mutation gate preflight check before editing evaluate_mutation_gate mutation_gate preflight",
            vec![
                semantic_match(
                    "crates/codelens-mcp/src/telemetry.rs",
                    "record_mutation_preflight_checked",
                    "function",
                    0.402,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/telemetry.rs",
                    "record_mutation_preflight_gate_denied",
                    "function",
                    0.314,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/mutation_gate.rs",
                    "evaluate_mutation_gate",
                    "function",
                    0.280,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "evaluate_mutation_gate");
    }

    #[test]
    fn expands_stdio_alias_terms() {
        let expanded = analyze_retrieval_query("read input from stdin line by line").expanded_query;
        assert!(expanded.contains("run_stdio"));
        assert!(expanded.contains("stdio"));
    }
}
