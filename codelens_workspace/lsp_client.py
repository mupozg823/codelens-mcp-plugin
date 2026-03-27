"""
LSP Client — JSON-RPC 2.0 over subprocess.

Manages a single Language Server process and provides typed methods
for common LSP requests (documentSymbol, references, rename, hover).
Falls back gracefully if the server is unavailable.
"""

from __future__ import annotations

import itertools
import json
import logging
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any, Dict, List, Optional
from urllib.parse import quote as url_quote

logger = logging.getLogger("codelens.lsp")

# LSP SymbolKind → CodeLens kind string
LSP_KIND_MAP = {
    1: "file",
    2: "module",
    3: "namespace",
    4: "package",
    5: "class",
    6: "method",
    7: "property",
    8: "field",
    9: "constructor",
    10: "enum",
    11: "interface",
    12: "function",
    13: "variable",
    14: "constant",
    15: "string",
    16: "number",
    17: "boolean",
    18: "array",
    19: "object",
    20: "key",
    21: "null",
    22: "enum_member",
    23: "struct",
    24: "event",
    25: "operator",
    26: "type_parameter",
}


def _file_uri(path: str) -> str:
    """Convert absolute file path to file:// URI."""
    abs_path = str(Path(path).resolve())
    return "file://" + url_quote(abs_path, safe="/:")


def _uri_to_path(uri: str) -> str:
    """Convert file:// URI back to path."""
    if uri.startswith("file://"):
        from urllib.parse import unquote

        return unquote(uri[7:])
    return uri


