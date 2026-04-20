use super::super::{
    default_lsp_args_for_command, default_lsp_command_for_path, optional_bool, optional_string,
    optional_usize, query_analysis::analyze_retrieval_query, required_string, success_meta,
    AppState, ToolResult,
};
use super::{
    analyzer::{
        annotate_ranked_context_provenance, compact_semantic_evidence,
        merge_semantic_ranked_entries, semantic_results_for_query, semantic_scores_for_query,
    },
    formatter::{compact_symbol_bodies, count_branches},
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{search_symbols_bm25f, unique_query_terms};
use codelens_engine::{
    find_referencing_symbols_via_text, read_file, search_symbols_hybrid_with_semantic, LspRequest,
    SymbolInfo, SymbolKind,
};
use serde_json::{json, Value};

pub fn get_symbols_overview(state: &AppState, arguments: &Value) -> ToolResult {
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

    let mut payload = json!({
        "symbols": symbols,
        "count": symbols.len(),
        "truncated": truncated,
        "auto_summarized": stripped,
    });
    let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
    if stripped || truncated {
        let param = if explicit_depth.is_some() {
            format!("depth={depth}")
        } else {
            format!(
                "depth=auto (default 1, hit at {}-char budget)",
                budget_chars
            )
        };
        decisions.push(crate::limits::LimitsApplied::depth_limit(param));
    }
    let mut meta = success_meta(BackendKind::TreeSitter, 0.93);
    crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, decisions);
    Ok((payload, meta))
}

pub fn find_symbol(state: &AppState, arguments: &Value) -> ToolResult {
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
    // Try SCIP precise definitions first (if available), then tree-sitter.
    #[cfg(feature = "scip-backend")]
    if let Some(backend) = state.scip() {
        use codelens_engine::PreciseBackend as _;
        let scip_file = file_path.unwrap_or("");
        if let Ok(defs) = backend.find_definitions(name, scip_file, 0) {
            if !defs.is_empty() {
                let limited: Vec<_> = defs.into_iter().take(max_matches).collect();
                let count = limited.len();
                let syms: Vec<serde_json::Value> = limited
                    .iter()
                    .map(|d| {
                        // Enrich with hover documentation from SCIP if available.
                        let doc = backend
                            .hover(&d.file_path, d.line, 0)
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        let mut sym = json!({
                            "name": d.name,
                            "kind": d.kind,
                            "file_path": d.file_path,
                            "line": d.line,
                            "signature": if d.signature.is_empty() { &doc } else { &d.signature },
                            "name_path": d.name_path,
                            "score": d.score,
                        });
                        if !doc.is_empty() {
                            sym["documentation"] = serde_json::Value::String(doc);
                        }
                        sym
                    })
                    .collect();
                let mut payload = json!({
                    "symbols": syms,
                    "count": count,
                    "body_truncated_count": 0,
                    "body_preview": false,
                    "backend": "scip",
                });
                let mut meta = success_meta(BackendKind::Scip, 0.98);
                crate::tools::transparency::attach_decisions_to_meta(
                    &mut payload,
                    &mut meta,
                    Vec::new(),
                );
                return Ok((payload, meta));
            }
        }
    }

    Ok(state
        .symbol_index()
        .find_symbol_cached(name, file_path, include_body, exact_match, max_matches)
        .map(|mut value| {
            let body_truncated_count = if include_body && !body_full {
                compact_symbol_bodies(&mut value, 3, body_line_limit, body_char_limit)
            } else {
                0
            };
            // 0-result fallback hint: agents guessing a slightly wrong name
            // hit dead-ends silently otherwise. Recommend the fuzzy path.
            let mut payload = json!({
                "symbols": value,
                "count": value.len(),
                "body_truncated_count": body_truncated_count,
                "body_preview": include_body && !body_full,
            });
            let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
            if value.is_empty() {
                if let Some(map) = payload.as_object_mut() {
                    map.insert(
                        "fallback_hint".to_owned(),
                        json!({
                            "reason": "no exact match",
                            "query": name,
                            "try": [
                                {
                                    "tool": "search_workspace_symbols",
                                    "arguments": {"query": name, "limit": 10},
                                    "why": "fuzzy / partial-name search across the full symbol index",
                                },
                                {
                                    "tool": "search_symbols_fuzzy",
                                    "arguments": {"query": name, "max_results": 10},
                                    "why": "alternate fuzzy matcher with score ranking",
                                },
                                {
                                    "tool": "bm25_symbol_search",
                                    "arguments": {"query": name, "max_results": 10},
                                    "why": "NL / identifier-token retrieval when the exact name is uncertain",
                                },
                            ],
                        }),
                    );
                }
                decisions.push(crate::limits::LimitsApplied::exact_match_only(name));
            }
            let mut meta = success_meta(BackendKind::TreeSitter, 0.93);
            crate::tools::transparency::attach_decisions_to_meta(
                &mut payload,
                &mut meta,
                decisions,
            );
            (payload, meta)
        })?)
}

