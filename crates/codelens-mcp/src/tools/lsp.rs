use super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_bool, optional_string,
    optional_usize, parse_lsp_args, required_string, success_meta,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_evidence::{meta_degraded, meta_for_backend};
use codelens_engine::{
    LspDiagnosticRequest, LspRenamePlanRequest, LspRequest, LspResolveTargetRequest,
    LspTypeHierarchyRequest, LspWorkspaceSymbolRequest, check_lsp_status as core_check_lsp_status,
    extract_word_at_position, find_referencing_symbols_via_text,
    get_lsp_recipe as core_get_lsp_recipe, get_type_hierarchy_native,
};
use serde_json::{Value, json};

const PATH_ALIAS_DEPRECATION: &str =
    "DEPRECATED v1.13.23 — use `path`. Soft alias maintained until v1.14.0.";

fn path_alias_warning(alias: &str) -> Value {
    json!({
        "param": alias,
        "replacement": "path",
        "message": PATH_ALIAS_DEPRECATION,
    })
}

fn resolve_path_argument(
    arguments: &serde_json::Value,
) -> Result<(&str, Vec<Value>), CodeLensError> {
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
        let mut precise_file_scope_count = 0usize;

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
                precise_file_scope_count = refs.len();
            }
        }
        // OXC is file-scoped. Keep it as evidence, but do not short-circuit
        // project-wide reference discovery for JS/TS symbols.
        // Try SCIP if available, then fall through to tree-sitter text search.

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
                    let meta = success_meta(BackendKind::Scip, 0.98);
                    let evidence = crate::tool_evidence::tool_evidence(
                        "references",
                        &meta,
                        "scip_precise",
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
                            json!({
                                "name": r.name,
                                "kind": r.kind,
                                "file_path": r.file_path,
                                "line": r.line,
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
                if precise_file_scope_count > 0 {
                    "tree_sitter_text_references_with_oxc_file_scope"
                } else {
                    "tree_sitter_text_references"
                },
                crate::tool_evidence::precision_signals(
                    precise_available,
                    false,
                    precise_source,
                    Some("tree_sitter"),
                    precise_file_scope_count,
                ),
            );
            let mut payload = json!({
                "references": references,
                "count": total_count,
                "returned_count": references.len(),
                "sampled": sampled,
                "include_context": include_context,
                "backend": if precise_file_scope_count > 0 { "tree_sitter_text_with_oxc_file_scope" } else { "tree_sitter_text" },
                "precise_file_scope_count": precise_file_scope_count,
                "evidence": evidence,
            });
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
                let meta = meta_for_backend("lsp", 0.95);
                let evidence = crate::tool_evidence::tool_evidence(
                    "references",
                    &meta,
                    "lsp_precise",
                    crate::tool_evidence::precision_signals(
                        true,
                        true,
                        Some("lsp"),
                        None,
                        value.len(),
                    ),
                );
                let mut payload = json!({
                    "references": value,
                    "count": value.len(),
                    "returned_count": value.len(),
                    "sampled": false,
                    "evidence": evidence,
                });
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
    const KNOWN_ARGS: &[&str] = &[
        "path",
        "file_path",
        "relative_path",
        "command",
        "args",
        "max_results",
    ];
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let file_path = file_path_arg.to_owned();
    let max_results = optional_usize(arguments, "max_results", 200);
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);

    // Try SCIP diagnostics first (if available).
    #[cfg(feature = "scip-backend")]
    if let Some(backend) = state.scip() {
        use codelens_engine::PreciseBackend as _;
        if let Ok(scip_diags) = backend.diagnostics(&file_path)
            && !scip_diags.is_empty()
        {
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
            let mut payload = json!({
                "diagnostics": diags_json,
                "count": count,
                "backend": "scip",
            });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            return Ok((payload, success_meta(BackendKind::Scip, 0.95)));
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
            let mut payload = json!({ "diagnostics": value, "count": value.len() });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, success_meta(BackendKind::Lsp, 0.9))
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
    // #180: prefer canonical `path`; envelope normalises `relative_path → path`
    // for this tool, so reading `path` first picks up either input shape.
    let relative_path = optional_string(arguments, "path")
        .or_else(|| optional_string(arguments, "relative_path"))
        .map(ToOwned::to_owned);
    let alias_used = optional_string(arguments, "_path_alias_source")
        .filter(|s| *s == "relative_path")
        .map(|_| {
            json!({
                "param": "relative_path",
                "replacement": "path",
                "message": "DEPRECATED v1.13.23 — use `path`. Soft alias maintained until v1.14.0.",
            })
        });
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
            Ok(value) => Ok((
                attach_alias_warning(json!(value), alias_used.clone()),
                meta_for_backend("lsp_pooled", 0.82),
            )),
            Err(_) => Ok(get_type_hierarchy_native(
                &state.project(),
                &query,
                relative_path.as_deref(),
                &hierarchy_type,
                depth,
            )
            .map(|value| {
                (
                    attach_alias_warning(json!(value), alias_used.clone()),
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
                attach_alias_warning(json!(value), alias_used),
                meta_degraded("tree-sitter-native", 0.80, "no LSP command available"),
            )
        })?)
    }
}

