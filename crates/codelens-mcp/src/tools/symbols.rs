use super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::analyze_retrieval_query, required_string, success_meta,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    RankedContextResult, SemanticMatch, SymbolInfo, SymbolKind, read_file,
    search_symbols_hybrid_with_semantic,
};
use serde_json::{Value, json};

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
        let results = engine
            .search(&query_analysis.semantic_query, candidate_limit)
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

fn semantic_scores_for_query(
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

fn merge_semantic_ranked_entries(
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

fn compact_semantic_evidence(
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

fn annotate_ranked_context_provenance(
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

fn truncate_body_preview(body: &str, max_lines: usize, max_chars: usize) -> (String, bool) {
    let mut truncated = false;
    let lines = body.lines().take(max_lines).collect::<Vec<_>>();
    if body.lines().count() > max_lines {
        truncated = true;
    }
    let mut preview = lines.join("\n");
    if preview.len() > max_chars {
        let mut boundary = max_chars.min(preview.len());
        while boundary > 0 && !preview.is_char_boundary(boundary) {
            boundary -= 1;
        }
        preview.truncate(boundary);
        truncated = true;
    }
    if truncated {
        preview.push_str("\n... [truncated; rerun with body_full=true for the full body]");
    }
    (preview, truncated)
}

fn compact_symbol_bodies(
    symbols: &mut [SymbolInfo],
    max_symbols_with_body: usize,
    max_body_lines: usize,
    max_body_chars: usize,
) -> usize {
    let mut truncated_count = 0;
    for (idx, symbol) in symbols.iter_mut().enumerate() {
        if let Some(body) = symbol.body.as_ref() {
            if idx >= max_symbols_with_body {
                symbol.body = None;
                truncated_count += 1;
                continue;
            }
            let (preview, truncated) = truncate_body_preview(body, max_body_lines, max_body_chars);
            if truncated {
                symbol.body = Some(preview);
                truncated_count += 1;
            }
        }
    }
    truncated_count
}

pub fn get_symbols_overview(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let explicit_depth = arguments.get("depth").and_then(|v| v.as_u64());
    let depth = explicit_depth.unwrap_or(1) as usize;
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let budget = state.execution_token_budget(&session);
    let mut symbols = state
        .symbol_index()
        .get_symbols_overview_cached(path, depth)?;

    // Token guard: auto-strip children when response would exceed budget.
    // Skip if depth was explicitly requested (user intentionally wants full detail).
    let estimated_chars: usize = symbols.iter().map(|s| 80 + s.children.len() * 120).sum();
    let budget_chars = budget * 4;
    let stripped = explicit_depth.is_none() && estimated_chars > budget_chars;
    if stripped {
        for sym in &mut symbols {
            let child_count = sym.children.len();
            sym.children.clear();
            sym.signature = format!("{} ({child_count} symbols)", sym.signature);
        }
    }

    // Hard limit: truncate if still too large (unless explicit depth)
    let max_symbols = if explicit_depth.is_some() {
        usize::MAX
    } else {
        budget_chars / 80
    };
    let truncated = symbols.len() > max_symbols;
    if truncated {
        symbols.truncate(max_symbols);
    }

    Ok((
        json!({
            "symbols": symbols,
            "count": symbols.len(),
            "truncated": truncated,
            "auto_summarized": stripped,
        }),
        success_meta(BackendKind::TreeSitter, 0.93),
    ))
}

pub fn find_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let symbol_id = optional_string(arguments, "symbol_id");
    let name = symbol_id
        .or_else(|| optional_string(arguments, "name"))
        .ok_or_else(|| CodeLensError::MissingParam("symbol_id or name".into()))?;
    let file_path = optional_string(arguments, "file_path");
    let include_body = optional_bool(arguments, "include_body", false);
    let exact_match = optional_bool(arguments, "exact_match", false);
    let max_matches = optional_usize(arguments, "max_matches", 50);
    let body_full = optional_bool(arguments, "body_full", false);
    let body_line_limit = optional_usize(arguments, "body_line_limit", 12);
    let body_char_limit = optional_usize(arguments, "body_char_limit", 600);
    Ok(state
        .symbol_index()
        .find_symbol_cached(name, file_path, include_body, exact_match, max_matches)
        .map(|mut value| {
            let body_truncated_count = if include_body && !body_full {
                compact_symbol_bodies(&mut value, 3, body_line_limit, body_char_limit)
            } else {
                0
            };
            (
                json!({
                    "symbols": value,
                    "count": value.len(),
                    "body_truncated_count": body_truncated_count,
                    "body_preview": include_body && !body_full,
                }),
                success_meta(BackendKind::TreeSitter, 0.93),
            )
        })?)
}

pub fn get_ranked_context(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let query_analysis = analyze_retrieval_query(query);
    let path = optional_string(arguments, "path");
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let max_tokens = arguments
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or_else(|| state.execution_token_budget(&session));
    let include_body = optional_bool(arguments, "include_body", false);
    let depth = optional_usize(arguments, "depth", 2);
    let disable_semantic = optional_bool(arguments, "disable_semantic", false);
    let effective_disable_semantic = disable_semantic || query_analysis.prefer_lexical_only;
    let use_semantic_in_core = !effective_disable_semantic;
    // Build semantic scores for hybrid ranking if embeddings are available.
    // The default model is the bundled CodeSearchNet MiniLM-L12 INT8 variant.
    let semantic_results = semantic_results_for_query(state, query, 50, effective_disable_semantic);
    let semantic_scores = semantic_results
        .iter()
        .filter(|r| r.score > 0.05)
        .map(|r| (format!("{}:{}", r.file_path, r.symbol_name), r.score))
        .collect();

    // Boost scores for files recently accessed in this session
    let recent_files = state.recent_file_paths_for_session(&session);
    let mut boosted_scores: std::collections::HashMap<String, f64> = if use_semantic_in_core {
        semantic_scores
    } else {
        std::collections::HashMap::new()
    };
    if !recent_files.is_empty() {
        let boost = 0.15_f64;
        for (key, score) in boosted_scores.iter_mut() {
            if recent_files.iter().any(|f| key.starts_with(f.as_str())) {
                *score += boost;
            }
        }
    }

    // query-type-aware weights available via get_ranked_context_cached_with_query_type
    // but current dataset shows default weights are near-optimal (0.680 MRR).
    // Kept as None until per-type weight tuning yields measurable improvement.
    let mut result = state.symbol_index().get_ranked_context_cached(
        &query_analysis.expanded_query,
        path,
        max_tokens,
        include_body,
        depth,
        Some(&state.graph_cache()),
        boosted_scores,
    )?;
    let structural_keys = result
        .symbols
        .iter()
        .map(|entry| format!("{}:{}", entry.file, entry.name))
        .collect::<std::collections::HashSet<_>>();

    if !effective_disable_semantic {
        merge_semantic_ranked_entries(query, &mut result, semantic_results.clone(), 8);
    }

    // v1.5 Phase 2e: sparse term coverage bonus — post-process
    // re-ordering pass. Runs on the ORIGINAL user `query`, not the
    // MCP-expanded retrieval string, because the expansion adds dozens
    // of derivative tokens (snake_case, CamelCase, alias groups) that
    // dilute the coverage ratio below any reasonable threshold — the
    // 4-arm pilot that measured zero effect used the expanded query
    // and confirmed this dilution. Running the pass here (after
    // `get_ranked_context_cached` + `merge_semantic_ranked_entries`)
    // also keeps the engine layer free of query-semantics knowledge —
    // the engine ranks, the MCP layer decides what "the query" means.
    if codelens_engine::sparse_weighting_enabled() {
        let query_lower_for_sparse = query.to_lowercase();
        let mut changed = false;
        for entry in result.symbols.iter_mut() {
            let bonus = codelens_engine::sparse_coverage_bonus_from_fields(
                &query_lower_for_sparse,
                &entry.name,
                &entry.name, // no name_path on RankedContextEntry; reuse name
                &entry.signature,
                &entry.file,
            );
            if bonus > 0.0 {
                entry.relevance_score = entry.relevance_score.saturating_add(bonus as i32);
                changed = true;
            }
        }
        if changed {
            result
                .symbols
                .sort_unstable_by(|a, b| b.relevance_score.cmp(&a.relevance_score));
        }
    }

    let semantic_evidence = if effective_disable_semantic {
        Vec::new()
    } else {
        compact_semantic_evidence(&result, &semantic_results, 5)
    };
    let mut payload =
        serde_json::to_value(&result).map_err(|e| CodeLensError::Internal(e.into()))?;
    annotate_ranked_context_provenance(&mut payload, &structural_keys, &semantic_results);
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "retrieval".to_owned(),
            json!({
                "semantic_enabled": !effective_disable_semantic,
                "semantic_used_in_core": use_semantic_in_core,
                "query_type": if query_analysis.prefer_lexical_only { "identifier" }
                    else if query_analysis.natural_language { "natural_language" }
                    else { "short_phrase" },
                "lexical_query": query_analysis.expanded_query,
                "semantic_query": query_analysis.semantic_query,
            }),
        );
        if !semantic_evidence.is_empty() {
            map.insert("semantic_evidence".to_owned(), json!(semantic_evidence));
        }
    }

    let backend = if result.symbols.iter().any(|s| s.relevance_score > 0) {
        BackendKind::TreeSitter
    } else {
        BackendKind::Semantic
    };
    Ok((payload, success_meta(backend, 0.91)))
}

pub fn refresh_symbol_index(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let stats = state.symbol_index().refresh_all()?;
    state.graph_cache().invalidate();
    Ok((json!(stats), success_meta(BackendKind::TreeSitter, 0.95)))
}

pub fn get_complexity(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let symbol_name = optional_string(arguments, "symbol_name");
    let file_result = read_file(&state.project(), path, None, None)?;
    let lines = file_result.content.lines().collect::<Vec<_>>();
    let symbols = state.symbol_index().get_symbols_overview_cached(path, 2)?;

    let functions = flatten_symbols(&symbols)
        .into_iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| symbol_name.is_none_or(|name| s.name == name))
        .map(|s| {
            let start = s.line.saturating_sub(1).min(lines.len());
            let end = (s.line + 50).min(lines.len());
            let branches = count_branches(&lines[start..end]);
            json!({
                "name": s.name,
                "kind": s.kind.as_label(),
                "file": s.file_path,
                "line": s.line,
                "branches": branches,
                "complexity": 1 + branches
            })
        })
        .collect::<Vec<_>>();

    let results = if functions.is_empty() {
        let branches = count_branches(&lines);
        vec![json!({
            "name": path,
            "branches": branches,
            "complexity": 1 + branches
        })]
    } else {
        functions
    };

    let avg_complexity = if results.is_empty() {
        0.0
    } else {
        results
            .iter()
            .filter_map(|e| e.get("complexity").and_then(|v| v.as_i64()))
            .map(|v| v as f64)
            .sum::<f64>()
            / results.len() as f64
    };

    Ok((
        json!({
            "path": path,
            "functions": results,
            "count": results.len(),
            "avg_complexity": avg_complexity
        }),
        success_meta(BackendKind::TreeSitter, 0.89),
    ))
}