pub fn bm25_symbol_search(state: &AppState, arguments: &Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let query_analysis = analyze_retrieval_query(query);
    let max_results = optional_usize(arguments, "max_results", 10);
    let include_tests = optional_bool(arguments, "include_tests", false);
    let include_generated = optional_bool(arguments, "include_generated", false);
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let recent_files = state.recent_file_paths_for_session(&session);

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

    Ok((
        json!({
            "query": query,
            "results": payload_results,
            "count": payload_results.len(),
            "retrieval": {
                "lane": "sparse_bm25f",
                "query_type": if query_analysis.prefer_lexical_only {
                    "identifier"
                } else if query_analysis.natural_language {
                    "natural_language"
                } else {
                    "short_phrase"
                },
                "recommended": query_analysis.prefer_sparse_symbol_search,
                "lexical_query": query_analysis.expanded_query,
                "semantic_query": query_analysis.semantic_query,
            }
        }),
        success_meta(BackendKind::Sqlite, 0.88),
    ))
}

pub fn get_ranked_context(state: &AppState, arguments: &Value) -> ToolResult {
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
    // P1-4 caller wiring: opt-in LSP reference boost. When `true`, the
    // handler asks the LSP pool for `textDocument/references` on the
    // query target, collects the hit files, and feeds them through
    // `get_ranked_context_cached_with_lsp_boost`. The probe is
    // best-effort — if no `path` was supplied, if no LSP command is
    // available, or if the LSP call fails, the handler silently falls
    // back to the default (empty-boost) path so the response is
    // byte-identical to `lsp_boost=false`. Default `false` preserves
    // the existing benchmark envelope.
    let lsp_boost = optional_bool(arguments, "lsp_boost", false);
    let exact_identifier_projection = query_analysis.original_query
        != query_analysis.expanded_query
        && !query_analysis.expanded_query.contains(char::is_whitespace);
    let effective_disable_semantic =
        disable_semantic || query_analysis.prefer_lexical_only || exact_identifier_projection;
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
    let (lsp_boost_refs, lsp_signal_weight) = if lsp_boost {
        lsp_boost_probe(state, query, path)
    } else {
        (std::collections::HashMap::new(), None)
    };
    let mut result = state
        .symbol_index()
        .get_ranked_context_cached_with_lsp_boost(
            &query_analysis.expanded_query,
            path,
            max_tokens,
            include_body,
            depth,
            Some(&state.graph_cache()),
            boosted_scores,
            None,
            lsp_boost_refs,
            lsp_signal_weight,
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
                "preferred_lane": if query_analysis.prefer_sparse_symbol_search {
                    "sparse_bm25f"
                } else if effective_disable_semantic {
                    "structural_lexical"
                } else {
                    "hybrid_semantic"
                },
                "sparse_lane_recommended": query_analysis.prefer_sparse_symbol_search,
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

    let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
    if result.pruned_count > 0 {
        let returned = result.symbols.len();
        let total = returned + result.pruned_count;
        decisions.push(crate::limits::LimitsApplied::budget_prune(
            returned,
            total,
            result.last_kept_score,
            format!("max_tokens={max_tokens}"),
        ));
    }
    // Semantic index cold: caller did NOT disable semantic, but the
    // engine produced zero semantic evidence. Agents relying on
    // natural-language retrieval benefit from seeing this.
    if !effective_disable_semantic && semantic_results.is_empty() {
        decisions.push(crate::limits::LimitsApplied::index_partial("semantic"));
    }

    let mut meta = success_meta(backend, 0.91);
    crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, decisions);
    Ok((payload, meta))
}

