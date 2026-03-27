"""
LSP Client — JSON-RPC 2.0 over subprocess.

Manages a single Language Server process and provides typed methods
for common LSP requests (documentSymbol, references, rename, hover).
Includes 2-level caching, indexing wait, and graceful 3-stage shutdown.
"""

from __future__ import annotations

import hashlib
import itertools
import json
import logging
import os
import signal
import subprocess
import time
import threading
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
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
    abs_path = str(Path(path).resolve())
    return "file://" + url_quote(abs_path, safe="/:")


def _uri_to_path(uri: str) -> str:
    if uri.startswith("file://"):
        from urllib.parse import unquote

        return unquote(uri[7:])
    return uri


def _content_hash(file_path: str) -> str:
    """MD5 hash of file content for cache invalidation."""
    try:
        return hashlib.md5(Path(file_path).read_bytes()).hexdigest()
    except Exception:
        return ""


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
        self._start_count = 0
        self._analysis_ready = threading.Event()
        # Track opened files to avoid duplicate didOpen
        self._opened_files: dict[str, int] = {}  # uri → version
        # 2-level cache
        self._symbol_cache: dict[str, Tuple[str, list, Any]] = {}
        self._ref_cache: dict[str, Tuple[str, list]] = {}

    def start(self) -> bool:
        """Spawn the language server and complete initialize handshake."""
        self._start_count += 1
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

        try:
            result = self._send_request(
                "initialize",
                {
                    "processId": os.getpid(),
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
                self._opened_files.clear()
                self._symbol_cache.clear()
                self._ref_cache.clear()
                # Drain initial notifications (analysis progress, diagnostics)
                self._drain_notifications(timeout=3.0)
                logger.info(f"LSP '{self.name}' started (attempt #{self._start_count})")
                return True
        except Exception as e:
            logger.debug(f"LSP '{self.name}' initialize failed: {e}")
            self.stop()
        return False

    def stop(self):
        """Graceful 3-stage shutdown: shutdown request → SIGTERM → SIGKILL."""
        if not self._process or self._process.poll() is not None:
            self._process = None
            self._initialized = False
            return

        # Stage 1: LSP shutdown/exit protocol
        try:
            self._send_request("shutdown", None)
            self._send_notification("exit", None)
            self._process.wait(timeout=2)
            logger.debug(f"LSP '{self.name}' shutdown gracefully (stage 1)")
            self._process = None
            self._initialized = False
            return
        except Exception:
            pass

        # Stage 2: SIGTERM
        try:
            self._process.terminate()
            self._process.wait(timeout=3)
            logger.debug(f"LSP '{self.name}' terminated (stage 2)")
            self._process = None
            self._initialized = False
            return
        except Exception:
            pass

        # Stage 3: SIGKILL
        try:
            self._process.kill()
            self._process.wait(timeout=1)
            logger.debug(f"LSP '{self.name}' killed (stage 3)")
        except Exception:
            pass

        self._process = None
        self._initialized = False

    def restart(self) -> bool:
        """Stop and restart the language server."""
        self.stop()
        return self.start()

    def is_running(self) -> bool:
        if self._process is not None and self._process.poll() is not None:
            # Process died unexpectedly
            logger.debug(
                f"LSP '{self.name}' process died (exit={self._process.poll()})"
            )
            self._process = None
            self._initialized = False
        return self._process is not None and self._initialized

    def _ensure_alive(self) -> bool:
        """Auto-restart if crashed. Returns True if running."""
        if self.is_running():
            return True
        if self._start_count < 3:
            logger.info(f"LSP '{self.name}' auto-restarting...")
            return self.start()
        return False

    # ── LSP Requests with caching ──

    def document_symbols(self, file_path: str) -> list[dict]:
        """Get hierarchical document symbols with cache."""
        if not self._ensure_alive():
            return []

        # Check cache
        h = _content_hash(file_path)
        cached = self._symbol_cache.get(file_path)
        if cached and cached[0] == h:
            return cached[1]

        self._ensure_open(file_path)
        result = self._send_request(
            "textDocument/documentSymbol",
            {"textDocument": {"uri": _file_uri(file_path)}},
        )
        symbols = result if isinstance(result, list) else []

        # Store in cache
        if symbols:
            self._symbol_cache[file_path] = (h, symbols, None)

        return symbols

    def find_references(
        self, file_path: str, line: int, col: int, retry: bool = True
    ) -> list[dict]:
        """Find all references with indexing wait and retry."""
        if not self._ensure_alive():
            return []

        # Cache key includes position
        cache_key = f"{file_path}:{line}:{col}"
        h = _content_hash(file_path)
        cached = self._ref_cache.get(cache_key)
        if cached and cached[0] == h:
            return cached[1]

        self._ensure_open(file_path)

        # First attempt
        result = self._send_request(
            "textDocument/references",
            {
                "textDocument": {"uri": _file_uri(file_path)},
                "position": {"line": line, "character": col},
                "context": {"includeDeclaration": True},
            },
        )
        refs = result if isinstance(result, list) else []

        # Retry after waiting for analysis completion if empty
        if not refs and retry:
            self.wait_for_analysis(timeout=5.0)
            result = self._send_request(
                "textDocument/references",
                {
                    "textDocument": {"uri": _file_uri(file_path)},
                    "position": {"line": line, "character": col},
                    "context": {"includeDeclaration": True},
                },
            )
            refs = result if isinstance(result, list) else []

        if refs:
            self._ref_cache[cache_key] = (h, refs)

        return refs

    def rename(
        self, file_path: str, line: int, col: int, new_name: str
    ) -> Optional[dict]:
        if not self._ensure_alive():
            return None
        self._ensure_open(file_path)
        return self._send_request(
            "textDocument/rename",
            {
                "textDocument": {"uri": _file_uri(file_path)},
                "position": {"line": line, "character": col},
                "newName": new_name,
            },
        )

    def hover(self, file_path: str, line: int, col: int) -> Optional[dict]:
        if not self._ensure_alive():
            return None
        self._ensure_open(file_path)
        return self._send_request(
            "textDocument/hover",
            {
                "textDocument": {"uri": _file_uri(file_path)},
                "position": {"line": line, "character": col},
            },
        )

    # ── File Synchronization (dedup didOpen) ──

    def _ensure_open(self, file_path: str):
        """Send didOpen only if not already opened or content changed."""
        uri = _file_uri(file_path)
        try:
            content = Path(file_path).read_text(errors="replace")
        except Exception:
            return

        h = _content_hash(file_path)
        current_version = self._opened_files.get(uri, 0)

        if uri in self._opened_files:
            # File already open — send didChange if content changed
            cached = self._symbol_cache.get(file_path)
            if cached and cached[0] == h:
                return  # No change
            current_version += 1
            self._opened_files[uri] = current_version
            self._send_notification(
                "textDocument/didChange",
                {
                    "textDocument": {"uri": uri, "version": current_version},
                    "contentChanges": [{"text": content}],
                },
            )
            # Invalidate caches
            self._symbol_cache.pop(file_path, None)
            for k in list(self._ref_cache.keys()):
                if k.startswith(file_path + ":"):
                    del self._ref_cache[k]
        else:
            # First open
            self._opened_files[uri] = 1
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
        uri = _file_uri(file_path)
        self._opened_files.pop(uri, None)
        self._symbol_cache.pop(file_path, None)
        self._send_notification("textDocument/didClose", {"textDocument": {"uri": uri}})

    def invalidate_cache(self, file_path: Optional[str] = None):
        """Clear cache for a specific file or all files."""
        if file_path:
            self._symbol_cache.pop(file_path, None)
            for k in list(self._ref_cache.keys()):
                if k.startswith(file_path + ":"):
                    del self._ref_cache[k]
        else:
            self._symbol_cache.clear()
            self._ref_cache.clear()

    # ── JSON-RPC 2.0 Transport ──

    def _send_request(self, method: str, params: Any) -> Any:
        if not self._process or self._process.poll() is not None:
            return None
        req_id = next(self._request_id)
        message = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            message["params"] = params
        self._write_message(message)
        return self._read_response(req_id)

    def _send_notification(self, method: str, params: Any):
        if not self._process or self._process.poll() is not None:
            return
        message = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            message["params"] = params
        self._write_message(message)

    def _write_message(self, message: dict):
        body = json.dumps(message).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
        with self._write_lock:
            assert self._process and self._process.stdin
            self._process.stdin.write(header + body)
            self._process.stdin.flush()

    def _read_response(self, expected_id: int, timeout: float = 30.0) -> Any:
        assert self._process and self._process.stdout
        deadline = time.time() + timeout
        while time.time() < deadline:
            header_line = b""
            while True:
                byte = self._process.stdout.read(1)
                if not byte:
                    return None
                header_line += byte
                if header_line.endswith(b"\r\n\r\n") or header_line.endswith(b"\n\n"):
                    break

            content_length = 0
            for line in header_line.decode("ascii", errors="replace").split("\r\n"):
                if line.lower().startswith("content-length:"):
                    content_length = int(line.split(":")[1].strip())
                    break
            if content_length == 0:
                continue

            body = self._process.stdout.read(content_length)
            if not body:
                return None
            try:
                msg = json.loads(body.decode("utf-8"))
            except json.JSONDecodeError:
                continue

            if "id" not in msg:
                self._handle_notification(msg)
                continue
            if msg.get("id") == expected_id:
                if "error" in msg:
                    logger.debug(f"LSP error: {msg['error']}")
                    return None
                return msg.get("result")

        logger.debug(f"LSP response timeout for request {expected_id}")
        return None

    def _drain_notifications(self, timeout: float = 3.0):
        """Read and process pending notifications after initialization."""
        if not self._process or not self._process.stdout:
            return
        import select

        deadline = time.time() + timeout
        while time.time() < deadline:
            # Check if data available (non-blocking)
            ready, _, _ = select.select([self._process.stdout], [], [], 0.2)
            if not ready:
                if self._analysis_ready.is_set():
                    break
                continue
            # Read one message
            header_line = b""
            while True:
                byte = self._process.stdout.read(1)
                if not byte:
                    return
                header_line += byte
                if header_line.endswith(b"\r\n\r\n") or header_line.endswith(b"\n\n"):
                    break
            content_length = 0
            for line in header_line.decode("ascii", errors="replace").split("\r\n"):
                if line.lower().startswith("content-length:"):
                    content_length = int(line.split(":")[1].strip())
                    break
            if content_length == 0:
                continue
            body = self._process.stdout.read(content_length)
            if not body:
                return
            try:
                msg = json.loads(body.decode("utf-8"))
            except json.JSONDecodeError:
                continue
            if "id" not in msg:
                self._handle_notification(msg)

    def _handle_notification(self, msg: dict):
        """Process LSP server notifications for analysis readiness."""
        import re as _re

        method = msg.get("method", "")
        params = msg.get("params", {})

        if method == "window/logMessage":
            message = params.get("message", "")
            # Pyright: "Found X source files"
            if _re.search(r"Found \d+ source files?", message):
                logger.debug(f"LSP '{self.name}': workspace scan complete")
                self._analysis_ready.set()

        elif method == "$/progress":
            value = params.get("value", {})
            kind = value.get("kind", "")
            if kind == "end":
                logger.debug(f"LSP '{self.name}': progress complete")
                self._analysis_ready.set()

        elif method == "textDocument/publishDiagnostics":
            # First diagnostics = server has analyzed at least one file
            if not self._analysis_ready.is_set():
                self._analysis_ready.set()

    def wait_for_analysis(self, timeout: float = 5.0) -> bool:
        """Wait for the LSP server to complete initial workspace analysis."""
        if self._analysis_ready.is_set():
            return True
        logger.debug(f"LSP '{self.name}': waiting for analysis (max {timeout}s)...")
        result = self._analysis_ready.wait(timeout=timeout)
        if result:
            logger.debug(f"LSP '{self.name}': analysis ready")
        else:
            logger.debug(f"LSP '{self.name}': analysis timeout, proceeding anyway")
            self._analysis_ready.set()
        return result

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
