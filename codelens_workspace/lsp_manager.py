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
