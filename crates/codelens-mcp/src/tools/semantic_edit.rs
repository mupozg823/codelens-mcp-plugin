use super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, parse_lsp_args,
    required_string, success_meta,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{SymbolInfo, lsp::LspRenameRequest};
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
    let (line, column) = rename_position(state, arguments, &file_path, &symbol_name, name_path)?;
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

fn rename_position(
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
