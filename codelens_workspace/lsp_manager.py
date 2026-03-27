"""
LSP Manager — manages language servers per language.

Auto-detects required language servers from file extensions,
spawns them lazily, and shuts them down on exit.
If a server binary is not installed, returns None (regex fallback).
"""

from __future__ import annotations

import logging
import shutil
from pathlib import Path
from typing import Dict, Optional

from .lsp_client import LspClient

logger = logging.getLogger("codelens.lsp")

# 54 language server configurations (Serena SolidLSP parity)
LANGUAGE_SERVERS: Dict[str, dict] = {
    # ── Tier 1: Most common ──
    "python": {
        "cmd": ["pyright-langserver", "--stdio"],
        "alt_cmd": ["jedi-language-server"],
        "install": "npm i -g pyright",
    },
    "typescript": {
        "cmd": ["typescript-language-server", "--stdio"],
        "install": "npm i -g typescript-language-server typescript",
    },
    "javascript": {
        "cmd": ["typescript-language-server", "--stdio"],
        "install": "npm i -g typescript-language-server typescript",
    },
    "go": {
        "cmd": ["gopls", "serve"],
        "install": "go install golang.org/x/tools/gopls@latest",
    },
    "rust": {"cmd": ["rust-analyzer"], "install": "rustup component add rust-analyzer"},
    "java": {
        "cmd": ["jdtls"],
        "install": "see https://github.com/eclipse-jdtls/eclipse.jdt.ls",
    },
    "kotlin": {
        "cmd": ["kotlin-language-server"],
        "install": "see https://github.com/fwcd/kotlin-language-server",
    },
    "c": {
        "cmd": ["clangd"],
        "alt_cmd": ["ccls"],
        "install": "brew install llvm OR apt install clangd",
    },
    "cpp": {
        "cmd": ["clangd"],
        "alt_cmd": ["ccls"],
        "install": "brew install llvm OR apt install clangd",
    },
    "csharp": {
        "cmd": ["OmniSharp", "-lsp"],
        "install": "see https://github.com/OmniSharp/omnisharp-roslyn",
    },
    # ── Tier 2: Web & scripting ──
    "ruby": {
        "cmd": ["ruby-lsp"],
        "alt_cmd": ["solargraph", "stdio"],
        "install": "gem install ruby-lsp",
    },
    "php": {
        "cmd": ["intelephense", "--stdio"],
        "alt_cmd": ["phpactor", "language-server"],
        "install": "npm i -g intelephense",
    },
    "vue": {
        "cmd": ["vue-language-server", "--stdio"],
        "install": "npm i -g @vue/language-server",
    },
    "svelte": {
        "cmd": ["svelteserver", "--stdio"],
        "install": "npm i -g svelte-language-server",
    },
    "html": {
        "cmd": ["vscode-html-language-server", "--stdio"],
        "install": "npm i -g vscode-langservers-extracted",
    },
    "css": {
        "cmd": ["vscode-css-language-server", "--stdio"],
        "install": "npm i -g vscode-langservers-extracted",
    },
    "json": {
        "cmd": ["vscode-json-language-server", "--stdio"],
        "install": "npm i -g vscode-langservers-extracted",
    },
    # ── Tier 3: Systems ──
    "swift": {"cmd": ["sourcekit-lsp"], "install": "included with Xcode"},
    "dart": {
        "cmd": ["dart", "language-server", "--protocol=lsp"],
        "install": "see https://dart.dev/get-dart",
    },
    "scala": {"cmd": ["metals"], "install": "see https://scalameta.org/metals/"},
    "groovy": {
        "cmd": ["groovy-language-server"],
        "install": "see https://github.com/GroovyLanguageServer/groovy-language-server",
    },
    # ── Tier 4: Functional ──
    "haskell": {
        "cmd": ["haskell-language-server-wrapper", "--lsp"],
        "install": "ghcup install hls",
    },
    "elixir": {
        "cmd": ["elixir-ls"],
        "install": "see https://github.com/elixir-lsp/elixir-ls",
    },
    "erlang": {
        "cmd": ["erlang_ls"],
        "install": "see https://github.com/erlang-ls/erlang_ls",
    },
    "clojure": {"cmd": ["clojure-lsp"], "install": "see https://clojure-lsp.io/"},
    "fsharp": {
        "cmd": ["fsautocomplete", "--adaptive-lsp-server-enabled"],
        "install": "dotnet tool install fsautocomplete",
    },
    "ocaml": {"cmd": ["ocamllsp"], "install": "opam install ocaml-lsp-server"},
    "elm": {
        "cmd": ["elm-language-server"],
        "install": "npm i -g @elm-tooling/elm-language-server",
    },
    # ── Tier 5: Scripting & config ──
    "lua": {
        "cmd": ["lua-language-server"],
        "install": "see https://github.com/LuaLS/lua-language-server",
    },
    "perl": {"cmd": ["perlnavigator"], "install": "npm i -g perlnavigator-server"},
    "bash": {
        "cmd": ["bash-language-server", "start"],
        "install": "npm i -g bash-language-server",
    },
    "powershell": {
        "cmd": [
            "pwsh",
            "-NoLogo",
            "-NoProfile",
            "-Command",
            "Import-Module PowerShellEditorServices; Start-EditorServices -Stdio",
        ],
        "install": "Install-Module PowerShellEditorServices",
    },
    "r": {
        "cmd": ["R", "--no-echo", "-e", "languageserver::run()"],
        "install": "R -e 'install.packages(\"languageserver\")'",
    },
    "julia": {
        "cmd": [
            "julia",
            "--startup-file=no",
            "-e",
            "using LanguageServer; runserver()",
        ],
        "install": "julia -e 'using Pkg; Pkg.add(\"LanguageServer\")'",
    },
    # ── Tier 6: Config & markup ──
    "yaml": {
        "cmd": ["yaml-language-server", "--stdio"],
        "install": "npm i -g yaml-language-server",
    },
    "toml": {"cmd": ["taplo", "lsp", "stdio"], "install": "cargo install taplo-cli"},
    "markdown": {
        "cmd": ["marksman", "server"],
        "install": "see https://github.com/artempyanykh/marksman",
    },
    "terraform": {
        "cmd": ["terraform-ls", "serve"],
        "install": "see https://github.com/hashicorp/terraform-ls",
    },
    "nix": {"cmd": ["nixd"], "install": "nix profile install nixpkgs#nixd"},
    # ── Tier 7: Specialized ──
    "zig": {"cmd": ["zls"], "install": "see https://github.com/zigtools/zls"},
    "lean": {
        "cmd": ["lean", "--server"],
        "install": "see https://leanprover.github.io/",
    },
    "solidity": {
        "cmd": ["nomicfoundation-solidity-language-server", "--stdio"],
        "install": "npm i -g @nomicfoundation/solidity-language-server",
    },
    "fortran": {"cmd": ["fortls"], "install": "pip install fortran-language-server"},
    "pascal": {
        "cmd": ["pasls"],
        "install": "see https://github.com/castle-engine/pascal-language-server",
    },
    "matlab": {
        "cmd": ["matlab-language-server", "--stdio"],
        "install": "npm i -g matlab-language-server",
    },
    "verilog": {
        "cmd": ["svlangserver"],
        "install": "npm i -g @imc-trading/svlangserver",
    },
    "hlsl": {
        "cmd": ["shader-language-server"],
        "install": "cargo install shader-language-server",
    },
    "ansible": {
        "cmd": ["ansible-language-server", "--stdio"],
        "install": "npm i -g @ansible/ansible-language-server",
    },
    "al": {"cmd": ["al-language-server"], "install": "see AL Language extension"},
    "rego": {
        "cmd": ["regal", "language-server"],
        "install": "see https://github.com/StyraInc/regal",
    },
}

