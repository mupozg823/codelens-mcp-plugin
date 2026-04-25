/// Known-safe LSP server binaries. Commands not in this list are rejected.
pub(super) fn is_allowed_lsp_command(command: &str) -> bool {
    let binary = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);

    ALLOWED_COMMANDS.contains(&binary)
}

pub(super) const ALLOWED_COMMANDS: &[&str] = &[
    "pyright-langserver",
    "typescript-language-server",
    "rust-analyzer",
    "gopls",
    "jdtls",
    "kotlin-language-server",
    "clangd",
    "solargraph",
    "intelephense",
    "sourcekit-lsp",
    "csharp-ls",
    "dart",
    "metals",
    "lua-language-server",
    "terraform-ls",
    "yaml-language-server",
    "python3",
    "python",
];
