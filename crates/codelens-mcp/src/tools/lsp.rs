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

/// Thin wrapper: see `build_text_refs_response_with_decisions`. Used by
/// the primary (non-fallback) code path where only the `sampling`
/// decision can arise.
fn build_text_refs_response(
    references: Vec<serde_json::Value>,
    total_count: usize,
    sampled: bool,
    include_context: bool,
) -> serde_json::Value {
    build_text_refs_response_with_decisions(
        references,
        total_count,
        sampled,
        include_context,
        Vec::new(),
    )
}

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
            let shadow_count = report.shadow_files_suppressed.len();
            let (references, total_count, sampled) =
                compact_text_references(report.references, include_context, full_results, sample_limit);
            let mut extra: Vec<LimitsApplied> = Vec::new();
            if shadow_count > 0 {
                extra.push(LimitsApplied::shadow_suppression(shadow_count));
            }
            let envelope = build_text_refs_response_with_decisions(
                references, total_count, sampled, include_context, extra,
            );
            let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
            // TODO(Task 8): merge envelope["_meta"]["decisions"] into the outgoing
            // ToolResponseMeta so the MCP envelope carries the same decisions.
            (data, meta_for_backend("tree_sitter", 0.85))
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
                return Ok((
                    json!({
                        "references": value,
                        "count": value.len(),
                        "returned_count": value.len(),
                        "sampled": false,
                    }),
                    meta_for_backend("lsp", 0.95),
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
                let shadow_count = report.shadow_files_suppressed.len();
                let (references, total_count, sampled) =
                    compact_text_references(report.references, include_context, full_results, sample_limit);
                let mut extra: Vec<LimitsApplied> = Vec::new();
                if shadow_count > 0 {
                    extra.push(LimitsApplied::shadow_suppression(shadow_count));
                }
                let envelope = build_text_refs_response_with_decisions(
                    references, total_count, sampled, include_context, extra,
                );
                let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
                // TODO(Task 8): merge envelope["_meta"]["decisions"] into the outgoing
                // ToolResponseMeta so the MCP envelope carries the same decisions.
                (data, meta_degraded("tree_sitter_fallback", 0.85, "LSP failed, used tree-sitter"))
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
                (
                    json!(value),
                    meta_degraded(
                        "tree-sitter-native",
                        0.80,
                        "LSP failed, fell back to native",
                    ),
                )
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
            (
                json!(value),
                meta_degraded("tree-sitter-native", 0.80, "no LSP command available"),
            )
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
    use super::build_text_refs_response;
    use serde_json::json;

    #[test]
    fn notice_and_limits_are_absent_when_not_sampled() {
        let resp =
            build_text_refs_response(vec![json!({"file_path": "a.py", "line": 1})], 1, false, false);
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
        let resp = build_text_refs_response(refs, 62, true, false);
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
}
