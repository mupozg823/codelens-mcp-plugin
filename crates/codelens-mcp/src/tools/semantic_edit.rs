use super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, parse_lsp_args,
    required_string, success_meta,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    SymbolInfo,
    lsp::{LspRenameRequest, LspRequest},
};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticEditBackendSelection {
    TreeSitter,
    Lsp,
}

pub(crate) fn selected_backend(
    arguments: &serde_json::Value,
) -> Result<SemanticEditBackendSelection, CodeLensError> {
    let selected = optional_string(arguments, "semantic_edit_backend")
        .map(ToOwned::to_owned)
        .or_else(|| crate::env_compat::dual_prefix_env("CODELENS_SEMANTIC_EDIT_BACKEND"))
        .unwrap_or_else(|| "tree-sitter".to_owned());

    match selected.as_str() {
        "default" | "off" | "tree-sitter" | "tree_sitter" => {
            Ok(SemanticEditBackendSelection::TreeSitter)
        }
        "lsp" => Ok(SemanticEditBackendSelection::Lsp),
        other => Err(CodeLensError::Validation(format!(
            "unsupported semantic_edit_backend `{other}`; expected tree-sitter or lsp"
        ))),
    }
}

pub(crate) fn rename_symbol_with_lsp_backend(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let symbol_name = arguments
        .get("symbol_name")
        .or_else(|| arguments.get("name"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("symbol_name or name".into()))?
        .to_owned();
    let new_name = required_string(arguments, "new_name")?.to_owned();
    let name_path = arguments.get("name_path").and_then(|value| value.as_str());
    let position_source = position_source(arguments);
    let (line, column) = symbol_position(state, arguments, &file_path, &symbol_name, name_path)?;
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let command_ref = command.clone();
    state
        .lsp_pool()
        .rename_symbol(&LspRenameRequest {
            command,
            args,
            file_path: file_path.clone(),
            line,
            column,
            new_name,
            dry_run,
        })
        .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))
        .map(|result| {
            let success = result.success;
            let message = result.message.clone();
            let modified_files = result.modified_files;
            let total_replacements = result.total_replacements;
            let edits = result.edits.clone();
            (
                json!({
                    "backend": "semantic_edit_backend",
                    "semantic_edit_backend": "lsp",
                    "edit_authority": {
                        "kind": "authoritative_lsp",
                        "operation": "rename",
                        "embedding_used": false,
                        "search_used": false,
                        "position_source": position_source,
                        "validator": "lsp_textDocument_rename",
                    },
                    "position": {"line": line, "column": column},
                    "result": result,
                    "success": success,
                    "message": message,
                    "modified_files": modified_files,
                    "total_replacements": total_replacements,
                    "edits": edits,
                }),
                success_meta(BackendKind::Lsp, 0.96),
            )
        })
}

pub(crate) fn safe_delete_with_lsp_backend(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let symbol_name = required_string(arguments, "symbol_name")?.to_owned();
    let name_path = arguments.get("name_path").and_then(|value| value.as_str());
    let position_source = position_source(arguments);
    let (line, column) = symbol_position(state, arguments, &file_path, &symbol_name, name_path)?;
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let max_results = arguments
        .get("max_results")
        .and_then(|value| value.as_u64())
        .unwrap_or(200) as usize;

    let command_ref = command.clone();
    let references = state
        .lsp_pool()
        .find_referencing_symbols(&LspRequest {
            command,
            args,
            file_path: file_path.clone(),
            line,
            column,
            max_results,
        })
        .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;

    let mut declaration_references = 0usize;
    let mut affected_references = Vec::new();
    for reference in references {
        if reference.file_path == file_path && reference.line == line && reference.column == column
        {
            declaration_references += 1;
            continue;
        }
        affected_references.push(json!({
            "file": reference.file_path,
            "line": reference.line,
            "column": reference.column,
            "end_line": reference.end_line,
            "end_column": reference.end_column,
            "kind": "reference"
        }));
    }

    let total_references = affected_references.len();
    let safe_to_delete = total_references == 0;
    let message = if safe_to_delete {
        format!(
            "LSP found no non-declaration references for `{symbol_name}` in `{file_path}`. Deletion can proceed through the mutation gate."
        )
    } else {
        format!(
            "LSP found {total_references} non-declaration reference(s) for `{symbol_name}` in `{file_path}`. Do not delete until callers are handled."
        )
    };

    Ok((
        json!({
            "success": true,
            "backend": "semantic_edit_backend",
            "semantic_edit_backend": "lsp",
            "edit_authority": {
                "kind": "authoritative_lsp",
                "operation": "safe_delete_check",
                "embedding_used": false,
                "search_used": false,
                "position_source": position_source,
                "validator": "lsp_textDocument_references",
            },
            "symbol_name": symbol_name,
            "file_path": file_path,
            "position": {"line": line, "column": column},
            "safe_to_delete": safe_to_delete,
            "total_references": total_references,
            "declaration_references": declaration_references,
            "affected_references": affected_references,
            "dry_run": dry_run,
            "message": message,
            "safe_delete_action": "check_only",
            "suggested_next_tools": if safe_to_delete {
                json!(["delete_lines", "get_file_diagnostics"])
            } else {
                json!(["find_referencing_symbols", "get_callers", "plan_safe_refactor"])
            }
        }),
        success_meta(BackendKind::Lsp, 0.94),
    ))
}

fn position_source(arguments: &serde_json::Value) -> &'static str {
    match (
        arguments.get("line").and_then(|value| value.as_u64()),
        arguments.get("column").and_then(|value| value.as_u64()),
    ) {
        (Some(_), Some(_)) => "explicit",
        _ => "symbol_index",
    }
}

fn symbol_position(
    state: &AppState,
    arguments: &serde_json::Value,
    file_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
) -> Result<(usize, usize), CodeLensError> {
    match (
        arguments.get("line").and_then(|value| value.as_u64()),
        arguments.get("column").and_then(|value| value.as_u64()),
    ) {
        (Some(line), Some(column)) => return Ok((line as usize, column as usize)),
        (None, None) => {}
        _ => {
            return Err(CodeLensError::MissingParam(
                "line and column must be provided together".into(),
            ));
        }
    }

    let symbols = codelens_engine::get_symbols_overview(&state.project(), file_path, 0)
        .map_err(CodeLensError::Internal)?;
    let flat = flatten_symbols(symbols);
    flat.iter()
        .find(|symbol| {
            if let Some(name_path) = name_path {
                symbol.name_path == name_path
            } else {
                symbol.name == symbol_name
            }
        })
        .map(|symbol| (symbol.line, symbol.column))
        .ok_or_else(|| {
            CodeLensError::NotFound(format!(
                "symbol `{symbol_name}` not found in {file_path}; provide line and column for LSP rename"
            ))
        })
}

fn flatten_symbols(symbols: Vec<SymbolInfo>) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    for mut symbol in symbols {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        flat.extend(flatten_symbols(children));
    }
    flat
}
