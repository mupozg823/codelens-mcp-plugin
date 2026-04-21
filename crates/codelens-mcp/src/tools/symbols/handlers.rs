use super::super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::analyze_retrieval_query, required_string, success_meta,
};
use super::{
    analyzer::{
        annotate_ranked_context_provenance, compact_semantic_evidence,
        merge_semantic_ranked_entries, semantic_results_for_query, semantic_scores_for_query,
    },
    formatter::{count_branches, render_symbols_with_presentation},
    support::{confidence_tier, flatten_symbols, suggested_follow_up},
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::retrieval::symbols::{build_symbol_corpus, search_symbols_bm25f, unique_query_terms};
use crate::tools::lsp::lsp_boost_probe;
use codelens_engine::{SymbolKind, read_file, search_symbols_hybrid_with_semantic};
use serde_json::{Value, json};

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

    // Phase O1 — per-symbol compression level caps. `body_cap` limits
    // how many top symbols get L2 (signature + body) when
    // `include_body` is true. `presentation_cap` limits how many
    // symbols keep at least L1 (signature); beyond this cap they drop
    // to L0 (id + file + line only). Both caps are overridable via
    // `_body_cap` / `_presentation_cap` test-time knobs, defaulting to
    // values that preserve the previous "top-3 get bodies, everyone
    // else keeps signatures" behavior for real clients.
    let body_cap = optional_usize(arguments, "_body_cap", 3);
    let presentation_cap =
        optional_usize(arguments, "_presentation_cap", max_matches.max(body_cap));

    Ok(state
        .symbol_index()
        .find_symbol_cached(name, file_path, include_body, exact_match, max_matches)
        .map(|value| {
            let (rendered_symbols, presentation_stats) = render_symbols_with_presentation(
                &value,
                include_body,
                body_cap,
                presentation_cap,
                body_line_limit,
                body_char_limit,
                body_full,
            );
            let bodies_full_count = presentation_stats.signature_body_full;
            let bodies_truncated_count = presentation_stats.signature_body_truncated;
            let bodies_missing_count = presentation_stats.id_only + presentation_stats.signature;
            let body_delivery = if !include_body {
                json!({"requested": false, "status": "disabled"})
            } else {
                let status = if bodies_missing_count == 0 && bodies_truncated_count == 0 {
                    "full"
                } else if bodies_missing_count > 0 && bodies_full_count == 0
                    && bodies_truncated_count == 0
                {
                    "dropped"
                } else {
                    "partial"
                };
                json!({
                    "requested": true,
                    "status": status,
                    "bodies_full": bodies_full_count,
                    "bodies_truncated": bodies_truncated_count,
                    "bodies_omitted_over_cap": bodies_missing_count,
                    "max_symbols_with_body": if body_full { value.len() } else { body_cap },
                    "line_limit": if body_full { 0 } else { body_line_limit },
                    "char_limit": if body_full { 0 } else { body_char_limit },
                    "hint": if !body_full && (bodies_truncated_count > 0 || bodies_missing_count > 0) {
                        "pass body_full=true for untruncated bodies, or narrow the query (file_path/exact_match) to fit within the default cap"
                    } else {
                        ""
                    },
                })
            };
            let mut payload = json!({
                "symbols": rendered_symbols,
                "count": value.len(),
                "body_truncated_count": bodies_truncated_count,
                "body_preview": include_body && !body_full,
                "body_delivery": body_delivery,
                "presentation_summary": {
                    "id_only": presentation_stats.id_only,
                    "signature": presentation_stats.signature,
                    "signature_body_full": presentation_stats.signature_body_full,
                    "signature_body_truncated": presentation_stats.signature_body_truncated,
                    "body_cap": body_cap,
                    "presentation_cap": presentation_cap,
                },
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
    let semantic_requested =
        !disable_semantic && !query_analysis.prefer_lexical_only && !exact_identifier_projection;
    #[cfg(feature = "semantic")]
    if semantic_requested {
        // `embedding_status()` is intentionally read-only and reports
        // cold disk indexes as not-ready. For actual hybrid retrieval,
        // warm the engine on demand so an indexed project can use the
        // semantic lane without requiring a prior `semantic_search` call.
        drop(state.embedding_engine());
    }
    let effective_disable_semantic =
        disable_semantic || query_analysis.prefer_lexical_only || exact_identifier_projection;
    // Semantic lane readiness: the embedding index must be warm AND
    // the feature must be compiled in. When cold, the handler used to
    // keep `semantic_enabled=true` in the response envelope even
    // though every symbol's `provenance.semantic_score` came back
    // `null`. The harness had no way to tell that semantic actually
    // contributed nothing — so it made downstream decisions under a
    // false premise. Fold readiness into the effective flag here.
    let semantic_ready = super::semantic_lane_ready(state);
    let effective_disable_semantic = effective_disable_semantic || !semantic_ready;
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
                // `semantic_ready` is orthogonal to `semantic_enabled`:
                // the former reports whether the embedding index is
                // warm (structural readiness of the lane), the latter
                // reports whether the lane actually contributed to
                // this call (caller may have disabled it, query may
                // be an identifier, etc). Exposing both lets the
                // harness distinguish "turned off by policy" from
                // "lane is cold, warm me up".
                "semantic_ready": semantic_ready,
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
    // Semantic index cold / caller did not disable semantic, but the
    // lane could not contribute. Two triggers:
    //   a) lane produced zero evidence even though it ran
    //      (`!effective_disable_semantic`), or
    //   b) lane never ran because the index is cold (`!semantic_ready`)
    //      and the caller had not turned semantic off.
    //      Previously (b) was hidden because the envelope still
    //      advertised `semantic_enabled=true`; now we downgrade the
    //      envelope AND surface the cold index as a decision so the
    //      harness knows a warmup would change the answer.
    if (!effective_disable_semantic && semantic_results.is_empty())
        || (semantic_requested && !semantic_ready)
    {
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
