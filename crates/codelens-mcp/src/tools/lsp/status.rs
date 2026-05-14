use super::super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::get_lsp_recipe as core_get_lsp_recipe;
use serde_json::json;

pub fn get_lsp_recipe(_state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let extension = required_string(arguments, "extension")?;
    match core_get_lsp_recipe(extension) {
        Some(recipe) => Ok((json!(recipe), success_meta(BackendKind::Lsp, 1.0))),
        None => Err(CodeLensError::NotFound(format!(
            "LSP recipe for extension: {extension}"
        ))),
    }
}
