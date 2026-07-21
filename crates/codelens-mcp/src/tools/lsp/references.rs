use super::super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_bool, optional_string,
    optional_usize, parse_lsp_args, success_meta,
};
use super::rename::resolve_symbol_position;
use super::shared::{enhance_lsp_error, insert_response_annotations, resolve_path_argument};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_evidence::{meta_degraded, meta_for_backend};
use codelens_engine::{
    LspRequest, extract_word_at_position, find_referencing_symbols_via_text, get_callers,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn compact_text_references(
    references: Vec<codelens_engine::TextReference>,
    include_context: bool,
    full_results: bool,
    sample_limit: usize,
) -> (Vec<serde_json::Value>, usize, bool) {
    let total_count = references.len();
    let effective_limit = if full_results {
        references.len()
    } else {
        sample_limit.min(references.len())
    };
    let sampled = !full_results && total_count > effective_limit;
    let compact = references
        .into_iter()
        .take(effective_limit)
        .map(|reference| {
            let mut value = json!({
                "file_path": reference.file_path,
                "line": reference.line,
                "column": reference.column,
                "is_declaration": reference.is_declaration,
            });
            if include_context {
                value["line_content"] = json!(reference.line_content);
                if let Some(symbol) = reference.enclosing_symbol {
                    value["enclosing_symbol"] = json!(symbol);
                }
            }
            value
        })
        .collect::<Vec<_>>();
    (compact, total_count, sampled)
}

fn is_js_ts_path(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|value| value.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts")
    )
}

fn classify_ts_type_reference(line: &str, symbol_name: &str) -> &'static str {
    let trimmed = line.trim();
    if trimmed.contains("z.infer") && trimmed.contains(symbol_name) {
        "zod_infer_type"
    } else if trimmed.contains(".safeParse(") || trimmed.contains(" as ") {
        "schema_or_cast_type"
    } else if trimmed.starts_with("import type")
        || trimmed.contains("import { type ")
        || trimmed.contains("import {")
    {
        "type_import"
    } else if trimmed.contains(" extends ") || trimmed.contains(" implements ") {
        "type_inheritance"
    } else if trimmed.contains(':') || trimmed.contains('<') || trimmed.contains('&') {
        "type_annotation"
    } else {
        "text_name_match"
    }
}

fn is_structural_ts_reference_kind(kind: &str) -> bool {
    kind != "text_name_match"
}

fn structural_ts_reference_evidence(
    state: &AppState,
    file_path: &str,
    symbol_name: &str,
    max_results: usize,
) -> Option<Value> {
    if !is_js_ts_path(file_path) {
        return None;
    }

    let search_limit = max_results.saturating_mul(3).max(24);
    let refs = find_referencing_symbols_via_text(
        &state.project(),
        symbol_name,
        Some(file_path),
        search_limit,
    )
    .ok()?;

    let mut rows = Vec::new();
    let mut total_count = 0usize;
    for reference in refs {
        if !is_js_ts_path(&reference.file_path) {
            continue;
        }
        if reference.is_declaration {
            continue;
        }
        let evidence_kind = classify_ts_type_reference(&reference.line_content, symbol_name);
        if !is_structural_ts_reference_kind(evidence_kind) {
            continue;
        }
        total_count += 1;
        if rows.len() >= max_results.min(8) {
            continue;
        }
        rows.push(json!({
            "file_path": reference.file_path,
            "line": reference.line,
            "column": reference.column,
            "evidence_kind": evidence_kind,
            "line_content": reference.line_content.trim(),
        }));
    }

    if total_count == 0 {
        return None;
    }

    Some(json!({
        "basis": "text_type_or_cast_references",
        "count": total_count,
        "returned_count": rows.len(),
        "references": rows,
        "orphan_conclusion": "not_safe_to_mark_unused",
        "message": "TypeScript structural type/cast evidence exists outside the precise backend result; do not interpret a low precise count as unused without this evidence.",
    }))
}

fn structural_evidence_count(evidence: &Option<Value>) -> usize {
    evidence
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default() as usize
}

fn insert_structural_ts_evidence(
    payload: &mut Value,
    evidence: Option<Value>,
    precise_count: usize,
) {
    let Some(evidence) = evidence else {
        return;
    };
    let structural_count = evidence
        .get("count")
        .and_then(|value| value.as_u64())
        .unwrap_or_default() as usize;
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    map.insert("structural_reference_evidence".to_owned(), evidence);
    map.insert(
        "reference_evidence_count".to_owned(),
        json!(precise_count + structural_count),
    );
    map.insert(
        "structural_usage_warning".to_owned(),
        json!({
            "code": "ts_structural_evidence_present",
            "message": "Precise JS/TS reference backends can miss structural type, cast, and schema-derived usages. Treat low precise counts as inconclusive when structural evidence is present.",
            "recommended_action": "inspect_structural_reference_evidence",
        }),
    );
}

/// P4: oxc_semantic resolves references only within the requested file, so an
/// exported JS/TS symbol used only in sibling modules resolves to just its own
/// definition (`is_self_only`). Merge the import_graph backend's cross-file
/// callers (`get_callers` over a project-wide scan) so those usages surface in
/// the reference set. Rows are deduped against the oxc results (and each other)
/// by `(file_path, line)`; a same-file caller oxc already reported is dropped.
/// Returns the additional reference rows (empty when import_graph finds none).
fn cross_file_ts_caller_rows(
    state: &AppState,
    symbol_name: &str,
    target_file: &str,
    oxc_lines: &std::collections::HashSet<usize>,
    max_results: usize,
) -> Vec<Value> {
    let project = state.project();
    let target_rel = project
        .resolve(target_file)
        .map(|resolved| project.to_relative(resolved))
        .unwrap_or_else(|_| target_file.to_owned());
    // `file_path: None` makes get_callers scan the whole project (import_graph
    // resolution), which is exactly the cross-file set oxc cannot see.
    let callers = match get_callers(
        &project,
        symbol_name,
        None,
        max_results,
        Some(state.graph_cache().as_ref()),
    ) {
        Ok(callers) => callers,
        Err(_) => return Vec::new(),
    };
    let seen: std::collections::HashSet<(String, usize)> = oxc_lines
        .iter()
        .map(|line| (target_rel.clone(), *line))
        .collect();
    merge_caller_rows_dedup(callers, seen, max_results)
}

