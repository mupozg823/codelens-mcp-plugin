use super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, parse_lsp_args,
    required_string, success_meta,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    SymbolInfo,
    lsp::{LspCodeActionRequest, LspRenameRequest, LspRequest},
};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticEditBackendSelection {
    TreeSitter,
    Lsp,
    JetBrains,
    Roslyn,
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
        "jetbrains" => Ok(SemanticEditBackendSelection::JetBrains),
        "roslyn" => Ok(SemanticEditBackendSelection::Roslyn),
        other => Err(CodeLensError::Validation(format!(
            "unsupported semantic_edit_backend `{other}`; expected tree-sitter, lsp, jetbrains, or roslyn"
        ))),
    }
}

pub(crate) fn unsupported_external_adapter(
    backend: SemanticEditBackendSelection,
    operation: &str,
) -> ToolResult {
    let backend_name = match backend {
        SemanticEditBackendSelection::JetBrains => "jetbrains",
        SemanticEditBackendSelection::Roslyn => "roslyn",
        _ => "unknown",
    };
    Err(CodeLensError::Validation(format!(
        "unsupported_semantic_refactor: semantic_edit_backend={backend_name} for `{operation}` is an opt-in adapter boundary, but no local adapter process/protocol is configured in this release"
    )))
}

pub(crate) fn code_action_refactor_with_lsp_backend(
    state: &AppState,
    arguments: &serde_json::Value,
    operation: &'static str,
    default_kinds: &[&str],
) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let (start_line, start_column, end_line, end_column, position_source) =
        code_action_range(state, arguments, &file_path, operation)?;
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let only = code_action_kinds(arguments, default_kinds);
    let action_id = optional_string(arguments, "action_id").map(ToOwned::to_owned);

    let command_ref = command.clone();
    let result = state
        .lsp_pool()
        .code_action_refactor(&LspCodeActionRequest {
            command,
            args,
            file_path: file_path.clone(),
            start_line,
            start_column,
            end_line,
            end_column,
            only: only.clone(),
            action_id,
            operation: operation.to_owned(),
            dry_run,
        })
        .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;

    let transaction = serde_json::to_value(&result.transaction)
        .unwrap_or_else(|_| json!({"serialization_error": true}));
    Ok((
        json!({
            "success": result.success,
            "backend": "semantic_edit_backend",
            "semantic_edit_backend": "lsp",
            "operation": operation,
            "file_path": file_path,
            "range": {
                "start_line": start_line,
                "start_column": start_column,
                "end_line": end_line,
                "end_column": end_column,
                "position_source": position_source,
            },
            "action": {
                "title": result.action_title,
                "kind": result.action_kind,
                "resolved_via": result.resolved_via,
                "requested_kinds": only,
            },
            "edit_authority": {
                "kind": "authoritative_lsp",
                "backend": "lsp",
                "operation": operation,
                "language": language_for_file(&file_path),
                "methods": ["textDocument/codeAction", "codeAction/resolve"],
                "embedding_used": false,
                "search_used": false
            },
            "transaction": {
                "dry_run": dry_run,
                "modified_files": result.transaction.modified_files,
                "edit_count": result.transaction.edit_count,
                "resource_ops": result.transaction.resource_ops,
                "rollback_available": result.transaction.rollback_available
            },
            "workspace_edit": transaction,
            "verification": {
                "pre_diagnostics": [],
                "post_diagnostics": [],
                "references_checked": false,
                "conflicts": []
            },
            "applied": result.applied,
            "message": result.message,
        }),
        success_meta(BackendKind::Lsp, 0.93),
    ))
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
                    "transaction": {
                        "dry_run": dry_run,
                        "modified_files": modified_files,
                        "edit_count": total_replacements,
                        "resource_ops": [],
                        "rollback_available": true
                    },
                    "verification": {
                        "pre_diagnostics": [],
                        "post_diagnostics": [],
                        "references_checked": false,
                        "conflicts": []
                    },
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
    let mut safe_delete_action = "check_only";
    let mut modified_files = 0usize;
    let mut edit_count = 0usize;
    if !dry_run {
        if !safe_to_delete {
            return Err(CodeLensError::Validation(format!(
                "safe_delete_apply blocked: `{symbol_name}` still has {total_references} non-declaration reference(s)"
            )));
        }
        let (start_byte, end_byte) = codelens_engine::find_symbol_range(
            &state.project(),
            &file_path,
            &symbol_name,
            name_path,
        )
        .map_err(|error| {
            CodeLensError::Validation(format!(
                "safe_delete_apply blocked: tree-sitter could not isolate declaration range: {error}"
            ))
        })?;
        let resolved = state.project().resolve(&file_path)?;
        let mut source = std::fs::read_to_string(&resolved)?;
        if start_byte >= end_byte
            || end_byte > source.len()
            || !source.is_char_boundary(start_byte)
            || !source.is_char_boundary(end_byte)
        {
            return Err(CodeLensError::Validation(
                "safe_delete_apply blocked: invalid declaration byte range".into(),
            ));
        }
        let mut delete_end = end_byte;
        if source.as_bytes().get(delete_end) == Some(&b'\n') {
            delete_end += 1;
        }
        source.replace_range(start_byte..delete_end, "");
        std::fs::write(&resolved, source)?;
        safe_delete_action = "applied";
        modified_files = 1;
        edit_count = 1;
    }
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
            "affected_references": affected_references.clone(),
            "dry_run": dry_run,
            "message": message,
            "safe_delete_action": safe_delete_action,
            "transaction": {
                "dry_run": dry_run,
                "modified_files": modified_files,
                "edit_count": edit_count,
                "resource_ops": [],
                "rollback_available": false
            },
            "verification": {
                "pre_diagnostics": [],
                "post_diagnostics": [],
                "references_checked": true,
                "conflicts": if safe_to_delete {
                    json!([])
                } else {
                    serde_json::Value::Array(affected_references.clone())
                }
            },
            "suggested_next_tools": if safe_to_delete {
                json!(["delete_lines", "get_file_diagnostics"])
            } else {
                json!(["find_referencing_symbols", "get_callers", "plan_safe_refactor"])
            }
        }),
        success_meta(BackendKind::Lsp, 0.94),
    ))
}

