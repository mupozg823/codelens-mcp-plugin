#[path = "lsp/boost.rs"]
mod boost;
#[path = "lsp/diagnostics.rs"]
mod diagnostics;
#[path = "lsp/references.rs"]
mod references;
#[path = "lsp/status.rs"]
mod status;
#[path = "lsp/text_refs.rs"]
mod text_refs;
#[path = "lsp/workspace.rs"]
mod workspace;

use crate::error::CodeLensError;

pub(crate) use boost::lsp_boost_probe;
pub use diagnostics::get_file_diagnostics;
pub use references::find_referencing_symbols;
pub use status::{check_lsp_status, get_lsp_readiness, get_lsp_recipe};
pub use workspace::{get_type_hierarchy, plan_symbol_rename, search_workspace_symbols};

pub(super) fn lsp_install_hint(command: &str) -> &'static str {
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
    if msg.contains("No such file") || msg.contains("not found") || msg.contains("spawn") {
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