/// #180: append a single deprecation_warnings entry to a payload when the
/// caller used a legacy path alias. Mirrors the per-file helper in
/// filesystem.rs so all path-aliased tools emit the same shape.
fn attach_alias_warning(mut payload: Value, warning: Option<Value>) -> Value {
    if let Some(warning) = warning
        && let Some(map) = payload.as_object_mut()
    {
        map.insert(
            "deprecation_warnings".to_owned(),
            Value::Array(vec![warning]),
        );
    }
    payload
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

pub fn resolve_symbol_target(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let line = arguments
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("line".into()))? as usize;
    let column = arguments
        .get("column")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("column".into()))? as usize;
    let target = optional_string(arguments, "target")
        .unwrap_or("definition")
        .to_owned();
    let semantic_backend = optional_string(arguments, "semantic_backend").unwrap_or("lsp");
    if semantic_backend != "lsp" {
        return Err(CodeLensError::Validation(
            "resolve_symbol_target currently supports semantic_backend=lsp only".into(),
        ));
    }
    if !matches!(
        target.as_str(),
        "declaration" | "definition" | "implementation" | "type_definition"
    ) {
        return Err(CodeLensError::Validation(format!(
            "unsupported resolve target `{target}`"
        )));
    }
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let max_results = optional_usize(arguments, "max_results", 20);

    let command_ref = command.clone();
    state
        .lsp_pool()
        .resolve_symbol_target(&LspResolveTargetRequest {
            command,
            args,
            file_path: file_path.clone(),
            line,
            column,
            target: target.clone(),
            max_results,
        })
        .map_err(|e| enhance_lsp_error(e, &command_ref))
        .map(|targets| {
            let method = match target.as_str() {
                "declaration" => "textDocument/declaration",
                "definition" => "textDocument/definition",
                "implementation" => "textDocument/implementation",
                "type_definition" => "textDocument/typeDefinition",
                _ => "unknown",
            };
            (
                json!({
                    "success": true,
                    "semantic_backend": "lsp",
                    "edit_authority": {
                        "kind": "authoritative_lsp",
                        "backend": "lsp",
                        "operation": target,
                        "language": language_name_for_path(&file_path),
                        "methods": [method],
                        "embedding_used": false,
                        "search_used": false
                    },
                    "targets": targets,
                    "count": targets.len(),
                }),
                success_meta(BackendKind::Lsp, 0.95),
            )
        })
}

pub fn check_lsp_status(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let statuses = core_check_lsp_status();
    Ok((
        json!({ "servers": statuses, "count": statuses.len() }),
        success_meta(BackendKind::Lsp, 1.0),
    ))
}

fn language_name_for_path(file_path: &str) -> &'static str {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "java" => "java",
        "py" => "python",
        _ => "unknown",
    }
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