pub fn get_project_structure(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let dirs = state.symbol_index().get_project_structure()?;
    let total_files: usize = dirs.iter().map(|d| d.files).sum();
    let total_symbols: usize = dirs.iter().map(|d| d.symbols).sum();
    Ok((
        json!({
            "directories": dirs,
            "total_files": total_files,
            "total_symbols": total_symbols,
            "dir_count": dirs.len()
        }),
        success_meta(BackendKind::Sqlite, 0.95),
    ))
}

pub fn search_symbols_fuzzy(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let max_results = optional_usize(arguments, "max_results", 30);
    let fuzzy_threshold = arguments
        .get("fuzzy_threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.6);
    let disable_semantic = optional_bool(arguments, "disable_semantic", false);
    // Build semantic scores if embeddings are available (same pattern as get_ranked_context)
    let semantic_scores = semantic_scores_for_query(state, query, 50, disable_semantic);

    let sem_ref = if semantic_scores.is_empty() {
        None
    } else {
        Some(&semantic_scores)
    };

    let backend = if sem_ref.is_some() {
        BackendKind::Hybrid
    } else {
        BackendKind::Sqlite
    };

    Ok(search_symbols_hybrid_with_semantic(
        &state.project(),
        query,
        max_results,
        fuzzy_threshold,
        sem_ref,
    )
    .map(|value| {
        (
            json!({ "results": value, "count": value.len() }),
            success_meta(backend, 0.9),
        )
    })?)
}

// ── Helpers ──────────────────────────────────────────────────────────────

pub fn flatten_symbols(symbols: &[SymbolInfo]) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    let mut stack = symbols.to_vec();
    while let Some(mut symbol) = stack.pop() {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        stack.extend(children);
    }
    flat
}

