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
    // Phase 6a: new languages
    LspRecipe {
        language: "lua",
        extensions: &["lua"],
        server_name: "lua-language-server",
        install_command: "brew install lua-language-server",
        binary_name: "lua-language-server",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "zig",
        extensions: &["zig"],
        server_name: "zls",
        install_command: "brew install zls",
        binary_name: "zls",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "elixir",
        extensions: &["ex", "exs"],
        server_name: "next-ls",
        install_command: "mix escript.install hex next_ls",
        binary_name: "nextls",
        args: &["--stdio"],
        package_manager: "mix",
    },
    LspRecipe {
        language: "haskell",
        extensions: &["hs"],
        server_name: "haskell-language-server",
        install_command: "ghcup install hls",
        binary_name: "haskell-language-server-wrapper",
        args: &["--lsp"],
        package_manager: "ghcup",
    },
    LspRecipe {
        language: "ocaml",
        extensions: &["ml", "mli"],
        server_name: "ocamllsp",
        install_command: "opam install ocaml-lsp-server",
        binary_name: "ocamllsp",
        args: &[],
        package_manager: "opam",
    },
    LspRecipe {
        language: "erlang",
        extensions: &["erl", "hrl"],
        server_name: "erlang_ls",
        install_command: "brew install erlang_ls",
        binary_name: "erlang_ls",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "r",
        extensions: &["r", "R"],
        server_name: "languageserver",
        install_command: "R -e 'install.packages(\"languageserver\")'",
        binary_name: "R",
        args: &["--slave", "-e", "languageserver::run()"],
        package_manager: "R",
    },
    LspRecipe {
        language: "shellscript",
        extensions: &["sh", "bash"],
        server_name: "bash-language-server",
        install_command: "npm install -g bash-language-server",
        binary_name: "bash-language-server",
        args: &["start"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "julia",
        extensions: &["jl"],
        server_name: "julia-lsp",
        install_command: "julia -e 'using Pkg; Pkg.add(\"LanguageServer\")'",
        binary_name: "julia",
        args: &["--project=@.", "-e", "using LanguageServer; runserver()"],
        package_manager: "julia",
    },
    // Perl deferred until tree-sitter 0.26 upgrade
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
