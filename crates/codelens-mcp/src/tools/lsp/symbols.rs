use super::super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, optional_usize,
    parse_lsp_args, required_string, success_meta,
};
use super::shared::{attach_alias_warning, enhance_lsp_error};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_evidence::{meta_degraded, meta_for_backend};
use codelens_engine::{
    LspTypeHierarchyRequest, LspWorkspaceSymbolRequest, get_type_hierarchy_native,
};
use serde_json::json;

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
        .map(crate::tool_runtime::path_alias_warning);
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
