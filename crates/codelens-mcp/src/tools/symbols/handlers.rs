use super::super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::{RetrievalQueryAnalysis, analyze_retrieval_query},
    required_string, success_meta,
};
use super::{analyzer::semantic_scores_for_query, formatter::count_branches};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{ScoredSymbol, search_symbols_bm25f, unique_query_terms};
use crate::tools::symbol_query::{SymbolQueryPipeline, SymbolQueryRequest};
use codelens_engine::{SymbolInfo, SymbolKind, read_file, search_symbols_hybrid_with_semantic};
use serde_json::{Value, json};
fn resolve_path_argument(arguments: &Value) -> Result<(&str, Vec<Value>), CodeLensError> {
    if let Some(path) = optional_string(arguments, "path") {
        if let Some(alias @ ("file_path" | "relative_path")) =
            optional_string(arguments, "_path_alias_source")
        {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
        return Ok((path, Vec::new()));
    }
    for alias in ["file_path", "relative_path"] {
        if let Some(path) = optional_string(arguments, alias) {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
    }
    Err(CodeLensError::MissingParam("path".to_owned()))
}

fn insert_response_annotations(
    payload: &mut Value,
    unknown_args: &[String],
    deprecation_warnings: &[Value],
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    if !unknown_args.is_empty() {
        map.insert("unknown_args".to_owned(), json!(unknown_args));
    }
    if !deprecation_warnings.is_empty() {
        map.insert(
            "deprecation_warnings".to_owned(),
            json!(deprecation_warnings),
        );
    }
}

pub fn get_symbols_overview(state: &AppState, arguments: &Value) -> ToolResult {
    const KNOWN_ARGS: &[&str] = &["path", "file_path", "relative_path", "depth"];
    let (path, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
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

    let mut payload = json!({
        "symbols": symbols,
        "count": symbols.len(),
        "truncated": truncated,
        "auto_summarized": stripped,
    });
    // #183 + #184 follow-up: distinguish three empty-result cases so
    // callers (humans + agents) can act on the right signal:
    //
    //   - "file_not_indexed"        — file is on disk + supported extension
    //                                 but no row in the symbol DB yet
    //                                 (watcher lag / .gitignore / project
    //                                 root mismatch)
    //   - "indexed_no_symbols"      — file is on disk, supported, AND the
    //                                 DB has a row for it; the empty list
    //                                 is the truth (empty file, comments
    //                                 only, type-alias-only module, …)
    //                                 Callers should NOT trigger refresh.
    //   - "unsupported_extension"   — extension is not in lang_registry
    //
    // The previous version conflated the first two and pushed clients into
    // unnecessary `refresh_symbol_index` loops on legitimately empty
    // files (Codex P2 on PR #184).
    if symbols.is_empty() {
        let resolved = state.project().resolve(path).ok();
        let on_disk = resolved
            .as_deref()
            .map(std::path::Path::is_file)
            .unwrap_or(false);
        let extension = resolved
            .as_deref()
            .and_then(std::path::Path::extension)
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let supported = extension
            .as_deref()
            .map(codelens_engine::lang_registry::supports_symbols)
            .unwrap_or(false);
        if on_disk && supported {
            let already_indexed = state.symbol_index().file_is_indexed(path).unwrap_or(false);
            if already_indexed {
                payload["degraded_reason"] = json!("indexed_no_symbols");
                payload["recovery_message"] = json!(
                    "File is indexed but has no top-level symbols (empty file, comments only, \
                     or no declarations the active backend recognises). This is not an error; \
                     no recovery action is required."
                );
            } else {
                payload["degraded_reason"] = json!("file_not_indexed");
                payload["fallback_hint"] = json!(["refresh_symbol_index", "find_symbol"]);
                payload["recovery_message"] = json!(
                    "Result is empty because this file is not in the on-disk index yet \
                     (watcher lag, .gitignore, or project root mismatch). \
                     Call `refresh_symbol_index` with this path, or wait ~1-2s after a recent edit."
                );
            }
        } else if on_disk {
            payload["degraded_reason"] = json!("unsupported_extension");
        }
    }
    insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);

    Ok((payload, success_meta(BackendKind::TreeSitter, 0.93)))
}

pub fn find_symbol(state: &AppState, arguments: &Value) -> ToolResult {
    crate::tools::symbol_query::SymbolQueryPipeline::new(state)
        .run(crate::tools::symbol_query::SymbolQueryRequest::FindSymbol { arguments })
}

pub(crate) fn sparse_symbol_hits_for_query(
    state: &AppState,
    query_analysis: &RetrievalQueryAnalysis,
    max_results: usize,
    include_tests: bool,
    include_generated: bool,
    session: &crate::session_context::SessionRequestContext,
) -> Result<Vec<ScoredSymbol>, CodeLensError> {
    let mut all_symbols = Vec::new();
    for path in state.symbol_index().indexed_file_paths()? {
        if let Ok(symbols) = state.symbol_index().get_symbols_overview_cached(&path, 3) {
            all_symbols.extend(flatten_symbols(&symbols));
        }
    }

    let corpus = build_symbol_corpus(&all_symbols);
    let mut scored = search_symbols_bm25f(
        &corpus,
        &query_analysis.expanded_query,
        max_results.saturating_mul(3).max(max_results),
        include_tests,
        include_generated,
    );

    let recent_files = state.recent_file_paths_for_session(session);
    if !recent_files.is_empty() {
        for hit in &mut scored {
            if recent_files
                .iter()
                .any(|path| hit.document.file_path.starts_with(path))
            {
                hit.score *= 1.08;
            }
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    scored.truncate(max_results);
    Ok(scored)
}

pub fn bm25_symbol_search(state: &AppState, arguments: &Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let query_analysis = analyze_retrieval_query(query);
    let max_results = optional_usize(arguments, "max_results", 10);
    let include_tests = optional_bool(arguments, "include_tests", false);
    let include_generated = optional_bool(arguments, "include_generated", false);
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let scored = sparse_symbol_hits_for_query(
        state,
        &query_analysis,
        max_results,
        include_tests,
        include_generated,
        &session,
    )?;

    let total_query_terms = unique_query_terms(&query_analysis.expanded_query).len();
    let payload_results: Vec<Value> = scored
        .into_iter()
        .enumerate()
        .map(|(idx, hit)| {
            let follow_up = suggested_follow_up(&hit.document.kind, hit.document.exported);
            let confidence = confidence_tier(
                &hit.matched_terms,
                total_query_terms,
                &hit.document.name,
                &hit.document.name_path,
            );
            json!({
                "symbol_id": hit.document.symbol_id,
                "name": hit.document.name,
                "name_path": hit.document.name_path,
                "kind": hit.document.kind,
                "file_path": hit.document.file_path,
                "module_path": hit.document.module_path,
                "signature": hit.document.signature,
                "language": hit.document.language,
                "line": hit.document.line_start,
                "score": ((hit.score * 1000.0).round() / 1000.0),
                "why_matched": hit.matched_terms,
                "flags": {
                    "is_test": hit.document.is_test,
                    "is_generated": hit.document.is_generated,
                    "exported": hit.document.exported,
                },
                "provenance": {
                    "source": "sparse_bm25f",
                    "retrieval_rank": idx + 1,
                },
                "suggested_follow_up": follow_up,
                "confidence": confidence,
            })
        })
        .collect();

    let query_type = if query_analysis.prefer_lexical_only {
        "identifier"
    } else if query_analysis.natural_language {
        "natural_language"
    } else {
        "short_phrase"
    };
    let retrieval = json!({
        "lane": "sparse_bm25f",
        "query_type": query_type,
        "recommended": query_analysis.prefer_sparse_symbol_search,
        "lexical_query": query_analysis.expanded_query,
        "semantic_query": query_analysis.semantic_query,
    });
    let meta = success_meta(BackendKind::Sqlite, 0.88);
    let evidence = crate::tool_evidence::tool_evidence(
        "retrieval",
        &meta,
        "sparse_bm25f",
        json!({
            "preferred_lane": "sparse_bm25f",
            "query_type": query_type,
            "semantic_enabled": false,
            "semantic_used_in_core": false,
            "sparse_used_in_core": true,
            "semantic_evidence_count": 0,
            "sparse_evidence_count": payload_results.len(),
            "precise_available": false,
            "precise_used": false,
            "precise_source": null,
            "fallback_source": "sparse_bm25f",
            "precise_result_count": 0,
        }),
    );

    Ok((
        json!({
            "query": query,
            "results": payload_results,
            "count": payload_results.len(),
            "retrieval": retrieval,
            "evidence": evidence,
        }),
        meta,
    ))
}

/// Scale a base token budget to the host's advertised model context window.
///
/// Returns the smaller of (base × multiplier) and a per-tier ceiling so a
/// 1M-context host doesn't end up with a budget larger than reasonably
/// retrievable evidence, while a 32K host doesn't get pushed over its head.
///
/// Tiers are conservative on purpose. The intent is to widen room when there
/// is room, not to fill the host's window — the host still owns the response
/// and may apply its own truncation downstream.
pub(crate) fn adapt_budget_to_context_window(base: usize, context_window: usize) -> usize {
    let (multiplier, cap) = match context_window {
        n if n >= 1_000_000 => (4.0_f64, 131_072_usize),
        n if n >= 200_000 => (2.0_f64, 65_536_usize),
        n if n >= 32_000 => (1.0_f64, 32_768_usize),
        _ => (0.5_f64, 16_384_usize),
    };
    ((base as f64 * multiplier).round() as usize).min(cap)
}

pub fn get_ranked_context(state: &AppState, arguments: &Value) -> ToolResult {
    SymbolQueryPipeline::new(state).run(SymbolQueryRequest::RankedContext { arguments })
}

pub fn refresh_symbol_index(state: &AppState, _arguments: &Value) -> ToolResult {
    let stats = state.symbol_index().refresh_all()?;
    state.graph_cache().invalidate();
    #[cfg(feature = "semantic")]
    let mut payload = json!(stats);
    #[cfg(not(feature = "semantic"))]
    let payload = json!(stats);
    #[cfg(feature = "semantic")]
    {
        let project = state.project();
        let guard = state.embedding_ref();
        if let Some(engine) = guard.as_ref()
            && engine.is_indexed()
        {
            match engine.ensure_index_fresh_for_project(&project) {
                Ok(report) => {
                    if let Some(map) = payload.as_object_mut() {
                        map.insert("embedding_freshness".to_owned(), json!(report));
                    }
                }
                Err(error) => {
                    if let Some(map) = payload.as_object_mut() {
                        map.insert(
                            "embedding_freshness".to_owned(),
                            json!({
                                "status": "unavailable",
                                "reason": error.to_string()
                            }),
                        );
                    }
                }
            }
        }
    }
    Ok((payload, success_meta(BackendKind::TreeSitter, 0.95)))
}

pub fn get_complexity(state: &AppState, arguments: &Value) -> ToolResult {
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

#[allow(dead_code)]
pub fn get_project_structure(state: &AppState, _arguments: &Value) -> ToolResult {
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

pub fn search_symbols_fuzzy(state: &AppState, arguments: &Value) -> ToolResult {
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

    let pagerank_scores = state.graph_cache().file_pagerank_scores(&state.project());
    let pagerank_ref = if pagerank_scores.is_empty() {
        None
    } else {
        Some(pagerank_scores.as_ref())
    };

    Ok(search_symbols_hybrid_with_semantic(
        &state.project(),
        query,
        max_results,
        fuzzy_threshold,
        sem_ref,
        pagerank_ref,
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

/// Follow-up tool hints for a BM25 symbol card.
///
/// Mirrors the `bm25-sparse-lane-spec` matrix. Frontier-model harnesses
/// select their next tool off this list, so the output is part of the
/// response contract. Keep it short (1-3 entries) — the goal is
/// guidance, not an exhaustive menu.
fn suggested_follow_up(kind: &str, exported: bool) -> Vec<&'static str> {
    let base: Vec<&'static str> = match kind {
        "function" | "method" => vec!["find_symbol", "get_file_diagnostics"],
        "class" | "interface" | "enum" | "type_alias" => {
            vec!["find_symbol", "find_referencing_symbols"]
        }
        "module" | "file" => vec!["get_symbols_overview", "find_referencing_symbols"],
        "variable" | "property" => vec!["find_symbol", "find_referencing_symbols"],
        _ => vec!["find_symbol"],
    };
    if exported
        && matches!(kind, "function" | "method" | "class" | "interface")
        && !base.contains(&"find_referencing_symbols")
    {
        let mut with_refs = base.clone();
        with_refs.push("find_referencing_symbols");
        return with_refs;
    }
    base
}

#[cfg(test)]
mod adapt_budget_tests {
    use super::adapt_budget_to_context_window;

    #[test]
    fn small_window_halves_budget_capped_at_16k() {
        // 8K context — base 32K halved to 16K, capped at 16K floor
        assert_eq!(adapt_budget_to_context_window(32_768, 8_000), 16_384);
        // base 8K halved to 4K — under cap
        assert_eq!(adapt_budget_to_context_window(8_000, 16_000), 4_000);
    }

    #[test]
    fn standard_window_passes_base_capped_at_32k() {
        // 64K window → ×1, cap 32K
        assert_eq!(adapt_budget_to_context_window(16_384, 64_000), 16_384);
        assert_eq!(adapt_budget_to_context_window(40_000, 64_000), 32_768);
    }

    #[test]
    fn large_window_doubles_budget_capped_at_64k() {
        // 200K → ×2 cap 64K
        assert_eq!(adapt_budget_to_context_window(16_384, 200_000), 32_768);
        assert_eq!(adapt_budget_to_context_window(50_000, 200_000), 65_536);
    }

    #[test]
    fn xl_window_quadruples_budget_capped_at_128k() {
        // 1M → ×4 cap 128K
        assert_eq!(adapt_budget_to_context_window(16_384, 1_000_000), 65_536);
        assert_eq!(adapt_budget_to_context_window(40_000, 1_000_000), 131_072);
    }

    #[test]
    fn boundary_at_32k_uses_standard_tier() {
        // exactly 32K → standard tier (×1, cap 32K), not small tier
        assert_eq!(adapt_budget_to_context_window(16_384, 32_000), 16_384);
    }

    #[test]
    fn boundary_at_200k_uses_large_tier() {
        // exactly 200K → large tier (×2, cap 64K)
        assert_eq!(adapt_budget_to_context_window(16_384, 200_000), 32_768);
    }
}

#[cfg(test)]
mod suggested_follow_up_tests {
    use super::suggested_follow_up;

    #[test]
    fn function_gets_body_then_diagnostics() {
        let hints = suggested_follow_up("function", false);
        assert_eq!(hints.first().copied(), Some("find_symbol"));
        assert!(hints.contains(&"get_file_diagnostics"));
    }

    #[test]
    fn class_gets_body_and_references() {
        let hints = suggested_follow_up("class", false);
        assert_eq!(hints, vec!["find_symbol", "find_referencing_symbols"]);
    }

    #[test]
    fn module_gets_overview_first() {
        let hints = suggested_follow_up("module", false);
        assert_eq!(hints.first().copied(), Some("get_symbols_overview"));
    }

    #[test]
    fn exported_function_also_offers_references() {
        let hints = suggested_follow_up("function", true);
        assert!(hints.contains(&"find_referencing_symbols"));
        assert!(hints.contains(&"find_symbol"));
    }

    #[test]
    fn unknown_kind_falls_back_to_find_symbol() {
        let hints = suggested_follow_up("unknown", false);
        assert_eq!(hints, vec!["find_symbol"]);
    }
}

/// Cross-field confidence tier for a BM25 symbol card.
///
/// Without a separate dense arm, we cannot yet compute a true
/// BM25-vs-dense agreement signal. This heuristic is the *cross-field*
/// proxy: a result that matches query terms on the high-weight
/// identifier fields (`name`, `name_path`) **and** covers most of the
/// unique query terms is a high-confidence hit; a result that matches
/// only on low-weight fields (body lexical chunk, doc comment) is low.
///
/// - `high`   — ≥80% query-term coverage AND a hit on name or name_path
/// - `medium` — 2+ matched terms OR a name/name_path hit
/// - `low`    — single term hit, or matches only on body/doc fields
///
/// Frontier-model callers use this to decide whether to trust the card
/// for direct consumption or to cross-check via `find_symbol` +
/// `find_referencing_symbols` before acting.
fn confidence_tier(
    matched_terms: &[String],
    unique_query_terms: usize,
    name: &str,
    name_path: &str,
) -> &'static str {
    if matched_terms.is_empty() || unique_query_terms == 0 {
        return "low";
    }
    let coverage = matched_terms.len() as f64 / unique_query_terms as f64;
    let name_lower = name.to_ascii_lowercase();
    let name_path_lower = name_path.to_ascii_lowercase();
    let identifier_hit = matched_terms.iter().any(|term| {
        let term_lower = term.to_ascii_lowercase();
        name_lower.contains(&term_lower) || name_path_lower.contains(&term_lower)
    });

    if coverage >= 0.8 && identifier_hit {
        "high"
    } else if identifier_hit || matched_terms.len() >= 2 {
        "medium"
    } else {
        "low"
    }
}

#[cfg(test)]
mod confidence_tier_tests {
    use super::confidence_tier;

    #[test]
    fn full_coverage_on_name_path_is_high() {
        let matched = vec!["dispatch".to_owned(), "tool".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 2, "dispatch_tool", "dispatch::dispatch_tool"),
            "high"
        );
    }

    #[test]
    fn partial_coverage_with_name_hit_is_medium() {
        let matched = vec!["dispatch".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 3, "dispatch_tool", "dispatch::dispatch_tool"),
            "medium"
        );
    }

    #[test]
    fn body_only_match_is_low() {
        let matched = vec!["invoke".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 2, "dispatch_tool", "dispatch::dispatch_tool"),
            "low"
        );
    }

    #[test]
    fn multiple_matches_without_name_hit_is_medium() {
        let matched = vec!["invoke".to_owned(), "handler".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 3, "dispatch_tool", "dispatch::dispatch_tool"),
            "medium"
        );
    }

    #[test]
    fn empty_matched_is_low() {
        assert_eq!(confidence_tier(&[], 2, "x", "a::x"), "low");
    }

    #[test]
    fn zero_query_terms_is_low() {
        let matched = vec!["dispatch".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 0, "dispatch_tool", "dispatch::dispatch_tool"),
            "low"
        );
    }
}