/// Pure merge/dedup for [`cross_file_ts_caller_rows`]: keep only JS/TS caller
/// rows, drop any `(file, line)` already present in `seen` (the oxc same-file
/// results) or already emitted, cap at `max_results`, and tag each row with the
/// merging backend so the evidence is self-describing. Isolated from the
/// project-scanning `get_callers` I/O so the union/dedup contract is
/// unit-testable with synthetic callers.
fn merge_caller_rows_dedup(
    callers: Vec<codelens_engine::CallerEntry>,
    mut seen: std::collections::HashSet<(String, usize)>,
    max_results: usize,
) -> Vec<Value> {
    let mut rows = Vec::new();
    for entry in callers {
        if !is_js_ts_path(&entry.file) {
            continue;
        }
        if !seen.insert((entry.file.clone(), entry.line)) {
            continue;
        }
        rows.push(json!({
            "file_path": entry.file,
            "line": entry.line,
            "enclosing_function": entry.function,
            "kind": "cross_file_caller",
            "resolution": entry.resolution,
            "confidence": entry.confidence,
            "backend": "import_graph",
        }));
        if rows.len() >= max_results {
            break;
        }
    }
    rows
}

/// P3: bounded cold-start wait for the explicit `use_lsp=true` path.
///
/// A `use_lsp=true` request against a COLD language server used to spawn it,
/// read back whatever the still-indexing server had (frequently zero
/// references), and fall through to a 0.7-confidence tree-sitter answer — a
/// silent recall miss on a machine where the server is installed and would
/// answer correctly once warm. This runs the reference query and, when the
/// server had to cold-start and returned nothing, retries on a bounded schedule
/// (total ≈ [`LSP_COLD_WAIT`]) until it returns results, reports quiescent
/// (genuinely no references), or the session dies. The wait blocks the request
/// thread but is hard-capped — never unbounded.
const LSP_COLD_WAIT: Duration = Duration::from_secs(10);
const LSP_COLD_POLL: Duration = Duration::from_millis(500);

fn lsp_references_with_cold_wait(
    pool: &codelens_engine::LspSessionPool,
    request: &LspRequest,
) -> anyhow::Result<Vec<codelens_engine::LspReference>> {
    let (mut refs, cold_started) = pool.find_referencing_symbols_tracking_spawn(request)?;
    if !refs.is_empty() || !cold_started {
        return Ok(refs);
    }
    let deadline = Instant::now() + LSP_COLD_WAIT;
    while refs.is_empty() && Instant::now() < deadline {
        std::thread::sleep(LSP_COLD_POLL);
        // Each request drains the server's pending notifications, so this both
        // retries the query and harvests the latest quiescence signal.
        match pool.find_referencing_symbols(request) {
            Ok(retry) => refs = retry,
            Err(_) => break,
        }
        if !refs.is_empty() {
            break;
        }
        match pool.warm_session_quiescence(&request.command, &request.args) {
            // Finished indexing but still empty ⇒ truly no references. Session
            // gone ⇒ nothing more to wait for. Either way, stop early.
            Some(Some(true)) | None => break,
            _ => {}
        }
    }
    Ok(refs)
}

/// Only probe the text baseline when the LSP count is this low or below —
/// a healthy multi-reference LSP result needs no cross-check, and the probe
/// runs a project text scan we would rather not pay on every explicit call.
const LSP_UNDERREPORT_PROBE_MAX: usize = 3;

/// Regression [B]: decide whether an LSP reference count is implausibly low
/// versus the tree-sitter text scan. `true` means the text scan found more than
/// twice what LSP returned — the signal that the server under-reported (a cold
/// or partial index, or a declaration-only answer) and the fuller text set
/// should win at reduced confidence. Pure so the threshold is unit-testable.
fn lsp_underreports_vs_text(lsp_count: usize, text_count: usize) -> bool {
    text_count > lsp_count && lsp_count.saturating_mul(2) < text_count
}

/// Decide the SCIP-undercount-vs-text fallback for `find_referencing_symbols`.
/// Returns `(degraded_reason, confidence_basis)` when the tree-sitter text scan
/// strictly out-counts the SCIP precise result — the signal SCIP lost call
/// sites, whether from a STALE index (#251) or a FRESH-index coverage gap where
/// rust-analyzer's SCIP export omits references in targets outside the default
/// check set (`[[bench]]` / examples / cfg-gated code; live E2E 2026-07-21:
/// scip 12 vs text 16, the 3+ missing rows living in `benches/indexing.rs`).
/// `None` means SCIP's count is complete (>= text), so the 0.98 precise tier
/// stands. `stale` only selects the label so downstream can tell a stale index
/// from a coverage gap; both variants serve the fuller text set. Unlike the
/// 2×-threshold LSP guard, ANY strict undercount fires — a precise backend that
/// silently drops even one real reference must not win at 0.98. Pure so the
/// firing condition and the stale/fresh split are unit-testable without a live
/// index.
#[cfg(feature = "scip-backend")]
fn scip_undercount_fallback(
    scip_count: usize,
    text_count: usize,
    stale: bool,
) -> Option<(&'static str, &'static str)> {
    if text_count > scip_count {
        Some(if stale {
            (
                "scip_stale_undercount_vs_text",
                "scip_stale_undercount_text_fallback",
            )
        } else {
            ("scip_undercount_vs_text", "scip_undercount_text_fallback")
        })
    } else {
        None
    }
}

