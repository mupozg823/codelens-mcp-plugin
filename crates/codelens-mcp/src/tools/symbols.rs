use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_core::{read_file, search_symbols_hybrid_with_semantic, SymbolInfo, SymbolKind};
use serde_json::json;

pub fn get_symbols_overview(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let explicit_depth = arguments.get("depth").and_then(|v| v.as_u64());
    let depth = explicit_depth.unwrap_or(1) as usize;
    let budget = state.token_budget();
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
    let symbol_id = arguments.get("symbol_id").and_then(|v| v.as_str());
    let name = symbol_id
        .or_else(|| arguments.get("name").and_then(|v| v.as_str()))
        .ok_or_else(|| CodeLensError::MissingParam("symbol_id or name".into()))?;
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());
    let include_body = arguments
        .get("include_body")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let exact_match = arguments
        .get("exact_match")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_matches = arguments
        .get("max_matches")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    Ok(state
        .symbol_index()
        .find_symbol_cached(name, file_path, include_body, exact_match, max_matches)
        .map(|value| {
            (
                json!({ "symbols": value, "count": value.len() }),
                success_meta(BackendKind::TreeSitter, 0.93),
            )
        })?)
}

pub fn get_ranked_context(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let path = arguments.get("path").and_then(|v| v.as_str());
    let max_tokens = arguments
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or_else(|| state.token_budget());
    let include_body = arguments
        .get("include_body")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let depth = arguments.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
    // Build semantic scores for hybrid ranking if embeddings are available.
    // Model is bundled in binary (~34MB ONNX), loads in ~200ms on first call.
    #[cfg(feature = "semantic")]
    let semantic_scores = {
        let mut scores = std::collections::HashMap::new();
        let project = state.project();
        let engine_opt = state
            .embedding
            .get_or_init(|| codelens_core::EmbeddingEngine::new(&project).ok());
        if let Some(engine) = engine_opt {
            if engine.is_indexed() {
                if let Ok(sem_results) = engine.search(query, 50) {
                    for r in sem_results {
                        if r.score > 0.2 {
                            let key = format!("{}:{}", r.file_path, r.symbol_name);
                            scores.insert(key, r.score as f64);
                        }
                    }
                }
            }
        }
        scores
    };
    #[cfg(not(feature = "semantic"))]
    let semantic_scores = std::collections::HashMap::new();

    let result = state.symbol_index().get_ranked_context_cached(
        query,
        path,
        max_tokens,
        include_body,
        depth,
        Some(&state.graph_cache()),
        semantic_scores,
    )?;

    // Semantic fallback: if tree-sitter ranking returned few results and semantic
    // search is available, supplement with semantic-only results.
    #[cfg(feature = "semantic")]
    if result.symbols.len() < 3 {
        if let Some(Some(engine)) = state.embedding.get() {
            if engine.is_indexed() {
                if let Ok(sem_results) = engine.search(query, 10) {
                    let existing_keys: std::collections::HashSet<String> = result
                        .symbols
                        .iter()
                        .map(|s| format!("{}:{}", s.file, s.name))
                        .collect();
                    for r in sem_results {
                        if r.score > 0.4
                            && !existing_keys
                                .contains(&format!("{}:{}", r.file_path, r.symbol_name))
                        {
                            result.symbols.push(codelens_core::RankedContextEntry {
                                name: r.symbol_name,
                                kind: r.kind,
                                file: r.file_path,
                                line: r.line,
                                signature: r.signature,
                                body: None,
                                relevance_score: (r.score * 80.0) as i32,
                            });
                        }
                    }
                }
            }
        }
    }

    let backend = if result.symbols.iter().any(|s| s.relevance_score > 0) {
        BackendKind::TreeSitter
    } else {
        BackendKind::Semantic
    };
    Ok((json!(result), success_meta(backend, 0.91)))
}

pub fn refresh_symbol_index(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let stats = state.symbol_index().refresh_all()?;
    state.graph_cache().invalidate();
    Ok((json!(stats), success_meta(BackendKind::TreeSitter, 0.95)))
}

pub fn get_complexity(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let symbol_name = arguments.get("symbol_name").and_then(|v| v.as_str());
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
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(30) as usize;
    let fuzzy_threshold = arguments
        .get("fuzzy_threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.6);

    // Build semantic scores if embeddings are available (same pattern as get_ranked_context)
    #[cfg(feature = "semantic")]
    let semantic_scores = {
        let mut scores = std::collections::HashMap::new();
        let project = state.project();
        let engine_opt = state
            .embedding
            .get_or_init(|| codelens_core::EmbeddingEngine::new(&project).ok());
        if let Some(engine) = engine_opt {
            if engine.is_indexed() {
                if let Ok(sem_results) = engine.search(query, 50) {
                    for r in sem_results {
                        if r.score > 0.2 {
                            let key = format!("{}:{}", r.file_path, r.symbol_name);
                            scores.insert(key, r.score as f64);
                        }
                    }
                }
            }
        }
        scores
    };
    #[cfg(not(feature = "semantic"))]
    let semantic_scores = std::collections::HashMap::new();

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
    let mut stack = symbols.iter().cloned().collect::<Vec<_>>();
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
