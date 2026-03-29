use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LspRecipe {
    pub language: &'static str,
    pub extensions: &'static [&'static str],
    pub server_name: &'static str,
    pub install_command: &'static str,
    pub binary_name: &'static str,
    pub args: &'static [&'static str],
    pub package_manager: &'static str,
}

pub const LSP_RECIPES: &[LspRecipe] = &[
    LspRecipe {
        language: "python",
        extensions: &["py"],
        server_name: "pyright",
        install_command: "npm install -g pyright",
        binary_name: "pyright-langserver",
        args: &["--stdio"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "typescript",
        extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
        server_name: "typescript-language-server",
        install_command: "npm install -g typescript-language-server typescript",
        binary_name: "typescript-language-server",
        args: &["--stdio"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "rust",
        extensions: &["rs"],
        server_name: "rust-analyzer",
        install_command: "rustup component add rust-analyzer",
        binary_name: "rust-analyzer",
        args: &[],
        package_manager: "rustup",
    },
    LspRecipe {
        language: "go",
        extensions: &["go"],
        server_name: "gopls",
        install_command: "go install golang.org/x/tools/gopls@latest",
        binary_name: "gopls",
        args: &["serve"],
        package_manager: "go",
    },
    LspRecipe {
        language: "java",
        extensions: &["java"],
        server_name: "jdtls",
        install_command: "brew install jdtls",
        binary_name: "jdtls",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "kotlin",
        extensions: &["kt", "kts"],
        server_name: "kotlin-language-server",
        install_command: "brew install kotlin-language-server",
        binary_name: "kotlin-language-server",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "c_cpp",
        extensions: &["c", "h", "cpp", "cc", "cxx", "hpp"],
        server_name: "clangd",
        install_command: "brew install llvm",
        binary_name: "clangd",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "ruby",
        extensions: &["rb"],
        server_name: "solargraph",
        install_command: "gem install solargraph",
        binary_name: "solargraph",
        args: &["stdio"],
        package_manager: "gem",
    },
    LspRecipe {
        language: "php",
        extensions: &["php"],
        server_name: "intelephense",
        install_command: "npm install -g intelephense",
        binary_name: "intelephense",
        args: &["--stdio"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "swift",
        extensions: &["swift"],
        server_name: "sourcekit-lsp",
        install_command: "xcode-select --install",
        binary_name: "sourcekit-lsp",
        args: &[],
        package_manager: "xcode",
    },
    LspRecipe {
        language: "csharp",
        extensions: &["cs"],
        server_name: "omnisharp",
        install_command: "dotnet tool install -g csharp-ls",
        binary_name: "csharp-ls",
        args: &[],
        package_manager: "dotnet",
    },
    LspRecipe {
        language: "dart",
        extensions: &["dart"],
        server_name: "dart-language-server",
        install_command: "dart pub global activate dart_language_server",
        binary_name: "dart",
        args: &["language-server", "--protocol=lsp"],
        package_manager: "dart",
    },
];

/// Check which LSP servers are installed and which are missing.
pub fn check_lsp_status() -> Vec<LspStatus> {
    LSP_RECIPES
        .iter()
        .map(|recipe| {
            let installed = std::process::Command::new("which")
                .arg(recipe.binary_name)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            LspStatus {
                language: recipe.language,
                server_name: recipe.server_name,
                installed,
                install_command: recipe.install_command,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct LspStatus {
    pub language: &'static str,
    pub server_name: &'static str,
    pub installed: bool,
    pub install_command: &'static str,
}

/// Get the recipe for a file extension.
pub fn get_lsp_recipe(extension: &str) -> Option<&'static LspRecipe> {
    let ext = extension.to_ascii_lowercase();
    LSP_RECIPES
        .iter()
        .find(|r| r.extensions.contains(&ext.as_str()))
}