/// Attach the `full_results` completeness marker so response summarization
/// preserves the entire primary result array instead of clipping it to the
/// preview cap (and falsely flagging `truncated`). Every reference path — the
/// tree-sitter scan, the LSP fallback, and the P4 cross-file merge (regression
/// [D]) — routes through this one seam, and `get_callers` / `get_callees` reuse
/// it for their `callers` / `callees` arrays. The summarizer selects the actual
/// protected key from the payload (see `full_results_primary_key` in
/// `payload_compact`), so this seam only stamps the boolean. A no-op when
/// `full_results` was not requested.
pub(crate) fn mark_full_results(payload: &mut Value, full_results: bool) {
    if full_results && let Some(map) = payload.as_object_mut() {
        map.insert("full_results".to_owned(), json!(true));
    }
}

/// P1.1b: readiness-aware confidence for LSP-backed reference results.
///
/// `quiescence` is the server's latest `experimental/serverStatus` state as
/// harvested by the session read loop (P1.1a):
/// - `Some(false)` — the server *itself* reports indexing in progress; its
///   reference answers may be incomplete, so the precise-tier label is a lie.
///   Degrade to 0.7 with an explicit reason (mirrors the SCIP-stale pattern).
/// - `Some(true)` — verified quiescent: the 0.95 precise label is earned and
///   the basis says so.
/// - `None` — the server emits no readiness signal (e.g. pyright): keep the
///   legacy 0.95 label; there is no evidence in either direction, and blanket
///   distrust would permanently punish servers without the extension.
///
/// Returns `(confidence, degraded_reason, confidence_basis)`; kept pure so
/// the signal→label mapping is unit-testable without a live server.
fn lsp_confidence_for_quiescence(
    quiescence: Option<bool>,
    quiescent_basis: &'static str,
    unknown_basis: &'static str,
) -> (f64, Option<&'static str>, &'static str) {
    match quiescence {
        Some(false) => (
            0.7,
            Some("lsp_server_indexing_in_progress"),
            "lsp_warm_indexing_in_progress",
        ),
        Some(true) => (0.95, None, quiescent_basis),
        None => (0.95, None, unknown_basis),
    }
}