# File extension → language mapping (comprehensive)
EXTENSION_MAP: Dict[str, str] = {
    # Python
    ".py": "python",
    ".pyi": "python",
    ".pyw": "python",
    # JavaScript/TypeScript
    ".ts": "typescript",
    ".tsx": "typescript",
    ".mts": "typescript",
    ".cts": "typescript",
    ".js": "javascript",
    ".jsx": "javascript",
    ".mjs": "javascript",
    ".cjs": "javascript",
    # Go
    ".go": "go",
    # Rust
    ".rs": "rust",
    # JVM
    ".java": "java",
    ".kt": "kotlin",
    ".kts": "kotlin",
    ".scala": "scala",
    ".groovy": "groovy",
    ".gradle": "groovy",
    # C/C++
    ".c": "c",
    ".h": "c",
    ".cpp": "cpp",
    ".hpp": "cpp",
    ".cc": "cpp",
    ".cxx": "cpp",
    ".hh": "cpp",
    ".hxx": "cpp",
    # C#/F#
    ".cs": "csharp",
    ".fs": "fsharp",
    ".fsx": "fsharp",
    # Web
    ".vue": "vue",
    ".svelte": "svelte",
    ".html": "html",
    ".htm": "html",
    ".css": "css",
    ".scss": "css",
    ".less": "css",
    ".json": "json",
    ".jsonc": "json",
    # Ruby/PHP
    ".rb": "ruby",
    ".rake": "ruby",
    ".gemspec": "ruby",
    ".php": "php",
    # Swift/Dart
    ".swift": "swift",
    ".dart": "dart",
    # Functional
    ".hs": "haskell",
    ".lhs": "haskell",
    ".ex": "elixir",
    ".exs": "elixir",
    ".erl": "erlang",
    ".hrl": "erlang",
    ".clj": "clojure",
    ".cljs": "clojure",
    ".cljc": "clojure",
    ".ml": "ocaml",
    ".mli": "ocaml",
    ".elm": "elm",
    # Scripting
    ".lua": "lua",
    ".pl": "perl",
    ".pm": "perl",
    ".sh": "bash",
    ".bash": "bash",
    ".zsh": "bash",
    ".ps1": "powershell",
    ".psm1": "powershell",
    ".r": "r",
    ".R": "r",
    ".jl": "julia",
    # Config/markup
    ".yaml": "yaml",
    ".yml": "yaml",
    ".toml": "toml",
    ".md": "markdown",
    ".tf": "terraform",
    ".tfvars": "terraform",
    ".nix": "nix",
    # Specialized
    ".zig": "zig",
    ".lean": "lean",
    ".sol": "solidity",
    ".f90": "fortran",
    ".f95": "fortran",
    ".f03": "fortran",
    ".f08": "fortran",
    ".pas": "pascal",
    ".pp": "pascal",
    ".m": "matlab",
    ".v": "verilog",
    ".sv": "verilog",
    ".hlsl": "hlsl",
    ".al": "al",
    ".rego": "rego",
}


