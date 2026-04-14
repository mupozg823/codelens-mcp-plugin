#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request
from typing import Any


def _extract_codelens_server(data: Any, config_format: str) -> dict[str, Any] | None:
    if not isinstance(data, dict):
        return None
    if config_format == "json":
        servers = data.get("mcpServers", {})
    else:
        servers = data.get("mcp_servers", {})
    if not isinstance(servers, dict):
        return None
    server = servers.get("codelens")
    return server if isinstance(server, dict) else None


def load_codelens_config(config_path: pathlib.Path) -> dict[str, Any]:
    info: dict[str, Any] = {
        "config_path": str(config_path),
        "config_exists": config_path.is_file(),
        "server_defined": False,
        "config_format": "unknown",
        "transport": "missing",
        "url": None,
        "command": None,
        "args": [],
        "parse_error": None,
    }
    if not config_path.is_file():
        return info

    try:
        raw = config_path.read_text()
    except OSError as error:
        info["transport"] = "invalid"
        info["parse_error"] = str(error)
        return info

    stripped = raw.lstrip()
    config_format = "json" if config_path.suffix.lower() == ".json" or stripped.startswith("{") else "toml"
    info["config_format"] = config_format

    try:
        data = json.loads(raw) if config_format == "json" else tomllib.loads(raw)
    except (json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        info["transport"] = "invalid"
        info["parse_error"] = str(error)
        return info

    server = _extract_codelens_server(data, config_format)
    if server is None:
        return info

    info["server_defined"] = True
    url = server.get("url")
    command = server.get("command")
    args = server.get("args", [])
    if isinstance(url, str) and url:
        info["transport"] = "http"
        info["url"] = url
        return info
    if isinstance(command, str) and command:
        info["transport"] = "stdio"
        info["command"] = command
        if isinstance(args, list):
            info["args"] = [str(item) for item in args]
        elif isinstance(args, str):
            info["args"] = [args]
        return info

    info["transport"] = "unknown"
    return info


def find_local_binary(root_dir: pathlib.Path) -> str | None:
    override = os.environ.get("CODELENS_BIN")
    if override and os.path.isfile(override) and os.access(override, os.X_OK):
        return override

    local_candidates = [
        root_dir / "target" / "debug" / "codelens-mcp",
        root_dir / "target" / "release" / "codelens-mcp",
    ]
    existing_locals = [
        str(candidate)
        for candidate in local_candidates
        if candidate.is_file() and os.access(candidate, os.X_OK)
    ]
    if existing_locals:
        existing_locals.sort(key=lambda path: pathlib.Path(path).stat().st_mtime, reverse=True)
        return existing_locals[0]

    resolved = shutil.which("codelens-mcp")
    if resolved and os.path.isfile(resolved) and os.access(resolved, os.X_OK):
        return resolved
    return None


def encode_frame(payload: dict[str, Any]) -> bytes:
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    return f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body


def parse_content_length_messages(blob: bytes) -> list[dict[str, Any]]:
    messages: list[dict[str, Any]] = []
    cursor = 0
    while cursor < len(blob):
        while cursor < len(blob) and blob[cursor:cursor + 1] in (b"\r", b"\n", b" ", b"\t"):
            cursor += 1
        if cursor >= len(blob):
            break
        if blob[cursor:cursor + 1] in (b"{", b"["):
            for line in blob[cursor:].splitlines():
                line = line.strip()
                if not line:
                    continue
                messages.append(json.loads(line.decode("utf-8")))
            break

        header_end = blob.find(b"\r\n\r\n", cursor)
        if header_end == -1:
            raise ValueError("missing stdio frame terminator")
        header = blob[cursor:header_end].decode("utf-8")
        content_length = None
        for line in header.split("\r\n"):
            if line.lower().startswith("content-length:"):
                content_length = int(line.split(":", 1)[1].strip())
                break
        if content_length is None:
            raise ValueError(f"missing Content-Length header in frame: {header!r}")
        body_start = header_end + 4
        body_end = body_start + content_length
        if body_end > len(blob):
            raise ValueError("truncated stdio frame body")
        messages.append(json.loads(blob[body_start:body_end].decode("utf-8")))
        cursor = body_end
    return messages


def extract_structured_content(response: dict[str, Any]) -> dict[str, Any]:
    result = response.get("result")
    if not isinstance(result, dict):
        return {}
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return structured
    content = result.get("content")
    if not isinstance(content, list):
        return {}
    for item in content:
        if not isinstance(item, dict) or item.get("type") != "text":
            continue
        text = item.get("text")
        if not isinstance(text, str):
            continue
        start = text.find("{")
        if start == -1:
            continue
        try:
            return json.loads(text[start:])
        except json.JSONDecodeError:
            continue
    return {}


def initialize_request() -> dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "CodeLensDoctor",
                "version": "1.0.0",
            }
        },
    }


def initialized_notification() -> dict[str, Any]:
    return {"jsonrpc": "2.0", "method": "notifications/initialized"}


def prepare_request(root_dir: pathlib.Path) -> dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "prepare_harness_session",
            "arguments": {
                "project": str(root_dir),
                "detail": "compact",
            },
        },
    }


def summarize_prepare_response(response: dict[str, Any]) -> dict[str, Any]:
    structured = extract_structured_content(response)
    visible = structured.get("visible_tools", {})
    tool_names = visible.get("tool_names", []) if isinstance(visible, dict) else []
    warnings = structured.get("warnings", [])
    return {
        "success": response.get("error") is None and not response.get("result", {}).get("isError", False),
        "active_surface": structured.get("active_surface"),
        "warning_count": len(warnings) if isinstance(warnings, list) else None,
        "visible_tool_count": len(tool_names) if isinstance(tool_names, list) else None,
    }


