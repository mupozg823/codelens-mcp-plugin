from __future__ import annotations

import json
import sys
from typing import Any, Dict, Optional


def success(data: Dict[str, Any]) -> Dict[str, Any]:
    return {"success": True, "data": data}


def error(message: str) -> Dict[str, Any]:
    return {"success": False, "error": message}


def tool_result(payload: Dict[str, Any], is_error: bool) -> Dict[str, Any]:
    text = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    return {"content": [{"type": "text", "text": text}], "structuredContent": payload, "isError": is_error}


def read_stdio_message() -> Optional[Dict[str, Any]]:
    headers: Dict[str, str] = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            if headers:
                break
            continue
        text = line.decode("ascii", errors="ignore").strip()
        if ":" not in text:
            continue
        key, value = text.split(":", 1)
        headers[key.strip().lower()] = value.strip()
    if "content-length" not in headers:
        raise ValueError("Missing Content-Length header")
    length = int(headers["content-length"])
    body = sys.stdin.buffer.read(length)
    if len(body) != length:
        raise ValueError("Unexpected EOF while reading MCP message body")
    return json.loads(body.decode("utf-8"))


def write_stdio_message(payload: Dict[str, Any]) -> None:
    body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()
