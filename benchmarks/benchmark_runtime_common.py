#!/usr/bin/env python3
"""Shared runtime helpers for benchmark scripts."""

from __future__ import annotations

import json
import socket
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable
from urllib import request as urllib_request


@dataclass(frozen=True)
class BenchmarkRuntime:
    codelens: Callable[..., Any]
    percentile_95: Callable[[list[int]], int]
    start_http_daemon: Callable[..., Any]
    stop_http_daemon: Callable[[Any], None]
    mcp_http_call: Callable[..., Any]
    initialize_http_session: Callable[..., Any]
    mcp_http_tool_call: Callable[..., Any]
    mcp_http_resource_read: Callable[..., Any]
    extract_tool_payload: Callable[[Any], dict]
    count_json_tokens: Callable[[Any], int]
    project: str


def build_token_counter():
    try:
        import tiktoken

        enc = tiktoken.get_encoding("cl100k_base")

        def count_tokens(text: str) -> int:
            return len(enc.encode(text)) if text else 0

        return count_tokens, None
    except ImportError:
        def count_tokens(text: str) -> int:
            return len(text.encode("utf-8")) // 4 if text else 0

        return count_tokens, "WARNING: tiktoken not installed. Falling back to bytes/4 estimate."


def percentile_95(values):
    if not values:
        return 0
    ordered = sorted(values)
    index = max(0, int(round(0.95 * (len(ordered) - 1))))
    return ordered[index]


def parse_output_json(output: str):
    text = (output or "").strip()
    if not text:
        return None
    try:
        return json.loads(text.splitlines()[-1])
    except Exception:
        return None


def count_json_tokens(payload, count_tokens):
    if payload is None:
        return 0
    try:
        return count_tokens(json.dumps(payload, ensure_ascii=False, sort_keys=True))
    except Exception:
        return 0


def codelens(bin_path, project, cmd, args, count_tokens, timeout=15, preset=None, profile=None):
    argv = [str(bin_path), str(project)]
    if profile:
        argv += ["--profile", profile]
    elif preset:
        argv += ["--preset", preset]
    argv += ["--cmd", cmd, "--args", json.dumps(args)]
    t0 = time.monotonic()
    try:
        result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        elapsed = int((time.monotonic() - t0) * 1000)
        output = result.stdout or ""
        return output, count_tokens(output), elapsed, parse_output_json(output)
    except Exception:
        elapsed = int((time.monotonic() - t0) * 1000)
        return "", 0, elapsed, None


def read_file(project, path):
    full = Path(path) if Path(path).is_absolute() else Path(project) / path
    try:
        return full.read_text(errors="replace")
    except Exception:
        return ""


def run_search(project, pattern, include="*.rs", max_lines=50):
    t0 = time.monotonic()
    try:
        result = subprocess.run(
            ["rg", "-n", pattern, ".", "-g", include],
            capture_output=True,
            text=True,
            timeout=10,
            cwd=project,
        )
        lines = result.stdout.strip().split("\n")[:max_lines]
        elapsed = int((time.monotonic() - t0) * 1000)
        return "\n".join(lines), elapsed
    except Exception:
        return "", 0


def reserve_port():
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


def start_http_daemon(bin_path, project, profile=None, preset="full"):
    port = reserve_port()
    argv = [str(bin_path), str(project), "--transport", "http"]
    if profile:
        argv += ["--profile", profile]
    elif preset:
        argv += ["--preset", preset]
    argv += ["--port", str(port)]
    proc = subprocess.Popen(
        argv,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    base_url = f"http://127.0.0.1:{port}"
    card_url = f"{base_url}/.well-known/mcp.json"
    for _ in range(50):
        if proc.poll() is not None:
            return None, None, proc
        try:
            with urllib_request.urlopen(card_url, timeout=0.5) as resp:
                if resp.status == 200:
                    return base_url, port, proc
        except Exception:
            time.sleep(0.1)
    return None, None, proc


def stop_http_daemon(proc):
    if not proc:
        return
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)


def mcp_http_call(base_url, method, params=None, request_id=1, headers=None, include_headers=False):
    payload = {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
    }
    if params is not None:
        payload["params"] = params
    request_headers = {"content-type": "application/json"}
    if headers:
        request_headers.update(headers)
    req = urllib_request.Request(
        f"{base_url}/mcp",
        data=json.dumps(payload).encode("utf-8"),
        headers=request_headers,
        method="POST",
    )
    with urllib_request.urlopen(req, timeout=5) as resp:
        parsed = json.loads(resp.read().decode("utf-8"))
        if include_headers:
            return parsed, {key.lower(): value for key, value in resp.headers.items()}
        return parsed


def initialize_http_session(
    base_url,
    profile=None,
    deferred_tool_loading=False,
    trusted_client=None,
    request_id=1,
):
    params = {"clientInfo": {"name": "BenchmarkHarness", "version": "1.0.0"}}
    if profile:
        params["profile"] = profile
    if deferred_tool_loading:
        params["deferredToolLoading"] = True
    headers = {}
    if trusted_client is not None:
        headers["x-codelens-trusted-client"] = "true" if trusted_client else "false"
    response, response_headers = mcp_http_call(
        base_url,
        "initialize",
        params,
        request_id=request_id,
        headers=headers,
        include_headers=True,
    )
    return response_headers.get("mcp-session-id"), response, response_headers


def mcp_http_tool_call(base_url, name, arguments, request_id=1, session_id=None, headers=None):
    request_headers = dict(headers or {})
    if session_id:
        request_headers["mcp-session-id"] = session_id
    return mcp_http_call(
        base_url,
        "tools/call",
        {"name": name, "arguments": arguments},
        request_id=request_id,
        headers=request_headers,
    )


def mcp_http_resource_read(base_url, uri, request_id=1, session_id=None, params=None, headers=None):
    payload = {"uri": uri}
    if params:
        payload.update(params)
    request_headers = dict(headers or {})
    if session_id:
        request_headers["mcp-session-id"] = session_id
    return mcp_http_call(
        base_url,
        "resources/read",
        payload,
        request_id=request_id,
        headers=request_headers,
    )


def extract_tool_payload(response):
    if not isinstance(response, dict):
        return {}
    result = response.get("result")
    if isinstance(result, dict):
        content = result.get("content")
        if isinstance(content, list) and content:
            text = content[0].get("text", "{}")
            try:
                parsed = json.loads(text)
                if isinstance(parsed, dict):
                    return parsed
            except Exception:
                pass
        if "data" in result or "success" in result or "error" in result:
            return result
    error = response.get("error")
    if isinstance(error, dict):
        return {"success": False, "error": error.get("message", "unknown error")}
    return {}
