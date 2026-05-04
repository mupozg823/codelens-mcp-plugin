use super::super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::{RetrievalQueryAnalysis, analyze_retrieval_query},
    required_string, success_meta,
};
use super::{
    analyzer::{
        annotate_ranked_context_provenance, compact_semantic_evidence, compact_sparse_evidence,
        merge_semantic_ranked_entries, merge_sparse_ranked_entries, semantic_results_for_query,
        semantic_scores_for_query,
    },
    formatter::{compact_symbol_bodies, count_branches},
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{ScoredSymbol, search_symbols_bm25f, unique_query_terms};
use codelens_engine::{SymbolInfo, SymbolKind, read_file, search_symbols_hybrid_with_semantic};
use serde_json::{Value, json};

#[cfg(feature = "scip-backend")]
const HEURISTIC_BODY_LINES: usize = 50;
const PATH_ALIAS_DEPRECATION: &str =
    "DEPRECATED v1.13.23 — use `path`. Soft alias maintained until v1.14.0.";

fn path_alias_warning(alias: &str) -> Value {
    json!({
        "param": alias,
        "replacement": "path",
        "message": PATH_ALIAS_DEPRECATION,
    })
}

fn resolve_path_argument(arguments: &Value) -> Result<(&str, Vec<Value>), CodeLensError> {
    if let Some(path) = optional_string(arguments, "path") {
        if let Some(alias @ ("file_path" | "relative_path")) =
            optional_string(arguments, "_path_alias_source")
        {
            return Ok((path, vec![path_alias_warning(alias)]));
        }
        return Ok((path, Vec::new()));
    }
    for alias in ["file_path", "relative_path"] {
        if let Some(path) = optional_string(arguments, alias) {
            return Ok((path, vec![path_alias_warning(alias)]));
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

#[cfg(feature = "scip-backend")]
fn heuristic_body_slice(state: &AppState, file_path: &str, line: usize) -> Option<String> {
    read_file(
        &state.project(),
        file_path,
        Some(line),
        Some(line.saturating_add(HEURISTIC_BODY_LINES)),
    )
    .ok()
    .map(|file| file.content)
    .filter(|body| !body.is_empty())
}

/// Issue #235 (sub-fix B): when SCIP returns a definition occurrence with
/// neither `d.signature` nor a usable hover string, fall back to reading
/// the single source line at the SCIP-reported position. Empty trimmed
/// lines (blank lines, attribute-only lines) yield `None` so the caller
/// can surface `"signature_source": "unavailable"` instead of a misleading
/// blank string.
///
/// Skip this fallback when the file is known to be SCIP-stale — the
/// SCIP-reported `line` would point at unrelated source after the index
/// drifted, making the read worse than an empty signature.
#[cfg(feature = "scip-backend")]
pub(super) fn read_signature_line(
    state: &AppState,
    file_path: &str,
    line: usize,
) -> Option<String> {
    // Matches `heuristic_body_slice`: `line` is treated as the 0-indexed
    // first row in the file (same convention as the SCIP `parse_range`
    // return value). `read_file` slices `lines[start..end]`, so reading
    // exactly one row needs an end of `line + 1`.
    let file = read_file(
        &state.project(),
        file_path,
        Some(line),
        Some(line.saturating_add(1)),
    )
    .ok()?;
    let trimmed = file.content.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

// Issue #240: SCIP staleness detection promoted to `tools/scip_health.rs`
// so `find_referencing_symbols` and `get_callers` can reuse the same
// probe + warning shape.
#[cfg(feature = "scip-backend")]
use crate::tools::scip_health::{
    detect_scip_staleness, scip_line_to_display, scip_stale_warning_payload,
};

/// Issue #235 (sub-fix C): humanize raw SCIP descriptors (e.g.
/// `"rust-analyzer cargo codelens-mcp 1.9.59 tools/session/project_ops/prepare_harness_session()."`)
/// before exposing them as `name_path`. Strips the
/// `<emitter> <pkg-mgr> <crate> <version> ` preamble and the trailing
/// `()` / `#` / `.` SCIP suffixes so callers get a stable tree-sitter-
/// shaped path. The raw descriptor is preserved separately under
/// `scip_descriptor` for debug / reverse-lookup. Falls back to the raw
/// value when the input shape is not recognised, so we never silently
/// drop information.
#[cfg(feature = "scip-backend")]
pub(super) fn humanize_scip_name_path(raw: &str) -> String {
    // SCIP descriptor format (sourcegraph spec):
    //   <emitter> <pkg-mgr> <crate> <version> <descriptor>
    // The four space-separated header fields are followed by a single
    // descriptor segment. After the 4th space is the path-ish part we
    // want to surface; before it is toolchain noise.
    let trimmed = raw.trim();
    let mut path_part = trimmed;
    if trimmed.split(' ').take(4).count() == 4
        && let Some(rest_start) = trimmed.match_indices(' ').nth(3).map(|(idx, _)| idx + 1)
        && rest_start < trimmed.len()
    {
        path_part = &trimmed[rest_start..];
    }
    // Strip trailing SCIP suffixes:
    //   `()`/`().` → function, `#`/`#.` → type, `.` → constant/module.
    let stripped = path_part
        .trim_end_matches('.')
        .trim_end_matches(')')
        .trim_end_matches('(')
        .trim_end_matches('#')
        .trim_end_matches('.');
    if stripped.is_empty() {
        raw.to_owned()
    } else {
        stripped.to_owned()
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
    // P1-B — `find_symbol`'s canonical limit field is `max_matches`,
    // not `max_results`, but agents typing `limit`/`top_k` mean the
    // same thing. See docs/design/arg-validation-policy.md.
    const KNOWN_ARGS: &[&str] = &[
        "symbol_id",
        "name",
        "file_path",
        "path",
        "include_body",
        "exact_match",
        "max_matches",
        "limit",
        "top_k",
        "body_full",
        "body_line_limit",
        "body_char_limit",
        "name_path", // legacy alias for `name`; deprecated since v1.13.23
    ];
    let symbol_id = optional_string(arguments, "symbol_id");
    let name_path_alias = optional_string(arguments, "name_path");
    let mut deprecation_warnings: Vec<String> = Vec::new();
    if name_path_alias.is_some()
        && optional_string(arguments, "name").is_none()
        && symbol_id.is_none()
    {
        deprecation_warnings
            .push("`name_path` is deprecated; use `name` (will be removed in v1.14.0)".to_owned());
    }
    let name = symbol_id
        .or_else(|| optional_string(arguments, "name"))
        .or(name_path_alias)
        .ok_or_else(|| CodeLensError::MissingParam("symbol_id or name".into()))?;
    let file_path = optional_string(arguments, "file_path");
    // Issue #203 (3): historically a directory `file_path` slipped through and
    // returned `{ symbols: [], count: 0 }` with the no-exact-match fallback
    // hint, which reads as "the symbol doesn't exist" rather than "you gave
    // me the wrong input shape". Reject directory inputs up front and steer
    // the caller to an alternative whose schema actually accepts a directory.
    if let Some(path_str) = file_path {
        let project_relative = state.project().as_path().join(path_str);
        if project_relative.is_dir() || std::path::Path::new(path_str).is_dir() {
            return Err(crate::error::CodeLensError::Validation(format!(
                "find_symbol received a directory `file_path` `{path_str}`; pass a single file path instead. For directory-scope symbol scans use `get_symbols_overview(path: \"{path_str}\")` for an AST tree, or `bm25_symbol_search(query: \"{name}\")` for a project-wide name search."
            )));
        }
    }
    let include_body = optional_bool(arguments, "include_body", false);
    let exact_match = optional_bool(arguments, "exact_match", false);
    let max_matches = crate::tool_runtime::optional_usize_with_aliases(
        arguments,
        "max_matches",
        &["limit", "top_k"],
        50,
    );
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let body_full = optional_bool(arguments, "body_full", false);
    let body_line_limit = optional_usize(arguments, "body_line_limit", 12);
    let body_char_limit = optional_usize(arguments, "body_char_limit", 600);
    #[cfg(feature = "scip-backend")]
    let scip_backend = state.scip();
    #[cfg(feature = "scip-backend")]
    let precise_available = scip_backend.is_some();
    #[cfg(feature = "scip-backend")]
    let precise_source = precise_available.then_some("scip");
    #[cfg(not(feature = "scip-backend"))]
    let precise_available = false;
    #[cfg(not(feature = "scip-backend"))]
    let precise_source: Option<&str> = None;
    // Try SCIP precise definitions first (if available), then tree-sitter.
    #[cfg(feature = "scip-backend")]
    if let Some(backend) = scip_backend {
        use codelens_engine::PreciseBackend as _;
        let scip_file = file_path.unwrap_or("");
        if let Ok(defs) = backend.find_definitions(name, scip_file, 0)
            && !defs.is_empty()
        {
            let limited: Vec<_> = defs.into_iter().take(max_matches).collect();
            let count = limited.len();
            // Issue #235: SCIP-backed answers carry the precise-tier 0.98
            // confidence label even when the on-disk index pre-dates one or
            // more of the resolved source files — the exact silent-miss
            // shape that makes reviewers act on stale line numbers /
            // bodies. Detect per-file staleness now, and degrade meta +
            // surface a structured warning if any resolved file is newer
            // than the index.
            let scip_candidate_files: Vec<String> =
                limited.iter().map(|d| d.file_path.clone()).collect();
            let scip_staleness =
                detect_scip_staleness(state.project().as_path(), &scip_candidate_files);
            let (meta, confidence_basis) = if scip_staleness.is_some() {
                (
                    crate::tool_evidence::meta_degraded("scip", 0.55, "scip_index_stale_vs_source"),
                    "scip_precise_stale_index",
                )
            } else {
                (success_meta(BackendKind::Scip, 0.98), "scip_precise")
            };
            let evidence = crate::tool_evidence::tool_evidence(
                "symbol",
                &meta,
                confidence_basis,
                crate::tool_evidence::precision_signals(true, true, Some("scip"), None, count),
            );
            // Issue #235 (sub-fix B): build a fast lookup of files whose
            // SCIP-reported line is suspect, so the per-symbol enrichment
            // below knows when to skip the source-line fallback (reading
            // the wrong line is worse than returning an empty signature).
            let stale_file_set: std::collections::HashSet<&str> = scip_staleness
                .as_ref()
                .map(|s| s.stale_files.iter().map(|(f, _)| f.as_str()).collect())
                .unwrap_or_default();
            let syms: Vec<serde_json::Value> = limited
                .iter()
                .map(|d| {
                    // Enrich with hover documentation from SCIP if available.
                    let doc = backend
                        .hover(&d.file_path, d.line, 0)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    // Pick the first non-empty signature source, recording
                    // which path provided it so reviewers can branch on
                    // signal quality instead of guessing.
                    let (signature_value, signature_source) = if !d.signature.is_empty() {
                        (d.signature.clone(), "scip_signature")
                    } else if !doc.is_empty() {
                        (doc.clone(), "scip_doc_hover")
                    } else if !stale_file_set.contains(d.file_path.as_str())
                        && let Some(line) = read_signature_line(state, &d.file_path, d.line)
                    {
                        (line, "source_line_read")
                    } else {
                        (String::new(), "unavailable")
                    };
                    // Issue #235 (sub-fix C): humanize the SCIP descriptor
                    // before exposing it as `name_path`, but keep the raw
                    // descriptor under `scip_descriptor` so debug /
                    // reverse-lookup callers don't lose information.
                    let scip_descriptor_raw = d.name_path.clone().unwrap_or_else(|| d.name.clone());
                    let humanized_name_path = humanize_scip_name_path(&scip_descriptor_raw);
                    // Issue #243: SCIP `parse_range` returns 0-indexed line
                    // numbers (per spec) but the rest of the CodeLens
                    // surface (tree-sitter `get_symbols_overview`,
                    // `read_file`, grep, IDE) is 1-indexed. Normalize at
                    // the JSON serialization boundary so cross-tool
                    // comparison stops needing a -1 fudge. The raw
                    // 0-indexed `d.line` is still passed to
                    // `read_signature_line` and `heuristic_body_slice`
                    // since both slice file content using `Vec<&str>`
                    // indices and need the original convention.
                    let display_line = scip_line_to_display(d.line);
                    let mut sym = json!({
                        "name": d.name,
                        "kind": d.kind,
                        "file_path": d.file_path,
                        "line": display_line,
                        "signature": signature_value,
                        "signature_source": signature_source,
                        "name_path": humanized_name_path,
                        "scip_descriptor": scip_descriptor_raw,
                        "score": d.score,
                    });
                    if !doc.is_empty() {
                        sym["documentation"] = serde_json::Value::String(doc);
                    }
                    if include_body
                        && let Some(body) = heuristic_body_slice(state, &d.file_path, d.line)
                    {
                        sym["body"] = Value::String(body);
                        sym["body_source"] = Value::String("scip_line_range_slice".to_owned());
                        sym["body_truncation"] = Value::String("heuristic_50_lines".to_owned());
                    }
                    sym
                })
                .collect();
            let mut payload = json!({
                "symbols": syms,
                "count": count,
                "body_truncated_count": 0,
                "body_preview": include_body,
                "backend": "scip",
                "evidence": evidence,
            });
            if let Some(map) = payload.as_object_mut() {
                map.insert(
                    "deprecation_warnings".to_owned(),
                    json!(deprecation_warnings),
                );
                if let Some(stale) = scip_staleness.as_ref() {
                    map.insert(
                        "scip_index_stale_warning".to_owned(),
                        scip_stale_warning_payload(stale),
                    );
                }
                if !unknown_args.is_empty() {
                    map.insert(
                        "warnings".to_owned(),
                        json!([format!("unknown args ignored: {:?}", unknown_args)]),
                    );
                }
            }
            return Ok((payload, meta));
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
            if value.is_empty()
                && let Some(map) = payload.as_object_mut()
            {
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
            let meta = success_meta(BackendKind::TreeSitter, 0.93);
            if let Some(map) = payload.as_object_mut() {
                map.insert(
                    "evidence".to_owned(),
                    crate::tool_evidence::tool_evidence(
                        "symbol",
                        &meta,
                        "tree_sitter_symbol_index",
                        crate::tool_evidence::precision_signals(
                            precise_available,
                            false,
                            precise_source,
                            Some("tree_sitter"),
                            0,
                        ),
                    ),
                );
                map.insert("deprecation_warnings".to_owned(), json!(deprecation_warnings));
                if !unknown_args.is_empty() {
                    map.insert(
                        "warnings".to_owned(),
                        json!([format!("unknown args ignored: {:?}", unknown_args)]),
                    );
                }
            }
            (payload, meta)
        })?)
}

fn sparse_symbol_hits_for_query(
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
    // P1-B — surface unknown_args. No `limit`/`top_k` alias here:
    // get_ranked_context's relevant control is `depth` (graph
    // expansion), not a top-N. See docs/design/arg-validation-policy.md.
    const KNOWN_ARGS: &[&str] = &[
        "query",
        "path",
        "file_path",
        "max_tokens",
        "context_window",
        "include_body",
        "depth",
        "disable_semantic",
        "expand_query",
        "session_id",
        "logical_session_id",
        "harness_phase",
        "lsp_boost",
    ];
    let query = required_string(arguments, "query")?;
    let query_analysis = analyze_retrieval_query(query);
    let path = optional_string(arguments, "path");
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    // v1.10.1 floor: when the user does not supply `max_tokens`, take the
    // larger of the surface token budget and 16K. The active surface budget
    // is intentionally tight (8K on `preset:full`, 4K on
    // `refactor-full`), but hybrid retrieval (semantic + sparse +
    // structural evidence) routinely exceeds that, triggering Stage 5
    // truncation. See `docs/eval/v1.10.0-post-release-eval.md` (F3).
    const HYBRID_RETRIEVAL_FLOOR: usize = 16384;
    // v1.13.18 adaptive: when the host advertises its model context window
    // (e.g. 1M for Opus 4.7, 200K for Sonnet 4.6, 32K for older models),
    // scale the budget so we don't waste headroom on huge contexts and
    // don't blow up small ones. See `adapt_budget_to_context_window`.
    let context_window = arguments
        .get("context_window")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let max_tokens = arguments
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or_else(|| {
            let base = state
                .execution_token_budget(&session)
                .max(HYBRID_RETRIEVAL_FLOOR);
            match context_window {
                Some(window) => adapt_budget_to_context_window(base, window),
                None => base,
            }
        });
    let include_body = optional_bool(arguments, "include_body", false);
    let depth = optional_usize(arguments, "depth", 2);
    let disable_semantic = optional_bool(arguments, "disable_semantic", false);
    // v1.10.1: opt-out of n-gram query expansion. The default behaviour
    // (expand_query=true) preserves prior recall on partial-identifier
    // queries; setting expand_query=false disables snake_case /
    // camelCase / cartesian-token expansion for natural-language
    // queries that don't benefit from it.
    let expand_query = optional_bool(arguments, "expand_query", true);
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let exact_identifier_projection = query_analysis.original_query
        != query_analysis.expanded_query
        && !query_analysis.expanded_query.contains(char::is_whitespace);
    let effective_disable_semantic =
        disable_semantic || query_analysis.prefer_lexical_only || exact_identifier_projection;
    let use_semantic_in_core = !effective_disable_semantic;
    let use_sparse_in_core = query_analysis.natural_language
        || (query_analysis.prefer_sparse_symbol_search
            && query_analysis.original_query.contains(char::is_whitespace));
    // Build semantic scores for hybrid ranking if embeddings are available.
    // The default model is the bundled CodeSearchNet MiniLM-L12 INT8 variant.
    let semantic_results = semantic_results_for_query(state, query, 50, effective_disable_semantic);
    let sparse_results = if use_sparse_in_core {
        sparse_symbol_hits_for_query(state, &query_analysis, 10, false, false, &session)?
    } else {
        Vec::new()
    };
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

    // v1.10.1: when `expand_query=false`, use the user's literal query
    // for retrieval. The default keeps the n-gram expansion path so
    // partial-identifier queries still match across snake_case /
    // camelCase boundaries. See `docs/eval/v1.10.0-post-release-eval.md`
    // (F3).
    let retrieval_query: &str = if expand_query {
        &query_analysis.expanded_query
    } else {
        &query_analysis.original_query
    };

    // query-type-aware weights available via get_ranked_context_cached_with_query_type
    // but current dataset shows default weights are near-optimal (0.680 MRR).
    // Kept as None until per-type weight tuning yields measurable improvement.
    let mut result = state.symbol_index().get_ranked_context_cached(
        retrieval_query,
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
    if use_sparse_in_core {
        merge_sparse_ranked_entries(query, &mut result, sparse_results.clone(), 6);
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
                .sort_unstable_by_key(|b| std::cmp::Reverse(b.relevance_score));
        }
    }

    let semantic_evidence = if effective_disable_semantic {
        Vec::new()
    } else {
        compact_semantic_evidence(&result, &semantic_results, 5)
    };
    let sparse_evidence = if use_sparse_in_core {
        compact_sparse_evidence(&result, &sparse_results, 5)
    } else {
        Vec::new()
    };
    let mut payload =
        serde_json::to_value(&result).map_err(|e| CodeLensError::Internal(e.into()))?;
    annotate_ranked_context_provenance(
        &mut payload,
        &structural_keys,
        &semantic_results,
        &sparse_results,
    );
    let preferred_lane = if use_sparse_in_core && !effective_disable_semantic {
        "hybrid_semantic_sparse"
    } else if use_sparse_in_core {
        "sparse_bm25f"
    } else if effective_disable_semantic {
        "structural_lexical"
    } else {
        "hybrid_semantic"
    };
    let query_type = if query_analysis.prefer_lexical_only {
        "identifier"
    } else if query_analysis.natural_language {
        "natural_language"
    } else {
        "short_phrase"
    };
    let retrieval = json!({
        "semantic_enabled": !effective_disable_semantic,
        "semantic_used_in_core": use_semantic_in_core,
        "sparse_used_in_core": use_sparse_in_core,
        "preferred_lane": preferred_lane,
        "sparse_lane_recommended": query_analysis.prefer_sparse_symbol_search,
        "query_type": query_type,
        "lexical_query": query_analysis.expanded_query,
        "semantic_query": query_analysis.semantic_query,
    });
    let backend = if result.symbols.iter().any(|s| s.relevance_score > 0) {
        BackendKind::TreeSitter
    } else {
        BackendKind::Semantic
    };
    let meta = success_meta(backend, 0.91);
    let evidence = crate::tool_evidence::tool_evidence(
        "retrieval",
        &meta,
        preferred_lane,
        json!({
            "preferred_lane": preferred_lane,
            "query_type": query_type,
            "semantic_enabled": !effective_disable_semantic,
            "semantic_used_in_core": use_semantic_in_core,
            "sparse_used_in_core": use_sparse_in_core,
            "semantic_evidence_count": semantic_evidence.len(),
            "sparse_evidence_count": sparse_evidence.len(),
            "precise_available": false,
            "precise_used": false,
            "precise_source": null,
            "fallback_source": preferred_lane,
            "precise_result_count": 0,
        }),
    );
    if let Some(map) = payload.as_object_mut() {
        map.insert("retrieval".to_owned(), retrieval);
        if !semantic_evidence.is_empty() {
            map.insert("semantic_evidence".to_owned(), json!(semantic_evidence));
        }
        if !sparse_evidence.is_empty() {
            map.insert("sparse_evidence".to_owned(), json!(sparse_evidence));
        }
        map.insert("evidence".to_owned(), evidence);
        if !unknown_args.is_empty() {
            map.insert("unknown_args".to_owned(), json!(unknown_args));
        }
    }

    Ok((payload, meta))
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
mod find_symbol_argument_tests {
    use super::find_symbol;
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;
    use serde_json::json;

    fn test_state(label: &str) -> crate::AppState {
        let project = temp_project_root(label);
        crate::AppState::new_minimal(project, ToolPreset::Full)
    }

    #[test]
    fn name_path_alias_resolves_with_deprecation_warning() {
        let state = test_state("find-symbol-name-path-alias");

        let (payload, _) = find_symbol(&state, &json!({ "name_path": "find_symbol" }))
            .expect("name_path alias should resolve without MissingParam");

        let warnings = payload["deprecation_warnings"]
            .as_array()
            .expect("deprecation_warnings should be an array");
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings
                .first()
                .and_then(|warning| warning.as_str())
                .is_some_and(|warning| warning.contains("name_path"))
        );
    }

    #[test]
    fn unknown_args_surfaced_in_top_level_warnings() {
        let state = test_state("find-symbol-unknown-args");

        let (payload, _) = find_symbol(
            &state,
            &json!({ "name": "find_symbol", "nonexistent_arg": "value" }),
        )
        .expect("unknown args should be ignored");

        let warnings = payload["warnings"]
            .as_array()
            .expect("warnings should be a top-level array");
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|warning| warning.contains("nonexistent_arg"))
        }));
    }
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

#[cfg(all(test, feature = "scip-backend"))]
mod humanize_scip_name_path_tests {
    use super::humanize_scip_name_path;

    #[test]
    fn strips_rust_analyzer_preamble_and_function_suffix() {
        // Real shape observed in dogfood today (issue #235 reproduction).
        let raw = "rust-analyzer cargo codelens-mcp 1.9.59 tools/session/project_ops/prepare_harness_session().";
        assert_eq!(
            humanize_scip_name_path(raw),
            "tools/session/project_ops/prepare_harness_session"
        );
    }

    #[test]
    fn strips_type_descriptor_hash_suffix() {
        let raw = "scip-rust cargo codelens-engine 1.9.59 ir/PreciseBackend#";
        assert_eq!(humanize_scip_name_path(raw), "ir/PreciseBackend");
    }

    #[test]
    fn strips_constant_dot_suffix() {
        let raw = "scip-rust cargo codelens-mcp 1.9.59 constants/MAX_SIZE.";
        assert_eq!(humanize_scip_name_path(raw), "constants/MAX_SIZE");
    }

    #[test]
    fn falls_back_to_raw_when_format_unrecognised() {
        // Fewer than four header tokens — return the raw input rather
        // than fabricate a wrong path.
        let raw = "no_descriptor_format";
        assert_eq!(humanize_scip_name_path(raw), "no_descriptor_format");
    }

    #[test]
    fn empty_after_strip_falls_back_to_raw() {
        // Edge case — descriptor is just punctuation; we'd otherwise
        // emit `""`, which loses the identity. Preserve raw instead.
        let raw = "scip-rust cargo crate 1.0 .";
        assert_eq!(humanize_scip_name_path(raw), raw);
    }
}

#[cfg(all(test, feature = "scip-backend"))]
mod read_signature_line_tests {
    use super::read_signature_line;
    use crate::AppState;
    use codelens_engine::ProjectRoot;

    fn make_test_state(project_root: &std::path::Path) -> AppState {
        let project = ProjectRoot::new(project_root.to_str().unwrap()).expect("project");
        AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full)
    }

    #[test]
    fn returns_trimmed_declaration_at_target_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("src.rs"),
            "use std::io;\n\npub fn alpha(x: i32) -> i32 {\n    x + 1\n}\n",
        )
        .unwrap();
        let state = make_test_state(dir.path());
        // Lines (0-indexed, matching SCIP `parse_range` convention):
        //   0: "use std::io;"
        //   1: ""
        //   2: "pub fn alpha(x: i32) -> i32 {"
        //   3: "    x + 1"
        //   4: "}"
        let signature = read_signature_line(&state, "src.rs", 2)
            .expect("non-empty declaration line should yield Some");
        assert_eq!(signature, "pub fn alpha(x: i32) -> i32 {");
    }

    #[test]
    fn returns_none_for_blank_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("src.rs"),
            "fn first() {}\n\nfn second() {}\n",
        )
        .unwrap();
        let state = make_test_state(dir.path());
        // 0-indexed line 1 is the empty line between the two functions —
        // must surface as None rather than `""` so the caller can branch
        // on `signature_source: "unavailable"`.
        assert!(read_signature_line(&state, "src.rs", 1).is_none());
    }

    #[test]
    fn returns_none_for_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = make_test_state(dir.path());
        assert!(read_signature_line(&state, "does_not_exist.rs", 1).is_none());
    }
}
