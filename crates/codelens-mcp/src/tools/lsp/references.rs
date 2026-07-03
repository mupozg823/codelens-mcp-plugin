use super::super::{
    AppState, ToolResult, default_lsp_args_for_command, default_lsp_command_for_path,
    optional_bool, optional_string, optional_usize, parse_lsp_args, success_meta,
};
use super::rename::resolve_symbol_position;
use super::shared::{enhance_lsp_error, insert_response_annotations, resolve_path_argument};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_evidence::{meta_degraded, meta_for_backend};
use codelens_engine::{LspRequest, extract_word_at_position, find_referencing_symbols_via_text};
use serde_json::{Value, json};

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

/// Warm-LSP precision stage for the default reference path.
///
/// The default path (symbol_name only, no `use_lsp`) is latency-sensitive and
/// must never trigger an LSP cold start (2-30s). This stage only upgrades to
/// precise LSP references when the file's language server is **already** warm
/// in the pool. For Python it closes the tree-sitter extractor gap on import
/// and type-annotation references (CLAUDE.md "Known accuracy limits").
#[derive(Debug, PartialEq, Eq)]
enum WarmLspStage {
    /// A warm server is resident — route through precise LSP references.
    UseLsp { command: String },
    /// The language has an LSP mapping but the server is cold — stay on
    /// tree-sitter and surface a hint toward `use_lsp=true`.
    ColdHint { command: String },
    /// No LSP mapping for this language — plain tree-sitter, no hint.
    NoMapping,
}

/// Pure routing decision, isolated from pool I/O so warmth can be injected in
/// tests. `lsp_command` is the file's default LSP binary (if any); `is_warm`
/// reports whether that binary already has a live pool session and is only
/// consulted when a mapping exists.
fn decide_warm_lsp_stage(
    lsp_command: Option<String>,
    is_warm: impl FnOnce(&str) -> bool,
) -> WarmLspStage {
    match lsp_command {
        Some(command) if is_warm(&command) => WarmLspStage::UseLsp { command },
        Some(command) => WarmLspStage::ColdHint { command },
        None => WarmLspStage::NoMapping,
    }
}

