use crate::error::CodeLensError;
use crate::tool_runtime::optional_string;
use crate::tools::{default_lsp_command_for_path, parse_lsp_args};

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
        "auto" => {
            let file_path = optional_string(arguments, "file_path");
            match file_path.and_then(default_lsp_command_for_path) {
                Some(_) => Ok(SemanticEditBackendSelection::Lsp),
                None => Ok(SemanticEditBackendSelection::TreeSitter),
            }
        }
        other => Err(CodeLensError::Validation(format!(
            "unsupported semantic_edit_backend `{other}`; expected tree-sitter, lsp, or auto"
        ))),
    }
}

pub(super) fn lsp_command_and_args(
    arguments: &serde_json::Value,
    file_path: &str,
) -> Result<(String, Vec<String>), CodeLensError> {
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    Ok((command, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_is_tree_sitter() {
        let result = selected_backend(&json!({})).expect("default succeeds");
        assert_eq!(result, SemanticEditBackendSelection::TreeSitter);
    }

    #[test]
    fn explicit_tree_sitter_aliases_resolve() {
        for alias in ["tree-sitter", "tree_sitter", "default", "off"] {
            let result =
                selected_backend(&json!({"semantic_edit_backend": alias})).expect("alias resolves");
            assert_eq!(
                result,
                SemanticEditBackendSelection::TreeSitter,
                "alias `{alias}` should resolve to TreeSitter"
            );
        }
    }

    #[test]
    fn explicit_lsp_resolves() {
        let result =
            selected_backend(&json!({"semantic_edit_backend": "lsp"})).expect("lsp resolves");
        assert_eq!(result, SemanticEditBackendSelection::Lsp);
    }

    #[test]
    fn auto_with_lsp_capable_extension_picks_lsp() {
        let result = selected_backend(&json!({
            "semantic_edit_backend": "auto",
            "file_path": "src/lib.rs",
        }))
        .expect("auto+rs resolves");
        assert_eq!(result, SemanticEditBackendSelection::Lsp);
    }

    #[test]
    fn auto_with_uncapable_extension_falls_back_to_tree_sitter() {
        let result = selected_backend(&json!({
            "semantic_edit_backend": "auto",
            "file_path": "data/manifest.unknownext",
        }))
        .expect("auto+unknown resolves");
        assert_eq!(result, SemanticEditBackendSelection::TreeSitter);
    }

    #[test]
    fn auto_without_file_path_falls_back_to_tree_sitter() {
        let result = selected_backend(&json!({"semantic_edit_backend": "auto"}))
            .expect("auto+nofile resolves");
        assert_eq!(result, SemanticEditBackendSelection::TreeSitter);
    }

    #[test]
    fn unsupported_backend_value_errors_with_hint_listing_auto() {
        let result = selected_backend(&json!({"semantic_edit_backend": "garbage"}));
        let err = result.expect_err("garbage should error");
        let msg = err.to_string();
        assert!(
            msg.contains("auto"),
            "error message must list `auto` as a valid choice (got: {msg})",
        );
    }
}
