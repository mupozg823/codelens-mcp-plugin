use super::enhance_lsp_error;
use super::text_refs::finalize_text_refs_response;
use crate::AppState;
use crate::authority::{meta_degraded, meta_for_backend};
use crate::error::CodeLensError;
use crate::limits::LimitsApplied;
use crate::tools::{
    ToolResult, default_lsp_command_for_path, optional_bool, optional_string, optional_usize,
    parse_lsp_args, required_string,
};
use codelens_engine::{LspRequest, extract_word_at_position, find_referencing_symbols_via_text};
use serde_json::json;

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

    if !use_lsp && !has_position {
        let sym_name = symbol_name_param.ok_or_else(|| {
            CodeLensError::MissingParam("symbol_name (or line+column with use_lsp=true)".into())
        })?;

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

        #[cfg(feature = "scip-backend")]
        if let Some(backend) = state.scip() {
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

        if let Ok(lsp_refs) = lsp_result {
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
            let sym_name_owned = symbol_name_param.map(ToOwned::to_owned).or_else(|| {
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
    }

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
