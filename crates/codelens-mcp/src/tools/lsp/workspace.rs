use super::enhance_lsp_error;
use crate::AppState;
use crate::authority::{meta_degraded, meta_for_backend};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tools::{
    ToolResult, default_lsp_command_for_path, optional_string, optional_usize, parse_lsp_args,
    required_string, success_meta,
};
use codelens_engine::{
    LspRenamePlanRequest, LspTypeHierarchyRequest, LspWorkspaceSymbolRequest,
    get_type_hierarchy_native,
};
use serde_json::json;

pub fn search_workspace_symbols(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?.to_owned();
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
