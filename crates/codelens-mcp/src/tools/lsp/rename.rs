use super::super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, optional_usize,
    parse_lsp_args, success_meta,
};
use super::shared::{
    enhance_lsp_error, insert_response_annotations, language_name_for_path, resolve_path_argument,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{LspRenamePlanRequest, LspResolveTargetRequest};
use serde_json::json;

/// Resolve a symbol name to its (line, column) position in a file via the symbol index.
pub(super) fn resolve_symbol_position(
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

pub fn plan_symbol_rename(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    const RENAME_PLAN_KNOWN_ARGS: &[&str] = &[
        "path",
        "file_path",
        "line",
        "column",
        "new_name",
        "command",
        "args",
    ];
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, RENAME_PLAN_KNOWN_ARGS);
    let file_path = file_path_arg.to_owned();
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
        .map(|value| {
            let mut payload = json!(value);
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, success_meta(BackendKind::Lsp, 0.86))
        })
}

pub fn resolve_symbol_target(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    const SYMBOL_TARGET_KNOWN_ARGS: &[&str] = &[
        "path",
        "file_path",
        "line",
        "column",
        "target",
        "semantic_backend",
        "command",
        "args",
        "max_results",
    ];
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args =
        crate::tool_runtime::collect_unknown_args(arguments, SYMBOL_TARGET_KNOWN_ARGS);
    let file_path = file_path_arg.to_owned();
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
            let mut payload = json!({
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
            });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, success_meta(BackendKind::Lsp, 0.95))
        })
}
