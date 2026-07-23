use super::super::optional_string;
use crate::error::CodeLensError;
use serde_json::{Value, json};

pub(super) fn resolve_path_argument(
    arguments: &serde_json::Value,
) -> Result<(&str, Vec<Value>), CodeLensError> {
    if let Some(path) = optional_string(arguments, "path") {
        if let Some(alias @ ("file_path" | "relative_path")) =
            optional_string(arguments, "_path_alias_source")
        {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
        return Ok((path, Vec::new()));
    }
    for alias in ["file_path", "relative_path"] {
        if let Some(path) = optional_string(arguments, alias) {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
    }
    Err(CodeLensError::MissingParam("path".to_owned()))
}

pub(super) fn insert_response_annotations(
    payload: &mut Value,
    unknown_args: &[String],
    deprecation_warnings: &[Value],
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    if !unknown_args.is_empty() {
        map.insert("unknown_args".to_owned(), json!(unknown_args));
    }
    if !deprecation_warnings.is_empty() {
        map.insert(
            "deprecation_warnings".to_owned(),
            json!(deprecation_warnings),
        );
    }
}

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
        "dart" => "  dart pub global activate dart_language_server",
        // Phase 6a languages
        "lua-language-server" => "  brew install lua-language-server",
        "zls" => "  brew install zls",
        "nextls" => "  mix escript.install hex next_ls",
        "haskell-language-server-wrapper" => "  ghcup install hls",
        "ocamllsp" => "  opam install ocaml-lsp-server",
        "erlang_ls" => "  brew install erlang_ls",
        "bash-language-server" => "  npm i -g bash-language-server",
        _ => "  Check your package manager for the LSP server binary",
    }
}

pub(super) fn enhance_lsp_error(err: anyhow::Error, command: &str) -> CodeLensError {
    let msg = err.to_string();
    if msg.contains("No such file")
        || msg.contains("not found")
        || msg.contains("spawn")
        || msg.contains("no trusted executable configured")
    {
        CodeLensError::LspNotAttached(format!(
            "LSP server '{command}' not found. Install it:\n{}",
            lsp_install_hint(command)
        ))
    } else if msg.contains("timed out") || msg.contains("timeout") {
        CodeLensError::Timeout {
            operation: format!("LSP {command}"),
            elapsed_ms: 30_000,
        }
    } else {
        CodeLensError::LspError(msg)
    }
}

/// #180: append a single deprecation_warnings entry to a payload when the
/// caller used a legacy path alias. Mirrors the per-file helper in
/// filesystem.rs so all path-aliased tools emit the same shape.
pub(super) fn attach_alias_warning(mut payload: Value, warning: Option<Value>) -> Value {
    if let Some(warning) = warning
        && let Some(map) = payload.as_object_mut()
    {
        map.insert(
            "deprecation_warnings".to_owned(),
            Value::Array(vec![warning]),
        );
    }
    payload
}

pub(super) fn language_name_for_path(file_path: &str) -> &'static str {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "java" => "java",
        "py" => "python",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::enhance_lsp_error;
    use crate::error::RecoveryHint;

    #[test]
    fn missing_trusted_executable_uses_symbol_fallback_when_lsp_cannot_attach() {
        // Given: the engine rejects a recipe whose executable is unavailable in the daemon.
        let engine_error = anyhow::anyhow!(
            "Blocked: 'pyright' has no trusted executable configured in the daemon environment"
        );

        // When: the MCP boundary classifies the engine error.
        let recovery_hint = enhance_lsp_error(engine_error, "pyright").recovery_hint();

        // Then: clients receive the structured non-LSP fallback.
        assert!(matches!(
            recovery_hint,
            Some(RecoveryHint::FallbackTool { tool, .. }) if tool == "find_symbol"
        ));
    }
}