/// tree-sitter-first strategy:
///
/// Default (symbol_name only):
///   tree-sitter scope analysis → fast, zero-config, works on broken code
///
/// LSP path (use_lsp=true or line+column):
///   LSP references → tree-sitter fallback on failure
///
/// Rationale: MCP tools serve AI agents that value speed and availability
/// over IDE-grade type precision. LSP adds latency (cold start 2-30s),
/// requires external server installation, and fails on incomplete code.
pub fn find_referencing_symbols(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    // P1-B — limit/top_k aliases + unknown_args.
    // See docs/design/arg-validation-policy.md.
    const KNOWN_ARGS: &[&str] = &[
        "path",
        "file_path",
        "relative_path",
        "symbol_name",
        "max_results",
        "limit",
        "top_k",
        "use_lsp",
        "include_context",
        "full_results",
        "sample_limit",
        "line",
        "column",
        "command",
        "args",
    ];
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let file_path = file_path_arg.to_owned();
    let symbol_name_param = optional_string(arguments, "symbol_name");
    let max_results = crate::tool_runtime::optional_usize_with_aliases(
        arguments,
        "max_results",
        &["limit", "top_k"],
        20,
    );
    let use_lsp = optional_bool(arguments, "use_lsp", false);
    let include_context = optional_bool(arguments, "include_context", false);
    let full_results = optional_bool(arguments, "full_results", false);
    let sample_limit = optional_usize(arguments, "sample_limit", 8);
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);

    let has_position = arguments.get("line").is_some() && arguments.get("column").is_some();

    // Default: scope analysis (fast, zero-config, works on broken code)
    if !use_lsp && !has_position {
        let sym_name = symbol_name_param.ok_or_else(|| {
            CodeLensError::MissingParam("symbol_name (or line+column with use_lsp=true)".into())
        })?;
        let mut precise_available = false;
        let mut precise_source = None;

        // JS/TS: use oxc_semantic for precise scope-aware reference resolution
        let resolved = state.project().resolve(&file_path)?;
        if codelens_engine::oxc_analysis::is_js_ts(&resolved) {
            precise_available = true;
            precise_source = Some("oxc_semantic");
            if let Ok(source) = std::fs::read_to_string(&resolved)
                && let Ok(refs) = codelens_engine::oxc_analysis::find_references_precise(
                    &source, &file_path, sym_name,
                )
                && !refs.is_empty()
            {
                let refs_limited: Vec<_> = refs.into_iter().take(max_results).collect();
                let count = refs_limited.len();
                // Issue #201: a self-only result (only the definition row
                // itself) is the prime symptom of the oxc_semantic
                // single-file scope gap. Historically the response kept the
                // precise-path 0.95 confidence, which reads as a
                // high-trust "this symbol is unused" answer — exactly the
                // shape of result reviewers wrongly act on. Detect
                // self-only up front so we can build a degraded meta and
                // a `oxc_semantic_self_only` evidence basis instead.
                let is_self_only = count == 1
                    && refs_limited
                        .first()
                        .and_then(|r| serde_json::to_value(r).ok())
                        .and_then(|v| {
                            v.get("kind")
                                .and_then(|k| k.as_str())
                                .map(|k| k.eq_ignore_ascii_case("definition"))
                        })
                        .unwrap_or(false);
                // P4: a self-only oxc result is the exact symptom of the
                // single-file scope gap — the symbol's cross-file callers (if
                // any) are invisible. Merge the import_graph backend's
                // cross-file callers so an exported-but-externally-used symbol
                // is not misreported as self-only/unused. When callers are
                // found the answer changes shape (hybrid, non-degraded), so
                // return the merged payload directly.
                if is_self_only {
                    let oxc_lines: std::collections::HashSet<usize> = refs_limited
                        .iter()
                        .map(|reference| reference.line)
                        .collect();
                    let cross_rows = cross_file_ts_caller_rows(
                        state,
                        sym_name,
                        &file_path,
                        &oxc_lines,
                        max_results,
                    );
                    if !cross_rows.is_empty() {
                        let mut references: Vec<Value> = refs_limited
                            .iter()
                            .filter_map(|reference| serde_json::to_value(reference).ok())
                            .collect();
                        let cross_file_count = cross_rows.len();
                        references.extend(cross_rows);
                        let merged_count = references.len();
                        let meta = success_meta(BackendKind::Hybrid, 0.9);
                        let evidence = crate::tool_evidence::tool_evidence(
                            "references",
                            &meta,
                            "oxc_semantic_plus_import_graph_cross_file",
                            crate::tool_evidence::precision_signals(
                                true,
                                true,
                                Some("oxc_semantic+import_graph"),
                                None,
                                merged_count,
                            ),
                        );
                        let mut payload = json!({
                            "references": references,
                            "count": merged_count,
                            "returned_count": merged_count,
                            "sampled": false,
                            "backend": "oxc_semantic+import_graph",
                            "evidence": evidence,
                            "precision_note": "oxc_semantic resolved same-file references; import_graph (get_callers) cross-file callers were merged in, so this count spans files.",
                            "cross_file_merge": {
                                "backend": "import_graph",
                                "basis": "get_callers_import_graph",
                                "cross_file_caller_count": cross_file_count,
                                "message": "A low oxc_semantic count was augmented with import_graph cross-file callers; do not read the pre-merge single-file count as unused.",
                            },
                        });
                        // Regression [D]: the merge produces the complete
                        // reference set, so honor full_results the same way the
                        // tree-sitter path does — preserve every merged row.
                        mark_full_results(&mut payload, full_results);
                        if !unknown_args.is_empty() || !deprecation_warnings.is_empty() {
                            insert_response_annotations(
                                &mut payload,
                                &unknown_args,
                                &deprecation_warnings,
                            );
                        }
                        return Ok((payload, meta));
                    }
                }
                let structural_evidence = is_self_only
                    .then(|| {
                        structural_ts_reference_evidence(state, &file_path, sym_name, max_results)
                    })
                    .flatten();
                let structural_count = structural_evidence_count(&structural_evidence);
                let (meta, confidence_basis) = if is_self_only {
                    if structural_count > 0 {
                        (
                            crate::tool_evidence::meta_degraded(
                                "hybrid",
                                0.72,
                                "single_definition_plus_ts_structural_evidence",
                            ),
                            "oxc_self_only_plus_ts_structural_evidence",
                        )
                    } else {
                        (
                            crate::tool_evidence::meta_degraded(
                                "hybrid",
                                0.6,
                                "single_definition_no_cross_file_visible",
                            ),
                            "oxc_semantic_self_only",
                        )
                    }
                } else {
                    (
                        success_meta(BackendKind::Hybrid, 0.95),
                        "oxc_semantic_precise",
                    )
                };
                let evidence = crate::tool_evidence::tool_evidence(
                    "references",
                    &meta,
                    confidence_basis,
                    crate::tool_evidence::precision_signals(
                        true,
                        true,
                        Some("oxc_semantic"),
                        None,
                        count,
                    ),
                );
                // Issue #214: oxc_semantic resolves references within the
                // file it was given — it does not cross file boundaries to
                // chase `import { foo } from '...'` callers in sibling
                // modules. Surface that limitation in every response so a
                // caller that sees only a self-definition row knows there
                // may be cross-file callers reachable via `get_callers`
                // (import_graph backend) or `find_scoped_references` /
                // `use_lsp=true`.
                let mut payload = json!({
                    "references": refs_limited,
                    "count": count,
                    "returned_count": count,
                    "sampled": false,
                    "backend": "oxc_semantic",
                    "evidence": evidence,
                    "precision_note": "oxc_semantic resolves references within the requested file; import-statement-based callers across files are not in scope.",
                    "cross_file_callers_hint": {
                        "tool": "get_callers",
                        "rationale": "import_graph backend follows `import { name } from '...'` to upstream callers across files",
                    },
                });
                insert_structural_ts_evidence(&mut payload, structural_evidence, count);
                // The "self-only" case (only the definition row itself)
                // is the prime symptom of the cross-file gap — flag it
                // explicitly so the caller does not mistake it for "no
                // callers exist".
                if is_self_only && let Some(map) = payload.as_object_mut() {
                    map.insert(
                        "self_only_warning".to_owned(),
                        json!({
                            "code": "definition_only",
                            "message": "Only the symbol's own definition row was returned. For exported symbols this almost always means cross-file callers exist but are not visible to oxc_semantic.",
                            "recommended_action": "call_get_callers",
                            "action_target": "cross_file_callers",
                        }),
                    );
                }
                if !unknown_args.is_empty() || !deprecation_warnings.is_empty() {
                    insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
                }
                return Ok((payload, meta));
            }
        }
        // oxc failed or empty — try SCIP if available, then fall through to tree-sitter

        // Left nested rather than collapsed into a let-chain: the SCIP
        // references block below is long enough that a multi-condition
        // guard reads worse than the explicit nesting.
        #[cfg(feature = "scip-backend")]
        #[allow(clippy::collapsible_if)]
        if let Some(backend) = state.scip() {
            if backend.has_index_for(&file_path) {
                precise_available = true;
                precise_source = Some("scip");
                if let Ok(refs) = backend.find_references(sym_name, &file_path, 0)
                    && !refs.is_empty()
                {
                    let limited: Vec<_> = refs.into_iter().take(max_results).collect();
                    let count = limited.len();
                    // Issue #240: same SCIP staleness probe as
                    // `find_symbol` — if the index pre-dates any
                    // resolved file, swap to a degraded meta + emit
                    // `scip_index_stale_warning` so reviewers can't
                    // mistake stale call sites for current ones.
                    let scip_candidate_files: Vec<String> =
                        limited.iter().map(|r| r.file_path.clone()).collect();
                    let scip_staleness = crate::tools::scip_health::detect_scip_staleness(
                        state.project().as_path(),
                        &scip_candidate_files,
                    );
                    // Issue #251 + P7: a SCIP precise reference count below the
                    // tree-sitter text scan means SCIP lost call sites — a
                    // STALE index dropped them (#251) or rust-analyzer's SCIP
                    // export never covered them (FRESH-index coverage gap:
                    // `[[bench]]` / example / cfg-gated targets outside the
                    // default check set; live E2E 2026-07-21: scip 12 vs text
                    // 16, the missing rows in `benches/indexing.rs`). Either way
                    // the 0.98 precise tier silently under-reports, so
                    // cross-check the text scan on EVERY precise result — not
                    // just the stale path — and when text finds strictly more,
                    // serve the fuller set at reduced confidence with a warning
                    // naming both counts. `scip_undercount_fallback` keeps the
                    // firing condition + stale/fresh label split pure. The extra
                    // text scan on the fresh path is the cost of not trusting an
                    // under-covering precise index.
                    if let Ok(text_refs) = find_referencing_symbols_via_text(
                        &state.project(),
                        sym_name,
                        Some(&file_path),
                        max_results,
                    ) && let Some((degraded_reason, confidence_basis)) =
                        scip_undercount_fallback(count, text_refs.len(), scip_staleness.is_some())
                    {
                        let scip_count = count;
                        let text_count = text_refs.len();
                        let (references, total_count, sampled) = compact_text_references(
                            text_refs,
                            include_context,
                            full_results,
                            sample_limit,
                        );
                        let meta = meta_degraded("hybrid", 0.6, degraded_reason);
                        let evidence = crate::tool_evidence::tool_evidence(
                            "references",
                            &meta,
                            confidence_basis,
                            crate::tool_evidence::precision_signals(
                                true,
                                false,
                                Some("scip"),
                                Some("tree_sitter"),
                                total_count,
                            ),
                        );
                        let mut payload = json!({
                            "references": references,
                            "count": total_count,
                            "returned_count": references.len(),
                            "sampled": sampled,
                            "include_context": include_context,
                            "backend": "tree_sitter",
                            "evidence": evidence,
                        });
                        if let Some(map) = payload.as_object_mut() {
                            match scip_staleness.as_ref() {
                                Some(stale) => {
                                    map.insert(
                                        "scip_index_stale_warning".to_owned(),
                                        crate::tools::scip_health::scip_stale_undercount_warning_payload(
                                            stale, scip_count, text_count,
                                        ),
                                    );
                                }
                                None => {
                                    map.insert(
                                        "scip_undercount_warning".to_owned(),
                                        json!({
                                            "code": "scip_undercount_vs_text",
                                            "scip_count": scip_count,
                                            "text_count": text_count,
                                            "message": format!(
                                                "SCIP precise backend returned {scip_count} references but the tree-sitter text scan found {text_count}. rust-analyzer's SCIP export does not cover some references (e.g. benchmark/example/cfg-gated targets outside the default check set), so the precise count under-reports. Serving the fuller tree-sitter set at reduced confidence."
                                            ),
                                            "recommended_action": "trust_text_superset",
                                        }),
                                    );
                                }
                            }
                        }
                        mark_full_results(&mut payload, full_results);
                        if !unknown_args.is_empty() || !deprecation_warnings.is_empty() {
                            insert_response_annotations(
                                &mut payload,
                                &unknown_args,
                                &deprecation_warnings,
                            );
                        }
                        return Ok((payload, meta));
                    }
                    let (meta, confidence_basis) = if scip_staleness.is_some() {
                        (
                            crate::tool_evidence::meta_degraded(
                                "scip",
                                0.55,
                                "scip_index_stale_vs_source",
                            ),
                            "scip_precise_stale_index",
                        )
                    } else {
                        (success_meta(BackendKind::Scip, 0.98), "scip_precise")
                    };
                    let evidence = crate::tool_evidence::tool_evidence(
                        "references",
                        &meta,
                        confidence_basis,
                        crate::tool_evidence::precision_signals(
                            true,
                            true,
                            Some("scip"),
                            None,
                            count,
                        ),
                    );
                    let refs_json: Vec<serde_json::Value> = limited
                        .iter()
                        .map(|r| {
                            // Issue #243: SCIP `parse_range` is 0-indexed
                            // per spec; tree-sitter / file-display /
                            // grep are 1-indexed. Normalize at the JSON
                            // serialization boundary so reviewers
                            // comparing `find_referencing_symbols`
                            // output to `read_file` / `Edit` line
                            // numbers stop landing one row early.
                            let display_line =
                                crate::tools::scip_health::scip_line_to_display(r.line);
                            json!({
                                "name": r.name,
                                "kind": r.kind,
                                "file_path": r.file_path,
                                "line": display_line,
                                "score": r.score,
                            })
                        })
                        .collect();
                    let mut payload = json!({
                        "references": refs_json,
                        "count": count,
                        "returned_count": count,
                        "sampled": false,
                        "backend": "scip",
                        "evidence": evidence,
                    });
                    if let Some(stale) = scip_staleness.as_ref()
                        && let Some(map) = payload.as_object_mut()
                    {
                        map.insert(
                            "scip_index_stale_warning".to_owned(),
                            crate::tools::scip_health::scip_stale_warning_payload(stale),
                        );
                    }
                    // P7 add-1: the SCIP precise path is the 5th complete-result
                    // path (tree-sitter default, LSP fallback, P4 merge, text
                    // fallback are the other four). Route it through the same
                    // seam so full_results=true preserves every precise row
                    // through response summarization instead of clipping to the
                    // preview cap (defect: full_results=true still returned n=3).
                    mark_full_results(&mut payload, full_results);
                    if !unknown_args.is_empty() || !deprecation_warnings.is_empty() {
                        insert_response_annotations(
                            &mut payload,
                            &unknown_args,
                            &deprecation_warnings,
                        );
                    }
                    return Ok((payload, meta));
                }
            }
        }

        // Regression [C]: the default path (no explicit use_lsp) must return the
        // same result whether or not a language server happens to be warm. A
        // prior warm-LSP hijack routed warm servers through LSP here, producing
        // a different — and often under-complete — result than the cold
        // tree-sitter path for the SAME request, and silently dropping the
        // full_results marker (n=3, truncated=true). The default path now stays
        // on tree-sitter unconditionally; when the language has a server
        // mapping it only advertises that use_lsp=true adds annotation-aware
        // precision (explicit opt-in), so warmth never changes the answer.
        let cold_lsp_hint: Option<Value> =
            default_lsp_command_for_path(&file_path).map(|command| {
                json!({
                    "code": "lsp_precision_available",
                    "server": command,
                    "message": format!(
                        "tree-sitter references can miss import and type-annotation usages for this language. Re-run with use_lsp=true for annotation-aware precise references via `{command}`."
                    ),
                    "recommended_action": "retry_with_use_lsp_true",
                })
            });

        return Ok(find_referencing_symbols_via_text(
            &state.project(),
            sym_name,
            Some(&file_path),
            max_results,
        )
        .map(|value| {
            let (references, total_count, sampled) =
                compact_text_references(value, include_context, full_results, sample_limit);
            let meta = success_meta(BackendKind::TreeSitter, 0.85);
            let evidence = crate::tool_evidence::tool_evidence(
                "references",
                &meta,
                "tree_sitter_text_references",
                crate::tool_evidence::precision_signals(
                    precise_available,
                    false,
                    precise_source,
                    Some("tree_sitter"),
                    0,
                ),
            );
            let mut payload = json!({
                "references": references,
                "count": total_count,
                "returned_count": references.len(),
                "sampled": sampled,
                "include_context": include_context,
                "evidence": evidence,
            });
            mark_full_results(&mut payload, full_results);
            if let Some(hint) = cold_lsp_hint
                && let Some(map) = payload.as_object_mut()
            {
                map.insert("lsp_precision_hint".to_owned(), hint);
            }
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, meta)
        })?);
    }

    // LSP path: explicit use_lsp=true or position-based lookup
    let (line, column) = match (
        arguments.get("line").and_then(|v| v.as_u64()),
        arguments.get("column").and_then(|v| v.as_u64()),
    ) {
        (Some(l), Some(c)) => (l as usize, c as usize),
        _ => {
            if let Some(sym_name) = symbol_name_param {
                resolve_symbol_position(state, sym_name, &file_path).unwrap_or((0, 0))
            } else {
                return Err(CodeLensError::MissingParam(
                    "line+column or symbol_name".into(),
                ));
            }
        }
    };

    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path));
    let lsp_command_attempted = command.is_some();

    if let Some(command) = command {
        let args = parse_lsp_args(arguments, &command);
        // P3: an explicit use_lsp request must not silently miss references
        // just because the server was cold — wait (bounded) for the freshly
        // spawned server to finish indexing and retry, instead of returning
        // the empty first read. Warm servers pay nothing; the wait is capped.
        let request = LspRequest {
            command: command.clone(),
            args: args.clone(),
            file_path: file_path.clone(),
            line,
            column,
            max_results,
        };
        let pool = state.lsp_pool();
        let lsp_result = lsp_references_with_cold_wait(&pool, &request)
            .map_err(|e| enhance_lsp_error(e, &command));

        match lsp_result {
            Ok(value) => {
                let precise_count = value.len();
                let structural_evidence = if precise_count <= 1 {
                    symbol_name_param.and_then(|symbol_name| {
                        structural_ts_reference_evidence(
                            state,
                            &file_path,
                            symbol_name,
                            max_results,
                        )
                    })
                } else {
                    None
                };
                let structural_count = structural_evidence_count(&structural_evidence);
                // Regression [B]: when NO TS structural evidence already covers a
                // low LSP count, cross-check against the tree-sitter text scan
                // and prefer the fuller set at reduced confidence — a low LSP
                // count is often an under-report (cold / partial index, or a
                // declaration-only answer). The structural-evidence path below
                // owns the TS case with its own degrade, so gate on
                // `structural_count == 0` to leave it untouched.
                if structural_count == 0
                    && precise_count <= LSP_UNDERREPORT_PROBE_MAX
                    && let Some(symbol_name) = symbol_name_param
                    && let Ok(text_refs) = find_referencing_symbols_via_text(
                        &state.project(),
                        symbol_name,
                        Some(&file_path),
                        max_results,
                    )
                    && lsp_underreports_vs_text(precise_count, text_refs.len())
                {
                    let text_count = text_refs.len();
                    let (references, total_count, sampled) = compact_text_references(
                        text_refs,
                        include_context,
                        full_results,
                        sample_limit,
                    );
                    let meta = meta_degraded("hybrid", 0.6, "lsp_underreport_vs_text_backend");
                    let evidence = crate::tool_evidence::tool_evidence(
                        "references",
                        &meta,
                        "lsp_underreport_text_fallback",
                        crate::tool_evidence::precision_signals(
                            true,
                            false,
                            Some("lsp"),
                            Some("tree_sitter"),
                            total_count,
                        ),
                    );
                    let mut payload = json!({
                        "references": references,
                        "count": total_count,
                        "returned_count": references.len(),
                        "sampled": sampled,
                        "include_context": include_context,
                        "backend": "tree_sitter",
                        "evidence": evidence,
                        "lsp_underreport_warning": {
                            "code": "lsp_underreport_vs_text_backend",
                            "lsp_count": precise_count,
                            "text_count": text_count,
                            "message": "The LSP backend resolved far fewer references than the tree-sitter text scan (likely a cold or partial index, or a declaration-only result). Returning the fuller text-backend set at reduced confidence; re-run once the language server is warm for precise references.",
                            "recommended_action": "retry_when_lsp_warm",
                        },
                    });
                    mark_full_results(&mut payload, full_results);
                    insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
                    return Ok((payload, meta));
                }
                // P1.1b: same readiness calibration as the warm default path.
                // The request above harvested any pending serverStatus
                // notifications, so this is the freshest readiness state.
                let quiescence = state
                    .lsp_pool()
                    .warm_session_quiescence(&command, &args)
                    .flatten();
                let (lsp_confidence, lsp_degraded_reason, lsp_basis) =
                    lsp_confidence_for_quiescence(
                        quiescence,
                        "lsp_precise_quiescent",
                        "lsp_precise",
                    );
                let meta = if structural_count > 0 && precise_count <= 1 {
                    meta_degraded("hybrid", 0.72, "lsp_low_count_plus_ts_structural_evidence")
                } else {
                    match lsp_degraded_reason {
                        Some(reason) => meta_degraded("lsp", lsp_confidence, reason),
                        None => meta_for_backend("lsp", lsp_confidence),
                    }
                };
                let confidence_basis = if structural_count > 0 && precise_count <= 1 {
                    "lsp_low_count_plus_ts_structural_evidence"
                } else {
                    lsp_basis
                };
                let evidence = crate::tool_evidence::tool_evidence(
                    "references",
                    &meta,
                    confidence_basis,
                    crate::tool_evidence::precision_signals(
                        true,
                        true,
                        Some("lsp"),
                        None,
                        precise_count,
                    ),
                );
                let mut payload = json!({
                    "references": value,
                    "count": precise_count,
                    "returned_count": precise_count,
                    "sampled": false,
                    "evidence": evidence,
                });
                insert_structural_ts_evidence(&mut payload, structural_evidence, precise_count);
                if !unknown_args.is_empty() || !deprecation_warnings.is_empty() {
                    insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
                }
                return Ok((payload, meta));
            }
            Err(_) => {
                // LSP failed — fall through to tree-sitter
            }
        }
    }

    // Fallback: tree-sitter text search
    let word = symbol_name_param
        .map(ToOwned::to_owned)
        .or_else(|| extract_word_at_position(&state.project(), &file_path, line, column).ok())
        .ok_or_else(|| CodeLensError::MissingParam("could not determine symbol name".into()))?;
    Ok(
        find_referencing_symbols_via_text(&state.project(), &word, Some(&file_path), max_results)
            .map(|value| {
            let (references, total_count, sampled) =
                compact_text_references(value, include_context, full_results, sample_limit);
            let mut meta = success_meta(BackendKind::TreeSitter, 0.85);
            meta.degraded_reason = Some("LSP failed, used tree-sitter".to_owned());
            let evidence = crate::tool_evidence::tool_evidence(
                "references",
                &meta,
                "tree_sitter_text_references",
                crate::tool_evidence::precision_signals(
                    lsp_command_attempted,
                    false,
                    lsp_command_attempted.then_some("lsp"),
                    Some("tree_sitter"),
                    0,
                ),
            );
            let mut payload = json!({
                "references": references,
                "count": total_count,
                "returned_count": references.len(),
                "sampled": sampled,
                "include_context": include_context,
                "evidence": evidence,
            });
            mark_full_results(&mut payload, full_results);
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, meta)
        })?,
    )
}