pub fn refresh_symbol_index(state: &AppState, _arguments: &Value) -> ToolResult {
    let stats = state.symbol_index().refresh_all()?;
    state.graph_cache().invalidate();
    Ok((json!(stats), success_meta(BackendKind::TreeSitter, 0.95)))
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

/// P1-4 caller wiring: run a best-effort reference probe for `query`
/// anchored at `path` and turn the hit files into a boost set.
///
/// Strategy mirrors `find_referencing_symbols` with `union=true`: try
/// LSP `textDocument/references` first, then merge tree-sitter text
/// references. The tree-sitter pass is the key fallback — LSP is
/// commonly cold (returning 0 refs for 5-30s on rust-analyzer /
/// pyright), and without a fallback the boost would be silently inert
/// on every cold CLI invocation. The union gives us a populated set
/// whenever *either* backend resolves anything, so the P1-4 wiring
/// meets the caller regardless of LSP readiness. Matches the
/// `benchmarks/results/v1.9.46-lsp-reference-precision.json` finding
/// that tree-sitter refs are a useful floor, not a ceiling.
///
/// Returns `(files, weight)`. Empty set means the caller should fall
/// through to the default no-op path; the weight is read from
/// `CODELENS_LSP_SIGNAL_WEIGHT` (default `0.25`).
fn lsp_boost_probe(
    state: &AppState,
    query: &str,
    path: Option<&str>,
) -> (std::collections::HashMap<String, Vec<usize>>, Option<f64>) {
    let empty = (std::collections::HashMap::new(), None);
    let Some(path) = path else {
        return empty;
    };

    let mut refs_by_file: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();

    // LSP leg — optional, only fires when an LSP command is available
    // and the query resolves to an anchor symbol inside `path`.
    if let Some(command) = default_lsp_command_for_path(path) {
        let anchor = match state
            .symbol_index()
            .find_symbol(query, Some(path), false, true, 1)
        {
            Ok(rows) => rows.into_iter().next().map(|s| (s.line, s.column)),
            Err(_) => None,
        };
        if let Some((line, column)) = anchor {
            let request = LspRequest {
                command: command.clone(),
                args: default_lsp_args_for_command(&command),
                file_path: path.to_owned(),
                line,
                column,
                max_results: 64,
            };
            if let Ok(refs) = state.lsp_pool().find_referencing_symbols(&request) {
                for r in refs {
                    refs_by_file.entry(r.file_path).or_default().push(r.line);
                }
            }
        }
    }

    // Tree-sitter leg — always attempted; populates callers that LSP
    // has not yet surfaced (e.g. cold rust-analyzer) or cannot see
    // (languages without an installed LSP recipe). `find_symbol`
    // guarantees the query string is a real symbol name in `path`, so
    // the tree-sitter text search is well-anchored.
    if let Ok(report) = find_referencing_symbols_via_text(&state.project(), query, Some(path), 128)
    {
        for r in report.references {
            refs_by_file.entry(r.file_path).or_default().push(r.line);
        }
    }

    if refs_by_file.is_empty() {
        return empty;
    }
    let weight = std::env::var("CODELENS_LSP_SIGNAL_WEIGHT")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.25);
    (refs_by_file, Some(weight))
}