class LspClient:
    """Communicates with a single Language Server over stdio JSON-RPC 2.0."""

    def __init__(self, name: str, command: list[str], workspace_root: str):
        self.name = name
        self.command = command
        self.workspace_root = str(Path(workspace_root).resolve())
        self._process: Optional[subprocess.Popen] = None
        self._request_id = itertools.count(1)
        self._write_lock = threading.Lock()
        self._initialized = False

    def start(self) -> bool:
        """Spawn the language server and complete initialize handshake."""
        try:
            self._process = subprocess.Popen(
                self.command,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                cwd=self.workspace_root,
            )
        except FileNotFoundError:
            logger.debug(f"LSP server '{self.command[0]}' not found on PATH")
            return False
        except Exception as e:
            logger.debug(f"Failed to start LSP '{self.name}': {e}")
            return False

        # Initialize handshake
        try:
            result = self._send_request(
                "initialize",
                {
                    "processId": None,
                    "capabilities": {
                        "textDocument": {
                            "documentSymbol": {
                                "hierarchicalDocumentSymbolSupport": True
                            },
                            "references": {},
                            "rename": {"prepareSupport": False},
                            "hover": {},
                        }
                    },
                    "rootUri": _file_uri(self.workspace_root),
                    "workspaceFolders": [
                        {
                            "uri": _file_uri(self.workspace_root),
                            "name": Path(self.workspace_root).name,
                        }
                    ],
                },
            )
            if result is not None:
                self._send_notification("initialized", {})
                self._initialized = True
                logger.info(f"LSP '{self.name}' started successfully")
                return True
        except Exception as e:
            logger.debug(f"LSP '{self.name}' initialize failed: {e}")
            self.stop()
        return False

    def stop(self):
        """Graceful shutdown: shutdown request → exit notification → kill."""
        if self._process and self._process.poll() is None:
            try:
                self._send_request("shutdown", None)
                self._send_notification("exit", None)
            except Exception:
                pass
            try:
                self._process.terminate()
                self._process.wait(timeout=3)
            except Exception:
                try:
                    self._process.kill()
                except Exception:
                    pass
        self._process = None
        self._initialized = False

    def is_running(self) -> bool:
        return (
            self._process is not None
            and self._process.poll() is None
            and self._initialized
        )

    # ── LSP Requests ──

    def document_symbols(self, file_path: str) -> list[dict]:
        """Get hierarchical document symbols (textDocument/documentSymbol)."""
        uri = _file_uri(file_path)
        self._ensure_open(file_path)
        result = self._send_request(
            "textDocument/documentSymbol", {"textDocument": {"uri": uri}}
        )
        return result if isinstance(result, list) else []

    def find_references(self, file_path: str, line: int, col: int) -> list[dict]:
        """Find all references (textDocument/references). Line/col are 0-based."""
        uri = _file_uri(file_path)
        self._ensure_open(file_path)
        result = self._send_request(
            "textDocument/references",
            {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": col},
                "context": {"includeDeclaration": True},
            },
        )
        return result if isinstance(result, list) else []

    def rename(
        self, file_path: str, line: int, col: int, new_name: str
    ) -> Optional[dict]:
        """Rename symbol (textDocument/rename). Returns WorkspaceEdit or None."""
        uri = _file_uri(file_path)
        self._ensure_open(file_path)
        return self._send_request(
            "textDocument/rename",
            {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": col},
                "newName": new_name,
            },
        )

    def hover(self, file_path: str, line: int, col: int) -> Optional[dict]:
        """Get hover info (textDocument/hover). Returns hover content or None."""
        uri = _file_uri(file_path)
        self._ensure_open(file_path)
        return self._send_request(
            "textDocument/hover",
            {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": col},
            },
        )

    # ── File Synchronization ──

    def _ensure_open(self, file_path: str):
        """Send didOpen if file not yet opened."""
        uri = _file_uri(file_path)
        try:
            content = Path(file_path).read_text(errors="replace")
        except Exception:
            return
        lang_id = self._detect_language_id(file_path)
        self._send_notification(
            "textDocument/didOpen",
            {
                "textDocument": {
                    "uri": uri,
                    "languageId": lang_id,
                    "version": 1,
                    "text": content,
                }
            },
        )

    def did_close(self, file_path: str):
        """Notify server that file is closed."""
        self._send_notification(
            "textDocument/didClose", {"textDocument": {"uri": _file_uri(file_path)}}
        )

    # ── JSON-RPC 2.0 Transport ──

    def _send_request(self, method: str, params: Any) -> Any:
        """Send a request and wait for response."""
        if not self._process or self._process.poll() is not None:
            return None
        req_id = next(self._request_id)
        message = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            message["params"] = params

        self._write_message(message)
        return self._read_response(req_id)

    def _send_notification(self, method: str, params: Any):
        """Send a notification (no response expected)."""
        if not self._process or self._process.poll() is not None:
            return
        message = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            message["params"] = params
        self._write_message(message)

    def _write_message(self, message: dict):
        """Write a JSON-RPC message with Content-Length header."""
        body = json.dumps(message).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
        with self._write_lock:
            assert self._process and self._process.stdin
            self._process.stdin.write(header + body)
            self._process.stdin.flush()

    def _read_response(self, expected_id: int, timeout: float = 30.0) -> Any:
        """Read messages until we get the response matching expected_id."""
        import select

        assert self._process and self._process.stdout

        deadline = __import__("time").time() + timeout
        while __import__("time").time() < deadline:
            # Read Content-Length header
            header_line = b""
            while True:
                byte = self._process.stdout.read(1)
                if not byte:
                    return None
                header_line += byte
                if header_line.endswith(b"\r\n\r\n"):
                    break
                if header_line.endswith(b"\n\n"):
                    break

            # Parse Content-Length
            content_length = 0
            for line in header_line.decode("ascii", errors="replace").split("\r\n"):
                if line.lower().startswith("content-length:"):
                    content_length = int(line.split(":")[1].strip())
                    break

            if content_length == 0:
                continue

            # Read body
            body = self._process.stdout.read(content_length)
            if not body:
                return None

            try:
                msg = json.loads(body.decode("utf-8"))
            except json.JSONDecodeError:
                continue

            # Skip notifications (no id)
            if "id" not in msg:
                continue

            if msg.get("id") == expected_id:
                if "error" in msg:
                    logger.debug(f"LSP error: {msg['error']}")
                    return None
                return msg.get("result")

        logger.debug(f"LSP response timeout for request {expected_id}")
        return None

    # ── Helpers ──

    @staticmethod
    def _detect_language_id(file_path: str) -> str:
        ext = Path(file_path).suffix.lower()
        return {
            ".py": "python",
            ".pyi": "python",
            ".ts": "typescript",
            ".tsx": "typescriptreact",
            ".js": "javascript",
            ".jsx": "javascriptreact",
            ".go": "go",
            ".rs": "rust",
            ".java": "java",
            ".kt": "kotlin",
            ".kts": "kotlin",
            ".c": "c",
            ".cpp": "cpp",
            ".h": "c",
            ".hpp": "cpp",
            ".cs": "csharp",
            ".rb": "ruby",
            ".php": "php",
            ".swift": "swift",
            ".scala": "scala",
        }.get(ext, "plaintext")