#[cfg(test)]
mod warm_lsp_routing_tests {
    #[test]
    fn quiescence_calibration_degrades_only_verified_indexing() {
        use super::lsp_confidence_for_quiescence;
        // Server explicitly reports indexing in progress → degraded label.
        let (confidence, reason, basis) =
            lsp_confidence_for_quiescence(Some(false), "quiescent_basis", "unknown_basis");
        assert!(confidence < 0.95, "indexing-in-progress must not keep 0.95");
        assert_eq!(reason, Some("lsp_server_indexing_in_progress"));
        assert_eq!(basis, "lsp_warm_indexing_in_progress");
        // Verified quiescent → full precise label with the quiescent basis.
        let (confidence, reason, basis) =
            lsp_confidence_for_quiescence(Some(true), "quiescent_basis", "unknown_basis");
        assert_eq!((confidence, reason, basis), (0.95, None, "quiescent_basis"));
        // No readiness signal (e.g. pyright) → legacy label, no false distrust.
        let (confidence, reason, basis) =
            lsp_confidence_for_quiescence(None, "quiescent_basis", "unknown_basis");
        assert_eq!((confidence, reason, basis), (0.95, None, "unknown_basis"));
    }
}

#[cfg(test)]
mod cross_file_merge_tests {
    use super::merge_caller_rows_dedup;
    use codelens_engine::CallerEntry;
    use serde_json::Value;
    use std::collections::HashSet;

