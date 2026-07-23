use super::super::{AppState, ToolResult, optional_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::get_lsp_recipe as core_get_lsp_recipe;
use serde_json::json;
use std::path::Path;

fn extension_from_path(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
}

pub fn get_lsp_recipe(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = optional_string(arguments, "path")
        .or_else(|| optional_string(arguments, "file_path"))
        .or_else(|| optional_string(arguments, "relative_path"));
    let extension = optional_string(arguments, "extension")
        .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
        .or_else(|| path.and_then(extension_from_path))
        .ok_or_else(|| CodeLensError::MissingParam("extension or path".to_owned()))?;
    match core_get_lsp_recipe(&extension) {
        Some(recipe) => {
            let resolved = state.lsp_pool().trusted_lsp_binary(recipe.binary_name);
            Ok((
                json!({
                    "extension": extension,
                    "path": path,
                    "language": recipe.language,
                    "server_name": recipe.server_name,
                    "binary_name": recipe.binary_name,
                    "args": recipe.args,
                    "installed": resolved.is_some(),
                    "trusted_launchable": resolved.is_some(),
                    "resolved_binary_path": resolved.as_ref().map(|path| path.display().to_string()),
                    "resolution_hint_dir": serde_json::Value::Null,
                    "execution_trust": "daemon_environment_or_host_registration",
                    "install_command": recipe.install_command,
                    "package_manager": recipe.package_manager,
                    "extensions": recipe.extensions,
                }),
                success_meta(BackendKind::Lsp, 1.0),
            ))
        }
        None => Err(CodeLensError::NotFound(format!(
            "LSP recipe for extension: {extension}"
        ))),
    }
}
