use super::{
    default_lsp_command_for_path, parse_lsp_args, required_string, success_meta, AppState,
    ToolResult,
};
use crate::error::CodeLensError;
use codelens_core::{
    check_lsp_status as core_check_lsp_status, extract_word_at_position,
    find_referencing_symbols_via_text, get_lsp_recipe as core_get_lsp_recipe,
    get_type_hierarchy_native, LspDiagnosticRequest, LspRenamePlanRequest, LspRequest,
    LspTypeHierarchyRequest, LspWorkspaceSymbolRequest,
};
use serde_json::json;

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
        _ => "  Check your package manager for the LSP server binary",
    }
}

fn enhance_lsp_error(err: anyhow::Error, command: &str) -> anyhow::Error {
    let msg = err.to_string();
    if msg.contains("No such file") || msg.contains("not found") || msg.contains("spawn") {
        anyhow::anyhow!(
            "{msg}\n\nHint: LSP server '{command}' not found. Install it:\n{}",
            lsp_install_hint(command)
        )
    } else if msg.contains("timed out") || msg.contains("timeout") {
        anyhow::anyhow!(
            "{msg}\n\nHint: LSP server '{command}' timed out. It may still be initializing on first run. Try again."
        )
    } else {
        err
    }
}

pub fn find_referencing_symbols(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let symbol_name_param = arguments.get("symbol_name").and_then(|v| v.as_str());
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    if let Some(sym_name) = symbol_name_param {
        return Ok(find_referencing_symbols_via_text(
            &state.project,
            sym_name,
            Some(&file_path),
            max_results,
        )
        .map(|value| {
            (
                json!({ "references": value, "count": value.len() }),
                success_meta("text_search", 0.80),
            )
        })?);
    }

    let line = arguments
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("line or symbol_name".into()))?
        as usize;
    let column = arguments
        .get("column")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("column or symbol_name".into()))?
        as usize;
    let command = arguments
        .get("command")
        .and_then(|v| v.as_str())
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
            Ok(value) => Ok((
                json!({ "references": value, "count": value.len() }),
                success_meta("lsp_pooled", 0.9),
            )),
            Err(_) => {
                let word = extract_word_at_position(&state.project, &file_path, line, column)?;
                Ok(find_referencing_symbols_via_text(
                    &state.project,
                    &word,
                    Some(&file_path),
                    max_results,
                )
                .map(|value| {
                    (
                        json!({ "references": value, "count": value.len() }),
                        success_meta("text_fallback", 0.75),
                    )
                })?)
            }
        }
    } else {
        let word = extract_word_at_position(&state.project, &file_path, line, column)?;
        Ok(
            find_referencing_symbols_via_text(&state.project, &word, Some(&file_path), max_results)
                .map(|value| {
                    (
                        json!({ "references": value, "count": value.len() }),
                        success_meta("text_fallback", 0.75),
                    )
                })?,
        )
    }
}

pub fn get_file_diagnostics(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let command = arguments
        .get("command")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(200) as usize;

    let command_ref = command.clone();
    Ok(state
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
                success_meta("lsp_pooled", 0.9),
            )
        })?)
}

pub fn search_workspace_symbols(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?.to_owned();
    let command = required_string(arguments, "command")?.to_owned();
    let args = parse_lsp_args(arguments, &command);
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let command_ref = command.clone();
    Ok(state
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
                success_meta("lsp_pooled", 0.88),
            )
        })?)
}

pub fn get_type_hierarchy(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = arguments
        .get("name_path")
        .or_else(|| arguments.get("fully_qualified_name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("name_path or fully_qualified_name".into()))?
        .to_owned();
    let relative_path = arguments
        .get("relative_path")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);
    let hierarchy_type = arguments
        .get("hierarchy_type")
        .and_then(|v| v.as_str())
        .unwrap_or("both")
        .to_owned();
    let depth = arguments.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let command = arguments
        .get("command")
        .and_then(|v| v.as_str())
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
            Ok(value) => Ok((json!(value), success_meta("lsp_pooled", 0.82))),
            Err(_) => Ok(get_type_hierarchy_native(
                &state.project,
                &query,
                relative_path.as_deref(),
                &hierarchy_type,
                depth,
            )
            .map(|value| (json!(value), success_meta("tree-sitter-native", 0.80)))?),
        }
    } else {
        Ok(get_type_hierarchy_native(
            &state.project,
            &query,
            relative_path.as_deref(),
            &hierarchy_type,
            depth,
        )
        .map(|value| (json!(value), success_meta("tree-sitter-native", 0.80)))?)
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
    let new_name = arguments
        .get("new_name")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);
    let command = arguments
        .get("command")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);

    let command_ref = command.clone();
    Ok(state
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
        .map(|value| (json!(value), success_meta("lsp_pooled", 0.86)))?)
}

pub fn check_lsp_status(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let statuses = core_check_lsp_status();
    Ok((
        json!({ "servers": statuses, "count": statuses.len() }),
        success_meta("lsp", 1.0),
    ))
}

pub fn get_lsp_recipe(_state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let extension = required_string(arguments, "extension")?;
    match core_get_lsp_recipe(extension) {
        Some(recipe) => Ok((json!(recipe), success_meta("lsp", 1.0))),
        None => Err(CodeLensError::NotFound(format!(
            "LSP recipe for extension: {extension}"
        ))),
    }
}
