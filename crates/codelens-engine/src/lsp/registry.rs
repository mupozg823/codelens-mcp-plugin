use serde::Serialize;
use std::path::{Path, PathBuf};

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
        args: &["--background-index"],
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
        language: "scala",
        extensions: &["scala", "sc"],
        server_name: "metals",
        install_command: "cs install metals",
        binary_name: "metals",
        args: &[],
        package_manager: "coursier",
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

fn command_candidates(command: &str) -> Vec<String> {
    #[cfg(windows)]
    let mut candidates = vec![command.to_owned()];
    #[cfg(not(windows))]
    let candidates = vec![command.to_owned()];
    #[cfg(windows)]
    if Path::new(command).extension().is_none() {
        let pathext = std::env::var_os("PATHEXT")
            .and_then(|value| value.into_string().ok())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
        for ext in pathext.split(';').filter(|ext| !ext.is_empty()) {
            let normalized = if ext.starts_with('.') {
                ext.to_owned()
            } else {
                format!(".{ext}")
            };
            let candidate = format!("{command}{normalized}");
            if !candidates
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&candidate))
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn resolve_in_dir(dir: &Path, command: &str) -> Option<PathBuf> {
    command_candidates(command)
        .into_iter()
        .map(|candidate| dir.join(candidate))
        .find(|candidate| candidate.is_file())
}

fn fallback_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        for dir in [
            "/opt/homebrew/bin".to_owned(),
            "/usr/local/bin".to_owned(),
            format!("{home}/.cargo/bin"),
            format!("{home}/.fnm/aliases/default/bin"),
            format!("{home}/.nvm/versions/node/current/bin"),
        ] {
            if !dir.is_empty() {
                dirs.push(PathBuf::from(dir));
            }
        }
    }
    #[cfg(windows)]
    {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
        let app_data = std::env::var("APPDATA").unwrap_or_default();
        for dir in [
            format!("{user_profile}\\.cargo\\bin"),
            format!("{app_data}\\npm"),
        ] {
            if !dir.is_empty() {
                dirs.push(PathBuf::from(dir));
            }
        }
    }
    dirs
}

pub(crate) fn resolve_lsp_binary(command: &str) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.components().count() > 1 {
        return if command_path.is_file() {
            Some(command_path.to_path_buf())
        } else if let Some(parent) = command_path.parent() {
            resolve_in_dir(parent, command_path.file_name()?.to_str()?)
        } else {
            None
        };
    }

    if let Some(path_dirs) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_dirs) {
            if let Some(path) = resolve_in_dir(&dir, command) {
                return Some(path);
            }
        }
    }

    for dir in fallback_search_dirs() {
        if let Some(path) = resolve_in_dir(&dir, command) {
            return Some(path);
        }
    }

    if let Some(extra) = std::env::var_os("CODELENS_LSP_PATH_EXTRA") {
        for dir in std::env::split_paths(&extra) {
            if let Some(path) = resolve_in_dir(&dir, command) {
                return Some(path);
            }
        }
    }

    None
}

/// Return `true` when the given LSP binary is resolvable either via the
/// current `PATH` or via a conservative allow-list of common install
/// locations. This keeps runtime capability reporting and `check_lsp_status`
/// aligned even when the daemon inherits a minimal launchd/systemd PATH.
pub fn lsp_binary_exists(command: &str) -> bool {
    resolve_lsp_binary(command).is_some()
}

/// Check which LSP servers are installed and which are missing.
pub fn check_lsp_status() -> Vec<LspStatus> {
    LSP_RECIPES
        .iter()
        .map(|recipe| LspStatus {
            language: recipe.language,
            server_name: recipe.server_name,
            installed: lsp_binary_exists(recipe.binary_name),
            install_command: recipe.install_command,
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

pub fn default_lsp_command_for_extension(extension: &str) -> Option<&'static str> {
    get_lsp_recipe(extension).map(|recipe| recipe.binary_name)
}

pub fn default_lsp_command_for_path(file_path: &str) -> Option<&'static str> {
    Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(default_lsp_command_for_extension)
}

pub fn default_lsp_args_for_command(command: &str) -> Option<&'static [&'static str]> {
    LSP_RECIPES
        .iter()
        .find(|recipe| recipe.binary_name == command)
        .map(|recipe| recipe.args)
}