fn code_action_range(
    state: &AppState,
    arguments: &serde_json::Value,
    file_path: &str,
    operation: &str,
) -> Result<(usize, usize, usize, usize, &'static str), CodeLensError> {
    if let Some(start_line) = arguments.get("start_line").and_then(|value| value.as_u64()) {
        let end_line = arguments
            .get("end_line")
            .and_then(|value| value.as_u64())
            .unwrap_or(start_line) as usize;
        let start_line = start_line as usize;
        let start_column = arguments
            .get("start_column")
            .or_else(|| arguments.get("column"))
            .and_then(|value| value.as_u64())
            .unwrap_or(1) as usize;
        let end_column = arguments
            .get("end_column")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| default_end_column(state, file_path, end_line));
        return Ok((
            start_line,
            start_column,
            end_line,
            end_column,
            "explicit_range",
        ));
    }

    let (symbol_name, name_path) = symbol_for_operation(arguments, operation)?;
    let (line, column) = symbol_position(state, arguments, file_path, &symbol_name, name_path)?;
    Ok((line, column, line, column, position_source(arguments)))
}

fn symbol_for_operation<'a>(
    arguments: &'a serde_json::Value,
    operation: &str,
) -> Result<(String, Option<&'a str>), CodeLensError> {
    let name_path = arguments.get("name_path").and_then(|value| value.as_str());
    let key = match operation {
        "move_symbol" => "symbol_name",
        "inline_function" | "change_signature" => "function_name",
        _ => "symbol_name",
    };
    let symbol_name = arguments
        .get(key)
        .or_else(|| arguments.get("symbol_name"))
        .or_else(|| arguments.get("function_name"))
        .or_else(|| arguments.get("name"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| CodeLensError::MissingParam(key.into()))?
        .to_owned();
    Ok((symbol_name, name_path))
}

fn default_end_column(state: &AppState, file_path: &str, line: usize) -> usize {
    state
        .project()
        .resolve(file_path)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|source| {
            source
                .lines()
                .nth(line.saturating_sub(1))
                .map(|text| text.len() + 1)
        })
        .unwrap_or(1)
}

fn code_action_kinds(arguments: &serde_json::Value, default_kinds: &[&str]) -> Vec<String> {
    if let Some(kind) = optional_string(arguments, "code_action_kind") {
        return vec![kind.to_owned()];
    }
    if let Some(items) = arguments
        .get("code_action_kinds")
        .and_then(|value| value.as_array())
    {
        let parsed = items
            .iter()
            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    default_kinds
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect()
}

fn language_for_file(file_path: &str) -> &'static str {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "java" => "java",
        _ => "unknown",
    }
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
