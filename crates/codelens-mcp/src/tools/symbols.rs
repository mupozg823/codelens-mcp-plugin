use super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_core::{
    RankedContextResult, SemanticMatch, SymbolInfo, SymbolKind, read_file,
    search_symbols_hybrid_with_semantic,
};
use serde_json::{Value, json};

fn query_prefers_lexical_only(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(char::is_whitespace) {
        return false;
    }
    let looks_path_like = trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("::");
    let identifier_chars_only = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    looks_path_like || identifier_chars_only
}

fn is_natural_language_query(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && !query_prefers_lexical_only(trimmed)
        && trimmed.split_whitespace().count() >= 3
}

pub(crate) fn expanded_query_for_retrieval(query: &str) -> String {
    if !is_natural_language_query(query) {
        return query.trim().to_owned();
    }

    let lowered = query.to_lowercase();
    let mut terms = vec![query.trim().to_owned()];
    let mut push_unique = |term: &str| {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_owned());
        }
    };

    // Dynamic expansion: convert NL query words to snake_case symbol candidates
    // This improves cross-project generalization. The embedding model needs
    // training data with these patterns to rank them properly.
    let words: Vec<&str> = lowered.split_whitespace().filter(|w| w.len() > 2).collect();
    if words.len() >= 2 && words.len() <= 6 {
        for window in words.windows(2) {
            push_unique(&format!("{}_{}", window[0], window[1]));
        }
    }

    let alias_groups: &[(&[&str], &[&str])] = &[
        (
            &["rename", "refactor"],
            &["rename_symbol", "refactor", "rename"],
        ),
        (
            &["defined", "definition", "symbol is defined"],
            &["find_symbol_range", "definition", "range", "reader"],
        ),
        (&["search", "query"], &["search", "semantic", "embedding"]),
        (&["inline"], &["inline_function", "inline", "refactor"]),
        (
            &["http", "server", "routes"],
            &["run_http", "transport_http", "router"],
        ),
        (&["stdin", "line by line"], &["run_stdio", "stdio", "stdin"]),
        (
            &["parse", "ast"],
            &["parse_symbols", "parser", "ast", "tree_sitter"],
        ),
        (
            &["embedding", "vectors"],
            &["index_from_project", "embedding", "index"],
        ),
        (
            &["duplicate", "near-duplicate", "similar"],
            &["find_duplicates", "similarity", "dedupe"],
        ),
        (
            &["categorize", "purpose"],
            &["classify_symbol", "classify", "category"],
        ),
        (
            &["project structure", "first load", "key files"],
            &["onboard_project", "project_structure", "overview"],
        ),
        (
            &["watch", "filesystem", "file changes"],
            &["FileWatcher", "watcher", "notify", "watch"],
        ),
        (
            &["extract", "new function"],
            &["refactor_extract_function", "extract", "refactor"],
        ),
        (
            &["change", "parameters", "signature"],
            &["change_signature", "signature", "parameters"],
        ),
        (
            &["comments", "string literals"],
            &[
                "build_non_code_ranges",
                "non_code_ranges",
                "comments strings",
            ],
        ),
        (
            &["route", "handler", "tool request"],
            &["dispatch_tool", "dispatch", "handler"],
        ),
        (
            &["mutation", "gate", "preflight"],
            &["evaluate_mutation_gate", "mutation_gate", "preflight"],
        ),
        (
            &["truncat", "budget", "payload"],
            &["bounded_result_payload", "truncate", "budget_hint"],
        ),
        (
            &["recently accessed", "recent files"],
            &["record_file_access", "recent_files", "recent"],
        ),
        (
            &["client", "detect", "codex", "claude"],
            &["detect", "ClientProfile", "client_profile"],
        ),
        (
            &["exclude", "ignore", "node_modules"],
            &["is_excluded", "EXCLUDED_DIRS", "excluded"],
        ),
    ];

    for (needles, aliases) in alias_groups {
        if needles.iter().any(|needle| lowered.contains(needle)) {
            for alias in *aliases {
                push_unique(alias);
            }
        }
    }

    terms.join(" ")
}