    fn caller(file: &str, function: &str, line: usize) -> CallerEntry {
        CallerEntry {
            file: file.to_owned(),
            function: function.to_owned(),
            line,
            confidence: 0.9,
            resolution: Some("import_map"),
        }
    }

    #[test]
    fn merges_cross_file_callers_and_tags_backend() {
        // The oxc self-only result is the definition at src/actions.ts:3.
        // import_graph reports two cross-file callers — both must merge in and
        // be tagged with the import_graph backend so the evidence is
        // self-describing.
        let seen: HashSet<(String, usize)> = [("src/actions.ts".to_owned(), 3)].into();
        let callers = vec![
            caller("src/handler.ts", "handleRequest", 12),
            caller("src/page.tsx", "render", 40),
        ];
        let rows = merge_caller_rows_dedup(callers, seen, 20);
        assert_eq!(
            rows.len(),
            2,
            "both cross-file callers must merge, got {rows:?}"
        );
        for row in &rows {
            assert_eq!(row["backend"], Value::String("import_graph".to_owned()));
            assert_eq!(row["kind"], Value::String("cross_file_caller".to_owned()));
            assert!(row["file_path"].is_string() && row["line"].is_u64());
        }
    }

    #[test]
    fn dedups_same_file_oxc_line_and_duplicate_callers() {
        // A same-file caller oxc already reported (src/actions.ts:3) must not be
        // re-added, and a duplicate (file, line) among the import_graph rows
        // must collapse to one — the merged set is a true union.
        let seen: HashSet<(String, usize)> = [("src/actions.ts".to_owned(), 3)].into();
        let callers = vec![
            caller("src/actions.ts", "applyAction", 3), // already in oxc result
            caller("src/handler.ts", "handleRequest", 12),
            caller("src/handler.ts", "handleRequest", 12), // duplicate
        ];
        let rows = merge_caller_rows_dedup(callers, seen, 20);
        assert_eq!(
            rows.len(),
            1,
            "same-file oxc line and duplicate caller must be deduped, got {rows:?}"
        );
        assert_eq!(
            rows[0]["file_path"],
            Value::String("src/handler.ts".to_owned())
        );
    }