fn count_branches(lines: &[&str]) -> i32 {
    lines.iter().map(|line| count_branches_in_line(line)).sum()
}

fn count_branches_in_line(line: &str) -> i32 {
    let mut count = 0i32;
    // "if" already counts the branch in "else if", so no separate else-if handling needed.
    for token in [
        "if", "elif", "for", "while", "catch", "except", "case", "and", "or",
    ] {
        count += count_word_occurrences(line, token);
    }
    count += line.match_indices("&&").count() as i32;
    count += line.match_indices("||").count() as i32;
    count
}

fn count_word_occurrences(line: &str, needle: &str) -> i32 {
    line.match_indices(needle)
        .filter(|(index, _)| {
            let start_ok = *index == 0
                || !line[..*index]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let end = index + needle.len();
            let end_ok = end == line.len()
                || !line[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            start_ok && end_ok
        })
        .count() as i32
}

#[cfg(test)]
mod tests {
    use super::{
        annotate_ranked_context_provenance, merge_semantic_ranked_entries, truncate_body_preview,
    };
    use codelens_engine::{RankedContextEntry, RankedContextResult, SemanticMatch};
    use serde_json::json;

    #[test]
    fn merge_semantic_ranked_entries_inserts_and_upgrades() {
        let mut result = RankedContextResult {
            query: "rename across project".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 128,
            symbols: vec![RankedContextEntry {
                name: "project_scope_renames_across_files".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-core/src/rename.rs".to_owned(),
                line: 10,
                signature: "fn project_scope_renames_across_files".to_owned(),
                body: None,
                relevance_score: 32,
            }],
        };

        merge_semantic_ranked_entries(
            "rename a variable or function across the project",
            &mut result,
            vec![
                SemanticMatch {
                    symbol_name: "project_scope_renames_across_files".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 10,
                    signature: "fn project_scope_renames_across_files".to_owned(),
                    name_path: "project_scope_renames_across_files".to_owned(),
                    score: 0.41,
                },
                SemanticMatch {
                    symbol_name: "rename_symbol".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 42,
                    signature: "fn rename_symbol".to_owned(),
                    name_path: "rename_symbol".to_owned(),
                    score: 0.93,
                },
            ],
            8,
        );

        assert_eq!(result.symbols[0].name, "rename_symbol");
        assert!(result.symbols[0].relevance_score >= 90);
        assert!(
            result
                .symbols
                .iter()
                .find(|entry| entry.name == "project_scope_renames_across_files")
                .unwrap()
                .relevance_score
                > 32
        );
    }

    #[test]
    fn short_phrase_merge_only_inserts_top_confident_semantic_hit() {
        let mut result = RankedContextResult {
            query: "change function parameters".to_owned(),
            count: 1,
            token_budget: 1200,
            chars_used: 64,
            symbols: vec![RankedContextEntry {
                name: "change_signature".to_owned(),
                kind: "function".to_owned(),
                file: "crates/codelens-core/src/refactor.rs".to_owned(),
                line: 12,
                signature: "fn change_signature".to_owned(),
                body: None,
                relevance_score: 41,
            }],
        };

        merge_semantic_ranked_entries(
            "change function parameters",
            &mut result,
            vec![
                SemanticMatch {
                    symbol_name: "apply_signature_change".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-core/src/refactor.rs".to_owned(),
                    line: 44,
                    signature: "fn apply_signature_change".to_owned(),
                    name_path: "apply_signature_change".to_owned(),
                    score: 0.32,
                },
                SemanticMatch {
                    symbol_name: "rewrite_call_arguments".to_owned(),
                    kind: "function".to_owned(),
                    file_path: "crates/codelens-core/src/refactor.rs".to_owned(),
                    line: 60,
                    signature: "fn rewrite_call_arguments".to_owned(),
                    name_path: "rewrite_call_arguments".to_owned(),
                    score: 0.27,
                },
            ],
            8,
        );

        assert!(
            result
                .symbols
                .iter()
                .any(|entry| entry.name == "apply_signature_change")
        );
        assert!(
            !result
                .symbols
                .iter()
                .any(|entry| entry.name == "rewrite_call_arguments")
        );
    }

    #[test]
    fn truncate_body_preview_respects_utf8_boundaries() {
        let body = "가나다abc";
        let (preview, truncated) = truncate_body_preview(body, 10, 4);
        assert!(truncated);
        assert!(preview.starts_with("가"));
        assert!(!preview.starts_with("가나"));
    }

    #[test]
    fn annotate_ranked_context_provenance_marks_structural_and_semantic_entries() {
        let result = RankedContextResult {
            query: "rename across project".to_owned(),
            count: 2,
            token_budget: 1200,
            chars_used: 128,
            symbols: vec![
                RankedContextEntry {
                    name: "project_scope_renames_across_files".to_owned(),
                    kind: "function".to_owned(),
                    file: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 10,
                    signature: "fn project_scope_renames_across_files".to_owned(),
                    body: None,
                    relevance_score: 64,
                },
                RankedContextEntry {
                    name: "rename_symbol".to_owned(),
                    kind: "function".to_owned(),
                    file: "crates/codelens-core/src/rename.rs".to_owned(),
                    line: 42,
                    signature: "fn rename_symbol".to_owned(),
                    body: None,
                    relevance_score: 91,
                },
            ],
        };
        let structural_keys = std::collections::HashSet::from([format!(
            "{}:{}",
            "crates/codelens-core/src/rename.rs", "project_scope_renames_across_files"
        )]);
        let semantic_results = vec![
            SemanticMatch {
                symbol_name: "project_scope_renames_across_files".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                line: 10,
                signature: "fn project_scope_renames_across_files".to_owned(),
                name_path: "project_scope_renames_across_files".to_owned(),
                score: 0.411,
            },
            SemanticMatch {
                symbol_name: "rename_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-core/src/rename.rs".to_owned(),
                line: 42,
                signature: "fn rename_symbol".to_owned(),
                name_path: "rename_symbol".to_owned(),
                score: 0.933,
            },
        ];

        let mut payload = json!(result);
        annotate_ranked_context_provenance(&mut payload, &structural_keys, &semantic_results);

        let symbols = payload["symbols"].as_array().unwrap();
        assert_eq!(
            symbols[0]["provenance"]["source"],
            json!("semantic_boosted")
        );
        assert_eq!(symbols[1]["provenance"]["source"], json!("semantic_added"));
        assert_eq!(symbols[1]["provenance"]["semantic_score"], json!(0.933));
    }
}