class LspManager:
    """Manages language servers per language with lazy initialization."""

    def __init__(self, workspace_root: str):
        self.workspace_root = str(Path(workspace_root).resolve())
        self._clients: Dict[str, LspClient] = {}
        self._unavailable: set[str] = set()

    def get_client(self, file_path: str) -> Optional[LspClient]:
        ext = Path(file_path).suffix.lower()
        language = EXTENSION_MAP.get(ext)
        if not language:
            return None
        return self.ensure_started(language)

    def ensure_started(self, language: str) -> Optional[LspClient]:
        if language in self._clients and self._clients[language].is_running():
            return self._clients[language]
        if language in self._unavailable:
            return None

        config = LANGUAGE_SERVERS.get(language)
        if not config:
            self._unavailable.add(language)
            return None

        cmd = config["cmd"][0]
        if not shutil.which(cmd):
            alt_cmd = config.get("alt_cmd")
            if alt_cmd and shutil.which(alt_cmd[0]):
                config = {**config, "cmd": alt_cmd}
            else:
                logger.info(
                    f"Language server for '{language}' not found ({cmd}). "
                    f"Install: {config.get('install', 'see docs')}"
                )
                self._unavailable.add(language)
                return None

        client = LspClient(language, config["cmd"], self.workspace_root)
        if client.start():
            self._clients[language] = client
            return client

        self._unavailable.add(language)
        return None

    def get_available_languages(self) -> list[str]:
        """Return list of languages with running servers."""
        return [lang for lang, client in self._clients.items() if client.is_running()]

    def shutdown_all(self):
        """Stop all running language servers."""
        for lang, client in self._clients.items():
            try:
                client.stop()
                logger.info(f"LSP '{lang}' stopped")
            except Exception as e:
                logger.debug(f"Error stopping LSP '{lang}': {e}")
        self._clients.clear()
        self._unavailable.clear()


class MultiProjectLspManager:
    """Manages LspManager instances per project root for multi-project support.

    Each project gets its own set of language servers, isolated by workspace root.
    The active project is the primary one; others can be queried read-only.
    """

    def __init__(self, primary_root: str):
        self._primary_root = str(Path(primary_root).resolve())
        self._managers: Dict[str, LspManager] = {
            self._primary_root: LspManager(self._primary_root)
        }

    @property
    def primary(self) -> LspManager:
        return self._managers[self._primary_root]

    def get_client(self, file_path: str) -> Optional[LspClient]:
        """Get LSP client, auto-detecting project from file path."""
        resolved = str(Path(file_path).resolve())
        manager = self._find_manager(resolved)
        return manager.get_client(resolved)

    def add_project(self, project_root: str) -> LspManager:
        """Register an additional project for cross-project queries."""
        resolved = str(Path(project_root).resolve())
        if resolved not in self._managers:
            self._managers[resolved] = LspManager(resolved)
            logger.info(f"Multi-project: added '{Path(resolved).name}'")
        return self._managers[resolved]

    def remove_project(self, project_root: str):
        """Remove a project and shut down its language servers."""
        resolved = str(Path(project_root).resolve())
        if resolved == self._primary_root:
            return  # Can't remove primary
        manager = self._managers.pop(resolved, None)
        if manager:
            manager.shutdown_all()

    def list_projects(self) -> list[dict]:
        """List all registered projects with their status."""
        result = []
        for root, manager in self._managers.items():
            result.append(
                {
                    "name": Path(root).name,
                    "path": root,
                    "is_active": root == self._primary_root,
                    "languages": manager.get_available_languages(),
                }
            )
        return result

    def get_manager(self, project_root: str) -> Optional[LspManager]:
        """Get the LspManager for a specific project."""
        resolved = str(Path(project_root).resolve())
        return self._managers.get(resolved)

    def get_manager_by_name(self, project_name: str) -> Optional[LspManager]:
        """Find manager by project directory name."""
        for root, manager in self._managers.items():
            if Path(root).name == project_name:
                return manager
        return None

    def shutdown_all(self):
        """Shut down all language servers across all projects."""
        for root, manager in self._managers.items():
            manager.shutdown_all()
        self._managers.clear()

    def _find_manager(self, file_path: str) -> LspManager:
        """Find the manager whose root is a parent of file_path."""
        best_root = self._primary_root
        best_len = 0
        for root in self._managers:
            if file_path.startswith(root) and len(root) > best_len:
                best_root = root
                best_len = len(root)
        return self._managers[best_root]
