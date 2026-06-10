//! D1 (#346 Phase 4): LSP read navigation — `find_declaration` /
//! `find_implementations`.
//!
//! Thin wrappers over the engine's `resolve_symbol_target` path with the
//! LSP method preset. Unlike `resolve_symbol_target` (which demands an
//! explicit position and errors when the server is missing), these
//! resolve the position from the symbol index by name and degrade to a
//! successful empty result with `degraded_reason` + `fallback_hint`
//! when no LSP server is reachable — read-surface agents should never
//! hard-fail just because a language server is absent.

use super::super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, optional_usize,
    parse_lsp_args, success_meta,
};
use super::rename::resolve_symbol_position;
use super::shared::{insert_response_annotations, language_name_for_path, resolve_path_argument};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::degraded_meta;
use codelens_engine::LspResolveTargetRequest;
use serde_json::json;

/// Index-backed tools an agent should fall back to when LSP is absent.
pub(super) const LSP_READ_FALLBACK_HINTS: [&str; 2] = ["find_symbol", "bm25_symbol_search"];

const KNOWN_ARGS: &[&str] = &[
    "path",
    "file_path",
    "relative_path",
    "symbol_name",
    "line",
    "column",
    "command",
    "args",
    "max_results",
];

pub fn find_declaration(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    run_lsp_navigation(state, arguments, "declaration")
}

pub fn find_implementations(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    run_lsp_navigation(state, arguments, "implementation")
}

fn run_lsp_navigation(
    state: &AppState,
    arguments: &serde_json::Value,
    target: &'static str,
) -> ToolResult {
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let file_path = file_path_arg.to_owned();
    let symbol_name = optional_string(arguments, "symbol_name").map(ToOwned::to_owned);

    // Position: explicit line/column wins; otherwise resolve the named
    // symbol through the index (same path find_referencing_symbols uses).
    let explicit_line = arguments.get("line").and_then(|v| v.as_u64());
    let explicit_column = arguments.get("column").and_then(|v| v.as_u64());
    let (line, column) = match (explicit_line, explicit_column) {
        (Some(line), Some(column)) => (line as usize, column as usize),
        _ => {
            let Some(name) = symbol_name.as_deref() else {
                return Err(CodeLensError::MissingParam(
                    "symbol_name (or line + column)".to_owned(),
                ));
            };
            resolve_symbol_position(state, name, &file_path).ok_or_else(|| {
                CodeLensError::Validation(format!(
                    "symbol '{name}' not found in {file_path} — run refresh_symbol_index if it was just added"
                ))
            })?
        }
    };

    let Some(command) = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
    else {
        return degraded_navigation(
            target,
            &file_path,
            symbol_name.as_deref(),
            "no default LSP server mapping for this file type",
            &unknown_args,
            &deprecation_warnings,
        );
    };
    let args = parse_lsp_args(arguments, &command);
    let max_results = optional_usize(arguments, "max_results", 20);

    match state
        .lsp_pool()
        .resolve_symbol_target(&LspResolveTargetRequest {
            command,
            args,
            file_path: file_path.clone(),
            line,
            column,
            target: target.to_owned(),
            max_results,
        }) {
        Ok(targets) => {
            let mut payload = json!({
                "success": true,
                "operation": target,
                "backend": "lsp",
                "symbol_name": symbol_name,
                "language": language_name_for_path(&file_path),
                "position": {"file_path": file_path, "line": line, "column": column},
                "targets": targets,
                "count": targets.len(),
            });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            Ok((payload, success_meta(BackendKind::Lsp, 0.95)))
        }
        Err(error) => degraded_navigation(
            target,
            &file_path,
            symbol_name.as_deref(),
            &error.to_string(),
            &unknown_args,
            &deprecation_warnings,
        ),
    }
}

/// LSP-absent contract: success with empty targets, a reason, and the
/// index-backed fallback chain — never an error.
fn degraded_navigation(
    target: &str,
    file_path: &str,
    symbol_name: Option<&str>,
    reason: &str,
    unknown_args: &[String],
    deprecation_warnings: &[serde_json::Value],
) -> ToolResult {
    let reason = format!("LSP unavailable for {target}: {reason}");
    let mut payload = json!({
        "success": true,
        "operation": target,
        "backend": "lsp",
        "symbol_name": symbol_name,
        "language": language_name_for_path(file_path),
        "targets": [],
        "count": 0,
        "degraded_reason": reason,
        "fallback_hint": LSP_READ_FALLBACK_HINTS,
    });
    insert_response_annotations(&mut payload, unknown_args, deprecation_warnings);
    Ok((payload, degraded_meta(BackendKind::Lsp, 0.3, &reason)))
}