def run_stdio_smoke(root_dir: pathlib.Path, config_path: pathlib.Path) -> dict[str, Any]:
    config = load_codelens_config(config_path)
    command = config.get("command")
    args = config.get("args") or []
    invocation_source = "config"
    if config.get("transport") != "stdio" or not isinstance(command, str) or not command:
        binary = find_local_binary(root_dir)
        if not binary:
            raise RuntimeError("codelens-mcp binary not found")
        command = binary
        args = [str(root_dir), "--transport", "stdio"]
        invocation_source = "local_binary"

    resolved_command = shutil.which(command) if isinstance(command, str) and os.path.sep not in command else command
    argv = [resolved_command or command, *args]
    env = os.environ.copy()
    env.setdefault("MCP_PROJECT_DIR", str(root_dir))

    payload = b"".join(
        [
            encode_frame(initialize_request()),
            encode_frame(initialized_notification()),
            encode_frame(prepare_request(root_dir)),
        ]
    )
    proc = subprocess.Popen(
        argv,
        cwd=str(root_dir),
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    stdout, stderr = proc.communicate(payload, timeout=30)
    responses = parse_content_length_messages(stdout)
    if len(responses) < 2:
        raise RuntimeError(
            f"expected initialize + prepare responses, got {len(responses)} response(s)"
        )
    init_response = responses[0]
    prepare_response = responses[1]
    if init_response.get("error"):
        raise RuntimeError(f"initialize failed: {init_response['error']}")
    if prepare_response.get("error"):
        raise RuntimeError(f"prepare_harness_session failed: {prepare_response['error']}")
    return {
        "success": True,
        "transport": "stdio",
        "endpoint": argv,
        "invocation_source": invocation_source,
        "initialize": {
            "protocol_version": init_response.get("result", {}).get("protocolVersion"),
            "server": init_response.get("result", {}).get("serverInfo", {}).get("name"),
        },
        "prepare_harness_session": summarize_prepare_response(prepare_response),
        "stderr": stderr.decode("utf-8", "replace").strip(),
        "returncode": proc.returncode,
    }


def http_json_request(
    url: str,
    payload: dict[str, Any],
    session_id: str | None = None,
) -> tuple[dict[str, Any], dict[str, str]]:
    data = json.dumps(payload).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json",
    }
    if session_id:
        headers["Mcp-Session-Id"] = session_id
    request = urllib.request.Request(url, data=data, headers=headers, method="POST")
    with urllib.request.urlopen(request, timeout=20) as response:
        body = response.read().decode("utf-8")
        parsed = json.loads(body) if body.strip() else {}
        return parsed, {k.lower(): v for k, v in response.headers.items()}


def run_http_smoke(root_dir: pathlib.Path, config_path: pathlib.Path, url_override: str | None) -> dict[str, Any]:
    config = load_codelens_config(config_path)
    url = url_override or config.get("url") or os.environ.get("CODELENS_MCP_URL") or "http://127.0.0.1:7837/mcp"
    init_response, init_headers = http_json_request(url, initialize_request())
    session_id = init_headers.get("mcp-session-id")
    if init_response.get("error"):
        raise RuntimeError(f"initialize failed: {init_response['error']}")
    if session_id:
        http_json_request(url, initialized_notification(), session_id=session_id)
    prepare_response, _ = http_json_request(
        url,
        prepare_request(root_dir),
        session_id=session_id,
    )
    if prepare_response.get("error"):
        raise RuntimeError(f"prepare_harness_session failed: {prepare_response['error']}")
    return {
        "success": True,
        "transport": "http",
        "endpoint": url,
        "initialize": {
            "protocol_version": init_response.get("result", {}).get("protocolVersion"),
            "server": init_response.get("result", {}).get("serverInfo", {}).get("name"),
            "session_id": session_id,
        },
        "prepare_harness_session": summarize_prepare_response(prepare_response),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Probe CodeLens MCP via real stdio/http MCP handshake.")
    parser.add_argument("root_dir", nargs="?", default=".")
    parser.add_argument("--transport", choices=["auto", "stdio", "http"], default="auto")
    parser.add_argument("--config", default=os.environ.get("CODEX_CONFIG", os.path.expanduser("~/.codex/config.toml")))
    parser.add_argument("--url")
    parser.add_argument("--print-config", action="store_true")
    args = parser.parse_args()

    root_dir = pathlib.Path(args.root_dir).resolve()
    config_path = pathlib.Path(args.config).expanduser()
    config = load_codelens_config(config_path)
    if args.print_config:
        print(json.dumps(config, indent=2))
        return 0

    transport = args.transport
    if transport == "auto":
        transport = config["transport"] if config["transport"] in {"stdio", "http"} else "stdio"

    try:
        if transport == "http":
            summary = run_http_smoke(root_dir, config_path, args.url)
        else:
            summary = run_stdio_smoke(root_dir, config_path)
    except (RuntimeError, subprocess.SubprocessError, urllib.error.URLError, TimeoutError, ValueError, OSError) as error:
        summary = {
        "success": False,
        "transport": transport,
        "config_transport": config.get("transport"),
        "config_path": str(config_path),
        "config_command": config.get("command"),
        "config_args": config.get("args"),
        "config_url": config.get("url"),
        "error": str(error),
    }
        print(json.dumps(summary, indent=2))
        return 1

    summary["config_transport"] = config.get("transport")
    summary["config_path"] = str(config_path)
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