    #[test]
    fn drops_non_js_ts_callers_and_respects_cap() {
        // Non-JS/TS callers are not part of the oxc reference surface and must
        // be filtered; the result is capped at max_results.
        let seen: HashSet<(String, usize)> = HashSet::new();
        let callers = vec![
            caller("src/native.rs", "call_it", 5), // non-TS — dropped
            caller("src/a.ts", "a", 1),
            caller("src/b.ts", "b", 2),
            caller("src/c.ts", "c", 3),
        ];
        let rows = merge_caller_rows_dedup(callers, seen, 2);
        assert_eq!(
            rows.len(),
            2,
            "cap must bound the merged rows, got {rows:?}"
        );
        assert!(
            rows.iter()
                .all(|r| r["file_path"].as_str().unwrap().ends_with(".ts")),
            "non-JS/TS caller must be filtered, got {rows:?}"
        );
    }
}

#[cfg(test)]
mod regression_fix_tests {
    use super::{lsp_underreports_vs_text, mark_full_results};
    use serde_json::{Value, json};

    #[test]
    fn lsp_underreport_guard_fires_only_below_half_the_text_count() {
        // Regression [B]: pyright returned only the definition (1) while the
        // text scan found the full 17 — the guard must fire and prefer text.
        assert!(lsp_underreports_vs_text(1, 17));
        // 2 vs 5: LSP is below half → under-report.
        assert!(lsp_underreports_vs_text(2, 5));
    }