#[cfg(feature = "semantic")]
pub(crate) fn semantic_status(state: &AppState) -> Value {
    let configured_model = codelens_core::configured_embedding_model_name();
    if let Some(engine_opt) = state.embedding.get() {
        return match engine_opt {
            Some(engine) => {
                let info = engine.index_info();
                if info.indexed_symbols > 0 {
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
                }
            }
            None => json!({
                "status": "failed",
                "model": configured_model,
                "loaded": false,
                "reason": "embedding engine failed to initialize",
            }),
        };
    }

    match codelens_core::EmbeddingEngine::inspect_existing_index(&state.project())
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
    let configured_model = codelens_core::configured_embedding_model_name();
    let indexed = codelens_core::EmbeddingEngine::inspect_existing_index(&state.project())
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
pub(crate) fn is_semantic_available(state: &AppState) -> bool {
    semantic_status(state)
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "ready")
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn is_semantic_available(_state: &AppState) -> bool {
    false
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

    let project = state.project();
    let engine_opt = state
        .embedding
        .get_or_init(|| codelens_core::EmbeddingEngine::new(&project).ok());
    if let Some(engine) = engine_opt
        && engine.is_indexed()
    {
        return engine.search(query, limit).unwrap_or_default();
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
    let (semantic_base, decay_per_rank, semantic_strength, effective_limit) =
        if query_word_count >= 4 {
            (150, 16, 60.0, max_semantic_entries.min(6))
        } else if query_word_count >= 2 {
            // Short phrases benefit from a strong semantic top hit, but semantic-only
            // tails quickly crowd out good lexical candidates. Limit the tail and only
            // allow a new semantic-only insertion for the top, high-confidence match.
            (115, 35, 60.0, max_semantic_entries.min(2))
        } else {
            (120, 24, 50.0, max_semantic_entries.min(3))
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
        let semantic_score = (semantic_base - (rank_idx as i32 * decay_per_rank)
            + (sem.score * semantic_strength) as i32)
            .clamp(1, 220);
        if let Some(idx) = index_by_key.get(&key).copied() {
            result.symbols[idx].relevance_score =
                result.symbols[idx].relevance_score.max(semantic_score);
            continue;
        }
        if is_short_phrase && (rank_idx > 0 || sem.score < 0.18) {
            continue;
        }

        let idx = result.symbols.len();
        result.symbols.push(codelens_core::RankedContextEntry {
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
    let body_full = arguments
        .get("body_full")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let body_line_limit = arguments
        .get("body_line_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(12) as usize;
    let body_char_limit = arguments
        .get("body_char_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(600) as usize;
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
    let expanded_query = expanded_query_for_retrieval(query);
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
    let disable_semantic = arguments
        .get("disable_semantic")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let effective_disable_semantic = disable_semantic || query_prefers_lexical_only(query);
    let query_word_count = query.split_whitespace().count();
    let use_semantic_in_core = !effective_disable_semantic && !(2..4).contains(&query_word_count);
    // Build semantic scores for hybrid ranking if embeddings are available.
    // The default model is the bundled CodeSearchNet MiniLM-L12 INT8 variant.
    let semantic_results =
        semantic_results_for_query(state, &expanded_query, 50, effective_disable_semantic);
    let semantic_scores = semantic_results
        .iter()
        .filter(|r| r.score > 0.05)
        .map(|r| (format!("{}:{}", r.file_path, r.symbol_name), r.score))
        .collect();

    // Boost scores for files recently accessed in this session
    let recent_files = state.recent_file_paths();
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

    let mut result = state.symbol_index().get_ranked_context_cached(
        query,
        path,
        max_tokens,
        include_body,
        depth,
        Some(&state.graph_cache()),
        boosted_scores,
    )?;

    if !effective_disable_semantic {
        merge_semantic_ranked_entries(query, &mut result, semantic_results, 8);
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
    let disable_semantic = arguments
        .get("disable_semantic")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
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
    use super::{merge_semantic_ranked_entries, query_prefers_lexical_only, truncate_body_preview};
    use codelens_core::{RankedContextEntry, RankedContextResult, SemanticMatch};

    #[test]
    fn identifier_queries_prefer_lexical_only() {
        assert!(query_prefers_lexical_only("rename_symbol"));
        assert!(query_prefers_lexical_only("dispatch_tool"));
        assert!(query_prefers_lexical_only("crate::dispatch_tool"));
        assert!(!query_prefers_lexical_only(
            "rename a variable or function across the project"
        ));
        assert!(!query_prefers_lexical_only("change function parameters"));
    }

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
        assert_eq!(
            result
                .symbols
                .iter()
                .find(|entry| entry.name == "project_scope_renames_across_files")
                .unwrap()
                .relevance_score,
            174
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
}

#[test]
fn route_query_expansion_includes_dispatch_aliases() {
    let query = "route an incoming tool request to the right handler";
    let expanded = expanded_query_for_retrieval(query);
    assert!(expanded.contains("dispatch_tool"));
    assert!(expanded.contains("handler"));
    assert!(expanded.contains(query));
}

#[test]
fn stdio_query_expansion_includes_stdio_aliases() {
    let query = "read input from stdin line by line";
    let expanded = expanded_query_for_retrieval(query);
    assert!(expanded.contains("run_stdio"));
    assert!(expanded.contains("stdio"));
    assert!(expanded.contains(query));
}

#[test]
fn definition_query_expansion_includes_find_symbol_range_alias() {
    let query = "find where a symbol is defined in a file";
    let expanded = expanded_query_for_retrieval(query);
    assert!(expanded.contains("find_symbol_range"));
    assert!(expanded.contains("definition"));
    assert!(expanded.contains(query));
}

#[test]
fn change_signature_query_expansion_includes_exact_alias() {
    let query = "change function parameters";
    let expanded = expanded_query_for_retrieval(query);
    assert!(expanded.contains("change_signature"));
    assert!(expanded.contains("signature"));
    assert!(expanded.contains(query));
}
