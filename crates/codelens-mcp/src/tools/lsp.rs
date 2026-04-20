use super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_bool, optional_string,
    optional_usize, parse_lsp_args, required_string, success_meta,
};
use crate::authority::{meta_degraded, meta_for_backend};
use crate::error::CodeLensError;
use crate::limits::{self, LimitsApplied};
use crate::protocol::BackendKind;
use codelens_engine::{
    LspDiagnosticRequest, LspRenamePlanRequest, LspRequest, LspTypeHierarchyRequest,
    LspWorkspaceSymbolRequest, check_lsp_status as core_check_lsp_status, extract_word_at_position,
    find_referencing_symbols_via_text, get_lsp_recipe as core_get_lsp_recipe,
    get_type_hierarchy_native,
};
use serde_json::json;

/// Build the `find_referencing_symbols` text-path response envelope
/// `{ data, _meta }`. Emits the `sampling` decision internally when
/// `sampled == true`, and appends `extra_decisions` (e.g.
/// `shadow_suppression`, `backend_degraded`) so the caller can tack
/// on decisions that only it knows about (file-count from the
/// engine; LSP-fallback signal from the handler branch).
///
/// `data.limits_applied` and `_meta.decisions` are always present,
/// byte-identical, and possibly an empty array.
pub(super) fn build_text_refs_response_with_decisions(
    references: Vec<serde_json::Value>,
    total_count: usize,
    sampled: bool,
    include_context: bool,
    extra_decisions: Vec<LimitsApplied>,
) -> serde_json::Value {
    let returned_count = references.len();
    let mut data = json!({
        "references": references,
        "count": total_count,
        "returned_count": returned_count,
        "sampled": sampled,
        "include_context": include_context,
    });
    let mut meta = json!({});

    let mut decisions: Vec<LimitsApplied> = Vec::with_capacity(1 + extra_decisions.len());
    if sampled {
        let entry = LimitsApplied::sampling(total_count, returned_count, "sample_limit");
        data["sampling_notice"] = json!(format!(
            "Returned {returned_count} of {total_count} matches (sampled). \
             Set `full_results=true` or raise `max_results` to retrieve the full set."
        ));
        decisions.push(entry);
    }
    decisions.extend(extra_decisions);

    limits::inject_into(&mut data, &mut meta, &decisions);
    json!({ "data": data, "_meta": meta })
}

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
    // Serena-parity output: always surface the enclosing symbol as
    // `container` (name_path + signature + line range) and a `snippet`
    // object with the matched line decorated so the harness can orient
    // itself in one response instead of following up with a Read.
    // `include_context=true` still adds the legacy `line_content` and
    // `enclosing_symbol` fields for backward compatibility.
    let compact = references
        .into_iter()
        .take(effective_limit)
        .map(|reference| {
            let container = reference.enclosing_symbol.as_ref().map(|symbol| {
                json!({
                    "name_path": symbol.name_path,
                    "kind": symbol.kind,
                    "signature": symbol.signature,
                    "start_line": symbol.start_line,
                    "end_line": symbol.end_line,
                })
            });
            let line_text = reference.line_content.trim_end_matches('\n');
            // Build the `> N: text` decorated window Serena emits as
            // `content_around_reference`: preceding lines prefixed with
            // `... N:`, the match line with `> N:`, following lines
            // prefixed with `... N:`. `before`/`after` arrays stay
            // available separately so a programmatic consumer can
            // skip the decoration.
            let match_line_number = reference.line;
            let before_start_line = reference
                .line
                .saturating_sub(reference.context_before.len());
            let before_decorated: Vec<String> = reference
                .context_before
                .iter()
                .enumerate()
                .map(|(idx, text)| format!("... {}: {}", before_start_line + idx, text))
                .collect();
            let after_decorated: Vec<String> = reference
                .context_after
                .iter()
                .enumerate()
                .map(|(idx, text)| format!("... {}: {}", match_line_number + 1 + idx, text))
                .collect();
            let snippet = json!({
                "line": match_line_number,
                "match": format!("> {}: {}", match_line_number, line_text),
                "text": line_text,
                "before": reference.context_before,
                "after": reference.context_after,
                "before_decorated": before_decorated,
                "after_decorated": after_decorated,
            });
            let mut value = json!({
                "file_path": reference.file_path,
                "line": reference.line,
                "column": reference.column,
                "is_declaration": reference.is_declaration,
                "container": container,
                "snippet": snippet,
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

fn lsp_install_hint(command: &str) -> &'static str {
    match command {
        "pyright" => "  pip install pyright",
        "typescript-language-server" => "  npm i -g typescript-language-server typescript",
        "rust-analyzer" => "  rustup component add rust-analyzer",
        "gopls" => "  go install golang.org/x/tools/gopls@latest",
        "clangd" => "  brew install llvm  (or apt install clangd)",
        "jdtls" => "  See https://github.com/eclipse-jdtls/eclipse.jdt.ls",
        "solargraph" => "  gem install solargraph",
        "intelephense" => "  npm i -g intelephense",
        "kotlin-language-server" => "  See https://github.com/fwcd/kotlin-language-server",
        "metals" => "  cs install metals  (via Coursier)",
        "sourcekit-lsp" => "  Included with Xcode / Swift toolchain",
        "csharp-ls" => "  dotnet tool install -g csharp-ls",
        "dart" => "  dart pub global activate dart_language_server",
        // Phase 6a languages
        "lua-language-server" => "  brew install lua-language-server",
        "zls" => "  brew install zls",
        "nextls" => "  mix escript.install hex next_ls",
        "haskell-language-server-wrapper" => "  ghcup install hls",
        "ocamllsp" => "  opam install ocaml-lsp-server",
        "erlang_ls" => "  brew install erlang_ls",
        "bash-language-server" => "  npm i -g bash-language-server",
        _ => "  Check your package manager for the LSP server binary",
    }
}

fn enhance_lsp_error(err: anyhow::Error, command: &str) -> CodeLensError {
    let msg = err.to_string();
    if msg.contains("No such file") || msg.contains("not found") || msg.contains("spawn") {
        CodeLensError::LspNotAttached(format!(
            "LSP server '{command}' not found. Install it:\n{}",
            lsp_install_hint(command)
        ))
    } else if msg.contains("timed out") || msg.contains("timeout") {
        CodeLensError::Timeout {
            operation: format!("LSP {command}"),
            elapsed_ms: 30_000,
        }
    } else {
        CodeLensError::LspError(msg)
    }
}

/// Shared tail for both `find_referencing_symbols` text-path branches:
/// turn a `TextRefsReport` into the `(data, ToolResponseMeta)` tuple,
/// attaching the decisions array (sampling + shadow_suppression +
/// optional leading `backend_degraded`) to both the envelope's
/// `data.limits_applied` and `meta.decisions`.
///
/// `leading_decisions` is pushed before shadow_suppression; pass
/// `vec![LimitsApplied::backend_degraded(...)]` on the LSP-fallback
/// path, or `Vec::new()` on the primary path.
fn finalize_text_refs_response(
    report: codelens_engine::TextRefsReport,
    include_context: bool,
    full_results: bool,
    sample_limit: usize,
    leading_decisions: Vec<LimitsApplied>,
    mut meta: crate::protocol::ToolResponseMeta,
) -> (serde_json::Value, crate::protocol::ToolResponseMeta) {
    let shadow_count = report.shadow_files_suppressed.len();
    let (references, total_count, sampled) =
        compact_text_references(report.references, include_context, full_results, sample_limit);
    let mut extra = leading_decisions;
    if shadow_count > 0 {
        extra.push(LimitsApplied::shadow_suppression(shadow_count));
    }
    let envelope = build_text_refs_response_with_decisions(
        references,
        total_count,
        sampled,
        include_context,
        extra,
    );
    let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
    let decisions_array = envelope
        .get("_meta")
        .and_then(|m| m.get("decisions"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    meta.decisions = decisions_array;
    (data, meta)
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
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let symbol_name_param = optional_string(arguments, "symbol_name");
    let max_results = optional_usize(arguments, "max_results", 20);
    let use_lsp = optional_bool(arguments, "use_lsp", false);
    let include_context = optional_bool(arguments, "include_context", false);
    let full_results = optional_bool(arguments, "full_results", false);
    let sample_limit = optional_usize(arguments, "sample_limit", 8);

    let has_position = arguments.get("line").is_some() && arguments.get("column").is_some();

    // Default: scope analysis (fast, zero-config, works on broken code)
    if !use_lsp && !has_position {
        let sym_name = symbol_name_param.ok_or_else(|| {
            CodeLensError::MissingParam("symbol_name (or line+column with use_lsp=true)".into())
        })?;

        // JS/TS: use oxc_semantic for precise scope-aware reference resolution
        let resolved = state.project().resolve(&file_path)?;
        if codelens_engine::oxc_analysis::is_js_ts(&resolved)
            && let Ok(source) = std::fs::read_to_string(&resolved)
            && let Ok(refs) = codelens_engine::oxc_analysis::find_references_precise(
                &source, &file_path, sym_name,
            )
            && !refs.is_empty()
        {
            let refs_limited: Vec<_> = refs.into_iter().take(max_results).collect();
            let count = refs_limited.len();
            return Ok((
                json!({
                    "references": refs_limited,
                    "count": count,
                    "returned_count": count,
                    "sampled": false,
                    "backend": "oxc_semantic"
                }),
                meta_for_backend("oxc_semantic", 0.95),
            ));
        }
        // oxc failed or empty — try SCIP if available, then fall through to tree-sitter

        #[cfg(feature = "scip-backend")]
        if let Some(backend) = state.scip() {
            use codelens_engine::PreciseBackend as _;
            if backend.has_index_for(&file_path) {
                if let Ok(refs) = backend.find_references(sym_name, &file_path, 0) {
                    if !refs.is_empty() {
                        let limited: Vec<_> = refs.into_iter().take(max_results).collect();
                        let count = limited.len();
                        let refs_json: Vec<serde_json::Value> = limited
                            .iter()
                            .map(|r| {
                                json!({
                                    "name": r.name,
                                    "kind": r.kind,
                                    "file_path": r.file_path,
                                    "line": r.line,
                                    "score": r.score,
                                })
                            })
                            .collect();
                        return Ok((
                            json!({
                                "references": refs_json,
                                "count": count,
                                "returned_count": count,
                                "sampled": false,
                                "backend": "scip"
                            }),
                            meta_for_backend("scip", 0.98),
                        ));
                    }
                }
            }
        }

        return Ok(find_referencing_symbols_via_text(
            &state.project(),
            sym_name,
            Some(&file_path),
            max_results,
        )
        .map(|report| {
            finalize_text_refs_response(
                report,
                include_context,
                full_results,
                sample_limit,
                Vec::new(),
                meta_for_backend("tree_sitter", 0.85),
            )
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

    // P0-2: `union=true` merges LSP and tree-sitter reference sets so
    // that tree-sitter hits missed by LSP (common during LSP cold-start
    // on large repos; see `benchmarks/results/v1.9.46-lsp-reference-precision.json`)
    // do not silently drop out of the response. Default is `false` for
    // backwards compatibility — existing callers that explicitly opted
    // into `use_lsp=true` keep their LSP-only envelope.
    let union_mode = optional_bool(arguments, "union", false);

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
            Ok(lsp_refs) => {
                if !union_mode {
                    return Ok((
                        json!({
                            "references": lsp_refs,
                            "count": lsp_refs.len(),
                            "returned_count": lsp_refs.len(),
                            "sampled": false,
                            "backend": "lsp",
                        }),
                        meta_for_backend("lsp", 0.95),
                    ));
                }
                // Union path: LSP + tree-sitter. Only meaningful when we
                // can recover a symbol name for the text-search fallback.
                let sym_name_owned = symbol_name_param
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        extract_word_at_position(&state.project(), &file_path, line, column).ok()
                    });
                let ts_refs_opt = sym_name_owned.as_ref().and_then(|name| {
                    find_referencing_symbols_via_text(
                        &state.project(),
                        name,
                        Some(&file_path),
                        max_results.saturating_mul(2),
                    )
                    .ok()
                });
                // Dedupe by (file_path, line). LSP and tree-sitter may
                // report slightly different columns for the same hit, so
                // the column is not part of the key.
                let mut seen: std::collections::HashSet<(String, usize)> = lsp_refs
                    .iter()
                    .map(|r| (r.file_path.clone(), r.line))
                    .collect();
                let mut merged: Vec<serde_json::Value> = lsp_refs
                    .iter()
                    .map(|r| {
                        json!({
                            "file_path": r.file_path,
                            "line": r.line,
                            "column": r.column,
                            "source": "lsp",
                        })
                    })
                    .collect();
                let mut tree_sitter_added = 0usize;
                if let Some(ts_report) = ts_refs_opt {
                    for ts_ref in ts_report.references {
                        let key = (ts_ref.file_path.clone(), ts_ref.line);
                        if seen.insert(key) {
                            merged.push(json!({
                                "file_path": ts_ref.file_path,
                                "line": ts_ref.line,
                                "column": ts_ref.column,
                                "line_content": ts_ref.line_content,
                                "is_declaration": ts_ref.is_declaration,
                                "source": "tree_sitter",
                            }));
                            tree_sitter_added += 1;
                        }
                    }
                }
                let lsp_count = lsp_refs.len();
                let merged_count = merged.len();
                return Ok((
                    json!({
                        "references": merged,
                        "count": merged_count,
                        "returned_count": merged_count,
                        "sampled": false,
                        "backend": "union",
                        "sources": {
                            "lsp": lsp_count,
                            "tree_sitter_added": tree_sitter_added,
                            "merged": merged_count,
                        },
                    }),
                    meta_for_backend("union", 0.93),
                ));
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
            .map(|report| {
                finalize_text_refs_response(
                    report,
                    include_context,
                    full_results,
                    sample_limit,
                    vec![LimitsApplied::backend_degraded(
                        "LSP failed, used tree-sitter",
                        "tree_sitter",
                    )],
                    meta_degraded("tree_sitter_fallback", 0.85, "LSP failed, used tree-sitter"),
                )
            })?,
    )
}

/// Resolve a symbol name to its (line, column) position in a file via the symbol index.
fn resolve_symbol_position(
    state: &AppState,
    symbol_name: &str,
    file_path: &str,
) -> Option<(usize, usize)> {
    let symbols = state
        .symbol_index()
        .find_symbol(symbol_name, Some(file_path), false, true, 1)
        .ok()?;
    symbols.first().map(|s| (s.line, s.column))
}

pub fn get_file_diagnostics(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let max_results = optional_usize(arguments, "max_results", 200);

    // Try SCIP diagnostics first (if available).
    #[cfg(feature = "scip-backend")]
    if let Some(backend) = state.scip() {
        use codelens_engine::PreciseBackend as _;
        if let Ok(scip_diags) = backend.diagnostics(&file_path) {
            if !scip_diags.is_empty() {
                let limited: Vec<_> = scip_diags.into_iter().take(max_results).collect();
                let count = limited.len();
                let diags_json: Vec<serde_json::Value> = limited
                    .iter()
                    .map(|d| {
                        json!({
                            "file_path": d.file_path,
                            "line": d.line,
                            "column": d.column,
                            "severity": format!("{:?}", d.severity),
                            "message": d.message,
                            "source": "scip",
                            "code": d.code,
                        })
                    })
                    .collect();
                return Ok((
                    json!({ "diagnostics": diags_json, "count": count, "backend": "scip" }),
                    success_meta(BackendKind::Scip, 0.95),
                ));
            }
        }
    }

    // Fall back to LSP diagnostics.
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);

    let command_ref = command.clone();
    state
        .lsp_pool()
        .get_diagnostics(&LspDiagnosticRequest {
            command,
            args,
            file_path,
            max_results,
        })
        .map_err(|e| enhance_lsp_error(e, &command_ref))
        .map(|value| {
            (
                json!({ "diagnostics": value, "count": value.len() }),
                success_meta(BackendKind::Lsp, 0.9),
            )
        })
}

pub fn search_workspace_symbols(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?.to_owned();
    // `command` is the LSP server binary (rust-analyzer / pyright / gopls …).
    // When missing, point users at the non-LSP fuzzy fallback instead of the
    // generic "Missing required parameter" error so CLI one-shot callers
    // don't hit a dead end.
    let Some(command) = optional_string(arguments, "command").map(ToOwned::to_owned) else {
        return Err(CodeLensError::MissingParam(format!(
            "command (LSP server binary, e.g. rust-analyzer/pyright). \
             For LSP-free fuzzy search over `{query}`, call \
             `bm25_symbol_search` (or `find_symbol` with an exact name)."
        )));
    };
    let args = parse_lsp_args(arguments, &command);
    let max_results = optional_usize(arguments, "max_results", 50);

    let command_ref = command.clone();
    state
        .lsp_pool()
        .search_workspace_symbols(&LspWorkspaceSymbolRequest {
            command,
            args,
            query,
            max_results,
        })
        .map_err(|e| enhance_lsp_error(e, &command_ref))
        .map(|value| {
            (
                json!({ "symbols": value, "count": value.len() }),
                success_meta(BackendKind::Lsp, 0.88),
            )
        })
}

pub fn get_type_hierarchy(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = arguments
        .get("name_path")
        .or_else(|| arguments.get("fully_qualified_name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("name_path or fully_qualified_name".into()))?
        .to_owned();
    let relative_path = optional_string(arguments, "relative_path").map(ToOwned::to_owned);
    let hierarchy_type = optional_string(arguments, "hierarchy_type")
        .unwrap_or("both")
        .to_owned();
    let depth = optional_usize(arguments, "depth", 1);
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| {
            relative_path
                .as_deref()
                .and_then(default_lsp_command_for_path)
        });

    if let Some(command) = command {
        let args = parse_lsp_args(arguments, &command);
        let lsp_result = state
            .lsp_pool()
            .get_type_hierarchy(&LspTypeHierarchyRequest {
                command,
                args,
                query: query.clone(),
                relative_path: relative_path.clone(),
                hierarchy_type: hierarchy_type.clone(),
                depth: if depth == 0 { 8 } else { depth },
            });

        match lsp_result {
            Ok(value) => Ok((json!(value), meta_for_backend("lsp_pooled", 0.82))),
            Err(_) => Ok(get_type_hierarchy_native(
                &state.project(),
                &query,
                relative_path.as_deref(),
                &hierarchy_type,
                depth,
            )
            .map(|value| {
                let mut payload = json!(value);
                let mut meta = meta_degraded(
                    "tree-sitter-native",
                    0.80,
                    "LSP failed, fell back to native",
                );
                crate::tools::transparency::attach_decisions_to_meta(
                    &mut payload,
                    &mut meta,
                    vec![crate::limits::LimitsApplied::backend_degraded(
                        "LSP failed, fell back to native",
                        "tree-sitter-native",
                    )],
                );
                (payload, meta)
            })?),
        }
    } else {
        Ok(get_type_hierarchy_native(
            &state.project(),
            &query,
            relative_path.as_deref(),
            &hierarchy_type,
            depth,
        )
        .map(|value| {
            let mut payload = json!(value);
            let mut meta = meta_degraded("tree-sitter-native", 0.80, "no LSP command available");
            crate::tools::transparency::attach_decisions_to_meta(
                &mut payload,
                &mut meta,
                vec![crate::limits::LimitsApplied::backend_degraded(
                    "no LSP command available",
                    "tree-sitter-native",
                )],
            );
            (payload, meta)
        })?)
    }
}

pub fn plan_symbol_rename(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let line = arguments
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("line".into()))? as usize;
    let column = arguments
        .get("column")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("column".into()))? as usize;
    let new_name = optional_string(arguments, "new_name").map(ToOwned::to_owned);
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);

    let command_ref = command.clone();
    state
        .lsp_pool()
        .get_rename_plan(&LspRenamePlanRequest {
            command,
            args,
            file_path,
            line,
            column,
            new_name,
        })
        .map_err(|e| enhance_lsp_error(e, &command_ref))
        .map(|value| (json!(value), success_meta(BackendKind::Lsp, 0.86)))
}

pub fn check_lsp_status(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let statuses = core_check_lsp_status();
    Ok((
        json!({ "servers": statuses, "count": statuses.len() }),
        success_meta(BackendKind::Lsp, 1.0),
    ))
}

/// Snapshot per-LSP-session readiness for the current project's
/// session pool. Intended as the polling target for bench harnesses
/// and long-lived agent sessions that want to wait for LSP indexing
/// to complete instead of sleeping a fixed wall-clock duration.
///
/// Two milestones per session:
/// - `is_alive` (`ms_to_first_response != null`) — handshake + any
///   `Ok` round-trip succeeded. Useful as a liveness signal.
/// - `is_ready` (`ms_to_first_nonempty != null`) — the server has
///   returned at least one non-empty result. This is the stronger
///   signal that indexing has progressed far enough to serve real
///   caller queries; pyright and rust-analyzer both emit `[]` while
///   still walking the project, so a caller polling for `is_alive`
///   alone would unblock early.
///
/// Aggregates at the top level (`all_alive`, `all_ready`, `any_ready`)
/// let callers short-circuit the poll when every pooled session has
/// warmed without iterating the per-session array themselves. For
/// projects with no sessions yet (e.g. pre-prewarm), the aggregates
/// are conservatively `false` and the array is empty — callers should
/// treat an empty snapshot as "not ready yet" rather than "ready".
pub fn get_lsp_readiness(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let snapshots = state.lsp_pool().readiness_snapshot();

    let total = snapshots.len();
    let alive_count = snapshots.iter().filter(|s| s.is_alive()).count();
    let ready_count = snapshots.iter().filter(|s| s.is_ready()).count();

    let sessions_json: Vec<serde_json::Value> = snapshots
        .iter()
        .map(|s| {
            json!({
                "command": s.command,
                "args": s.args,
                "elapsed_ms": s.elapsed_ms,
                "ms_to_first_response": s.ms_to_first_response,
                "ms_to_first_nonempty": s.ms_to_first_nonempty,
                "ms_to_last_response": s.ms_to_last_response,
                "response_count": s.response_count,
                "nonempty_count": s.nonempty_count,
                "failure_count": s.failure_count,
                "is_alive": s.is_alive(),
                "is_ready": s.is_ready(),
            })
        })
        .collect();

    Ok((
        json!({
            "sessions": sessions_json,
            "session_count": total,
            "alive_count": alive_count,
            "ready_count": ready_count,
            "all_alive": total > 0 && alive_count == total,
            "all_ready": total > 0 && ready_count == total,
            "any_ready": ready_count > 0,
        }),
        success_meta(BackendKind::Lsp, 1.0),
    ))
}

pub fn get_lsp_recipe(_state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let extension = required_string(arguments, "extension")?;
    match core_get_lsp_recipe(extension) {
        Some(recipe) => Ok((json!(recipe), success_meta(BackendKind::Lsp, 1.0))),
        None => Err(CodeLensError::NotFound(format!(
            "LSP recipe for extension: {extension}"
        ))),
    }
}

#[cfg(test)]
mod sampling_notice_tests {
    use super::build_text_refs_response_with_decisions;
    use serde_json::json;

    #[test]
    fn notice_and_limits_are_absent_when_not_sampled() {
        let resp = build_text_refs_response_with_decisions(
            vec![json!({"file_path": "a.py", "line": 1})],
            1,
            false,
            false,
            Vec::new(),
        );
        assert_eq!(resp["data"]["sampled"], json!(false));
        assert!(resp["data"].get("sampling_notice").is_none());
        // limits_applied is ALWAYS present (possibly empty) on participating tools.
        assert_eq!(resp["data"]["limits_applied"], json!([]));
        assert_eq!(resp["_meta"]["decisions"], json!([]));
    }

    #[test]
    fn sampled_response_contains_structured_sampling_entry_and_headline_notice() {
        let refs = vec![
            json!({"file_path": "a.py", "line": 1}),
            json!({"file_path": "a.py", "line": 2}),
        ];
        let resp = build_text_refs_response_with_decisions(refs, 62, true, false, Vec::new());
        assert_eq!(resp["data"]["sampled"], json!(true));

        // structured entry
        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("sampling"));
        assert_eq!(limits[0]["total"], json!(62));
        assert_eq!(limits[0]["returned"], json!(2));
        assert_eq!(limits[0]["dropped"], json!(60));
        assert!(
            limits[0]["remedy"].as_str().unwrap().contains("full_results=true"),
            "remedy must guide caller: {}",
            limits[0]["remedy"]
        );

        // data.limits_applied == _meta.decisions (byte-equal)
        assert_eq!(resp["data"]["limits_applied"], resp["_meta"]["decisions"]);

        // human headline still present
        let notice = resp["data"]["sampling_notice"].as_str().expect("string");
        assert!(notice.contains("2 of 62"), "notice={notice}");
    }

    #[test]
    fn shadow_suppression_emits_decision_when_files_dropped() {
        use super::build_text_refs_response_with_decisions;
        use crate::limits::LimitsApplied;

        let refs = vec![json!({"file_path": "a.py", "line": 1})];
        let extra = vec![LimitsApplied::shadow_suppression(2)];
        let resp = build_text_refs_response_with_decisions(refs, 1, false, false, extra);

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("shadow_suppression"));
        assert_eq!(limits[0]["dropped"], json!(2));
        assert_eq!(resp["data"]["limits_applied"], resp["_meta"]["decisions"]);
    }

    #[test]
    fn fallback_path_emits_backend_degraded_decision() {
        use super::build_text_refs_response_with_decisions;
        use crate::limits::LimitsApplied;

        let refs = vec![json!({"file_path": "a.py", "line": 1})];
        let extra = vec![LimitsApplied::backend_degraded(
            "LSP failed, used tree-sitter",
            "tree_sitter",
        )];
        let resp = build_text_refs_response_with_decisions(refs, 1, false, false, extra);

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("backend_degraded"));
        assert!(limits[0]["reason"].as_str().unwrap().contains("LSP failed"));
        assert!(limits[0]["remedy"].as_str().unwrap().contains("tree_sitter"));
    }

    #[test]
    fn all_combinations_keep_data_and_meta_byte_equal() {
        use super::build_text_refs_response_with_decisions;
        use crate::limits::LimitsApplied;

        let scenarios: Vec<(bool, Vec<LimitsApplied>)> = vec![
            (false, vec![]),
            (true, vec![]),
            (false, vec![LimitsApplied::shadow_suppression(3)]),
            (
                true,
                vec![
                    LimitsApplied::shadow_suppression(1),
                    LimitsApplied::backend_degraded("LSP failed", "tree_sitter"),
                ],
            ),
        ];

        for (sampled, extra) in scenarios {
            let refs = vec![json!({"file_path": "a.py", "line": 1})];
            let extra_len = extra.len();
            let resp = build_text_refs_response_with_decisions(refs, 5, sampled, false, extra);
            assert_eq!(
                resp["data"]["limits_applied"], resp["_meta"]["decisions"],
                "byte-equality failed for sampled={sampled}, extra_len={extra_len}"
            );
        }
    }
}
