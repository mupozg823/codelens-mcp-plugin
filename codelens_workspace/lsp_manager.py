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

# Language server configurations
LANGUAGE_SERVERS: Dict[str, dict] = {
    "python": {
        "cmd": ["pyright-langserver", "--stdio"],
        "alt_cmd": ["pylsp"],
        "install": "pip install pyright",
    },
    "typescript": {
        "cmd": ["typescript-language-server", "--stdio"],
        "install": "npm install -g typescript-language-server typescript",
    },
    "javascript": {
        "cmd": ["typescript-language-server", "--stdio"],
        "install": "npm install -g typescript-language-server typescript",
    },
    "go": {
        "cmd": ["gopls", "serve"],
        "install": "go install golang.org/x/tools/gopls@latest",
    },
    "rust": {
        "cmd": ["rust-analyzer"],
        "install": "rustup component add rust-analyzer",
    },
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
        "install": "brew install llvm  OR  apt install clangd",
    },
    "cpp": {
        "cmd": ["clangd"],
        "install": "brew install llvm  OR  apt install clangd",
    },
    "csharp": {
        "cmd": ["OmniSharp", "-lsp"],
        "install": "see https://github.com/OmniSharp/omnisharp-roslyn",
    },
    "ruby": {
        "cmd": ["solargraph", "stdio"],
        "install": "gem install solargraph",
    },
    "php": {
        "cmd": ["phpactor", "language-server"],
        "install": "see https://github.com/phpactor/phpactor",
    },
}

# File extension → language mapping
EXTENSION_MAP: Dict[str, str] = {
    ".py": "python",
    ".pyi": "python",
    ".ts": "typescript",
    ".tsx": "typescript",
    ".js": "javascript",
    ".jsx": "javascript",
    ".go": "go",
    ".rs": "rust",
    ".java": "java",
    ".kt": "kotlin",
    ".kts": "kotlin",
    ".c": "c",
    ".h": "c",
    ".cpp": "cpp",
    ".hpp": "cpp",
    ".cc": "cpp",
    ".cxx": "cpp",
    ".cs": "csharp",
    ".rb": "ruby",
    ".php": "php",
    ".scala": "java",  # jdtls handles scala with metals alternative
    ".swift": "swift",
}


class LspManager:
    """Manages language servers per language with lazy initialization."""

    def __init__(self, workspace_root: str):
        self.workspace_root = str(Path(workspace_root).resolve())
        self._clients: Dict[str, LspClient] = {}
        self._unavailable: set[str] = set()  # languages where server is not installed

    def get_client(self, file_path: str) -> Optional[LspClient]:
        """Get LSP client for a file. Returns None if no server available."""
        ext = Path(file_path).suffix.lower()
        language = EXTENSION_MAP.get(ext)
        if not language:
            return None
        return self.ensure_started(language)

    def ensure_started(self, language: str) -> Optional[LspClient]:
        """Start language server if not running. Returns None if unavailable."""
        # Already running?
        if language in self._clients and self._clients[language].is_running():
            return self._clients[language]

        # Known to be unavailable?
        if language in self._unavailable:
            return None

        config = LANGUAGE_SERVERS.get(language)
        if not config:
            self._unavailable.add(language)
            return None

        # Check if command exists
        cmd = config["cmd"][0]
        if not shutil.which(cmd):
            # Try alternative command
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

        # Start
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
