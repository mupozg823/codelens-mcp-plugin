use super::super::super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::{RetrievalQueryAnalysis, analyze_retrieval_query},
    required_string, success_meta,
};
use super::super::{
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

use super::path_args::{insert_response_annotations, resolve_path_argument};

#[cfg(feature = "scip-backend")]
const HEURISTIC_BODY_LINES: usize = 50;

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

/// Issue #245: defence against engine-layer drift that re-fills SCIP's
/// `signature` field with non-signature text. A real declaration line
/// is single-line and starts with one of the canonical Rust / TS / Python
/// declaration-introducing tokens. Multi-line strings or prose-shaped
/// text (a leading capital letter, sentence punctuation) are treated as
/// documentation and rejected so the picker falls through to the
/// source-line read fallback (#235-B).
#[cfg(feature = "scip-backend")]
pub(super) fn looks_like_signature(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return false;
    }
    const DECL_PREFIXES: &[&str] = &[
        // Rust
        "pub ",
        "pub(",
        "fn ",
        "async ",
        "unsafe ",
        "extern ",
        "const ",
        "static ",
        "struct ",
        "enum ",
        "trait ",
        "impl ",
        "type ",
        "mod ",
        "use ",
        "macro_rules!",
        "let ",
        // Python / TS / others we may see when SCIP indexes mixed repos
        "def ",
        "class ",
        "function ",
        "export ",
        "interface ",
        "namespace ",
        "var ",
        "void ",
        "int ",
        "double ",
        "float ",
        "bool ",
    ];
    DECL_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

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
                    //
                    // Issue #245: SCIP `d.signature` was historically
                    // populated from `SymbolInformation.documentation`
                    // (rustdoc prose, not a declaration). Even after the
                    // engine-side fix in this PR empties that field,
                    // guard the picker with `looks_like_signature` so a
                    // future engine regression that leaks doc text into
                    // `signature` cannot silently re-land. Also drop the
                    // `scip_doc_hover` branch — the hover string is
                    // documentation, never a signature, and is already
                    // surfaced under the separate `documentation` field.
                    let (signature_value, signature_source) =
                        if !d.signature.is_empty() && looks_like_signature(&d.signature) {
                            (d.signature.clone(), "scip_signature")
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