/// Factual `routing_note` prose for the warm-LSP stage. The warmth probe and
/// the reference request are separate lock acquisitions, so a warm server can
/// die in between and be respawned mid-request; `cold_start_incurred` reflects
/// what actually happened for this call so the note never over-claims "no cold
/// start". Kept pure so the flag→prose mapping is unit-testable.
fn warm_lsp_routing_rationale(cold_start_incurred: bool) -> &'static str {
    if cold_start_incurred {
        "The warmth probe passed but the LSP session had died and was respawned mid-request, so a cold start was incurred before precise references were returned."
    } else {
        "A warm LSP server was already resident, so the default path routed through precise LSP references to capture import and type-annotation usages tree-sitter misses. No cold start was incurred."
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

        #[cfg(feature = "scip-backend")]
        if let Some(backend) = state.scip() {
            use codelens_engine::PreciseBackend as _;
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

        // Warm-LSP precision stage: upgrade to precise LSP references *only*
        // when the file's language server is already warm in the pool. Never
        // cold-starts here — that would break the default path's latency
        // contract. Closes the tree-sitter gap on Python (pyright) import and
        // type-annotation references. oxc/SCIP stages above are unchanged.
        let lsp_command = default_lsp_command_for_path(&file_path);
        let lsp_args: Vec<String> = lsp_command
            .as_deref()
            .map(default_lsp_args_for_command)
            .unwrap_or_default();
        let mut cold_lsp_hint: Option<Value> = None;
        match decide_warm_lsp_stage(lsp_command, |command| {
            state.lsp_pool().has_warm_session(command, &lsp_args)
        }) {
            WarmLspStage::UseLsp { command } => {
                // The warmth probe above just confirmed this server was
                // resident, so the request below almost always reuses the live
                // session. `find_referencing_symbols_tracking_spawn` also
                // reports whether it actually had to spawn — covering the rare
                // TOCTOU case where the server died between the probe and this
                // call and was respawned mid-request — so the routing_note can
                // state truthfully whether a cold start occurred. If the
                // symbol position cannot be resolved or the server returns
                // nothing, fall through to tree-sitter (no hint — it is warm).
                if let Some((line, column)) = resolve_symbol_position(state, sym_name, &file_path)
                    && let Ok((refs, cold_start_incurred)) = state
                        .lsp_pool()
                        .find_referencing_symbols_tracking_spawn(&LspRequest {
                            command: command.clone(),
                            args: lsp_args.clone(),
                            file_path: file_path.clone(),
                            line,
                            column,
                            max_results,
                        })
                    && !refs.is_empty()
                {
                    let precise_count = refs.len();
                    let meta = meta_for_backend("lsp", 0.95);
                    let evidence = crate::tool_evidence::tool_evidence(
                        "references",
                        &meta,
                        "lsp_precise_warm_routed",
                        crate::tool_evidence::precision_signals(
                            true,
                            true,
                            Some("lsp"),
                            None,
                            precise_count,
                        ),
                    );
                    let mut payload = json!({
                        "references": refs,
                        "count": precise_count,
                        "returned_count": precise_count,
                        "sampled": false,
                        "backend": "lsp",
                        "evidence": evidence,
                        "routing_note": {
                            "stage": "warm_lsp_default_path",
                            "server": command,
                            "cold_start_incurred": cold_start_incurred,
                            "rationale": warm_lsp_routing_rationale(cold_start_incurred),
                        },
                    });
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
            WarmLspStage::ColdHint { command } => {
                cold_lsp_hint = Some(json!({
                    "code": "lsp_server_cold",
                    "server": command,
                    "message": format!(
                        "tree-sitter references can miss import and type-annotation usages for this language. `{command}` is not warm, so the default path stayed on tree-sitter to preserve latency. Re-run with use_lsp=true for annotation-aware precise references."
                    ),
                    "recommended_action": "retry_with_use_lsp_true",
                }));
            }
            WarmLspStage::NoMapping => {}
        }

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
        let lsp_result = state
            .lsp_pool()
            .find_referencing_symbols(&LspRequest {
                command: command.clone(),
                args,
                file_path: file_path.clone(),
                line,
                column,
                max_results,
            })
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
                let meta = if structural_count > 0 && precise_count <= 1 {
                    meta_degraded("hybrid", 0.72, "lsp_low_count_plus_ts_structural_evidence")
                } else {
                    meta_for_backend("lsp", 0.95)
                };
                let confidence_basis = if structural_count > 0 && precise_count <= 1 {
                    "lsp_low_count_plus_ts_structural_evidence"
                } else {
                    "lsp_precise"
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
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, meta)
        })?,
    )
}

#[cfg(test)]
mod warm_lsp_routing_tests {
    use super::{WarmLspStage, decide_warm_lsp_stage, warm_lsp_routing_rationale};

    #[test]
    fn rationale_claims_no_cold_start_only_when_session_was_reused() {
        let reused = warm_lsp_routing_rationale(false);
        assert!(
            reused.contains("No cold start was incurred"),
            "reused-session note must state no cold start: {reused}"
        );
        assert!(
            !reused.to_ascii_lowercase().contains("respawn"),
            "reused-session note must not mention a respawn: {reused}"
        );
    }

    #[test]
    fn rationale_admits_cold_start_when_session_respawned() {
        let respawned = warm_lsp_routing_rationale(true);
        assert!(
            respawned.contains("cold start was incurred"),
            "respawn note must admit the cold start: {respawned}"
        );
        assert!(
            !respawned.contains("No cold start was incurred"),
            "respawn note must not falsely claim no cold start: {respawned}"
        );
    }

    #[test]
    fn warm_server_routes_to_lsp() {
        let decision = decide_warm_lsp_stage(Some("pyright-langserver".to_owned()), |cmd| {
            assert_eq!(cmd, "pyright-langserver");
            true
        });
        assert_eq!(
            decision,
            WarmLspStage::UseLsp {
                command: "pyright-langserver".to_owned()
            }
        );
    }

    #[test]
    fn cold_server_falls_back_with_hint() {
        let decision = decide_warm_lsp_stage(Some("pyright-langserver".to_owned()), |_| false);
        assert_eq!(
            decision,
            WarmLspStage::ColdHint {
                command: "pyright-langserver".to_owned()
            }
        );
    }

    #[test]
    fn unmapped_language_stays_plain_tree_sitter_without_probing() {
        // Warmth must not be probed at all when there is no LSP mapping —
        // the closure panics if consulted, proving the short-circuit.
        let decision = decide_warm_lsp_stage(None, |_| {
            panic!("warmth must not be probed without a mapping")
        });
        assert_eq!(decision, WarmLspStage::NoMapping);
    }
}