    #[test]
    fn lsp_underreport_guard_stays_quiet_when_counts_agree() {
        // At or above half the text count the LSP result is plausibly complete —
        // no fallback, no confidence downgrade.
        assert!(!lsp_underreports_vs_text(3, 5), "3 is >50% of 5");
        assert!(!lsp_underreports_vs_text(5, 5), "equal counts agree");
        assert!(!lsp_underreports_vs_text(1, 1), "both minimal — no signal");
        assert!(!lsp_underreports_vs_text(0, 0), "empty on both sides");
        assert!(
            !lsp_underreports_vs_text(20, 10),
            "LSP finding more than text is never an under-report"
        );
    }

    #[test]
    fn mark_full_results_sets_marker_only_when_requested() {
        // Regression [D]: the merge/fallback paths must attach the completeness
        // marker when full_results is requested so summarization preserves the
        // whole array (no n=3/truncated clip); a non-full_results call is
        // untouched so the default sampling contract is preserved.
        let mut requested = json!({"references": [1, 2, 3], "count": 3});
        mark_full_results(&mut requested, true);
        assert_eq!(requested.get("full_results"), Some(&Value::Bool(true)));

        let mut default = json!({"references": [1, 2, 3], "count": 3});
        mark_full_results(&mut default, false);
        assert!(
            default.get("full_results").is_none(),
            "default path must not gain the marker, got {default:?}"
        );
    }

    #[cfg(feature = "scip-backend")]
    #[test]
    fn scip_undercount_fallback_fires_on_fresh_and_stale_and_labels_them() {
        use super::scip_undercount_fallback;
        // AC (f): a FRESH index whose SCIP count (12) trails the text scan (16)
        // must fire the fallback — the coverage-gap case the stale-only guard
        // missed (live probe: benches/indexing.rs rows SCIP never indexed) —
        // and carry the fresh reason so downstream can tell it from a stale
        // index.
        assert_eq!(
            scip_undercount_fallback(12, 16, false),
            Some(("scip_undercount_vs_text", "scip_undercount_text_fallback")),
            "fresh undercount must fire with the coverage-gap reason"
        );
        // A STALE index keeps its distinct #251 reason.
        assert_eq!(
            scip_undercount_fallback(12, 16, true),
            Some((
                "scip_stale_undercount_vs_text",
                "scip_stale_undercount_text_fallback"
            )),
            "stale undercount keeps the #251 reason"
        );
        // Complete or over-complete SCIP counts never fall back — the 0.98
        // precise tier stands, fresh or stale.
        assert_eq!(
            scip_undercount_fallback(16, 16, false),
            None,
            "equal counts: precise tier stands"
        );
        assert_eq!(
            scip_undercount_fallback(20, 16, true),
            None,
            "SCIP over-count never triggers a text fallback"
        );
    }
}
