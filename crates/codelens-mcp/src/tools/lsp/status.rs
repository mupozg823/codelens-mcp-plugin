use super::super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    check_lsp_status as core_check_lsp_status, get_lsp_recipe as core_get_lsp_recipe,
};
use serde_json::json;

#[allow(dead_code)]
pub fn check_lsp_status(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let statuses = core_check_lsp_status();
    Ok((
        json!({ "servers": statuses, "count": statuses.len() }),
        success_meta(BackendKind::Lsp, 1.0),
    ))
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
