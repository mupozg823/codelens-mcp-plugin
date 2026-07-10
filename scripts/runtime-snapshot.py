#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = REPO_ROOT / ".codelens" / "runtime-snapshot.json"
DEFAULT_BINARY = REPO_ROOT / ".codelens" / "bin" / "codelens-mcp-http"


def command_output(command: list[str]) -> str | None:
    result = subprocess.run(
        command,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    return result.stdout.strip() or result.stderr.strip() or None


def source_identity() -> dict[str, Any]:
    return {
        "head": command_output(["git", "rev-parse", "HEAD"]),
        "short_head": command_output(["git", "rev-parse", "--short=7", "HEAD"]),
        "tree": command_output(["git", "write-tree"]),
        "dirty": bool(command_output(["git", "status", "--porcelain"])),
    }


def binary_identity(binary: Path) -> dict[str, Any]:
    version = command_output([str(binary), "--version"]) if binary.is_file() else None
    match = re.search(r"git[:= ]+([0-9a-f]{7,40})", version or "", re.IGNORECASE)
    return {
        "path": str(binary),
        "exists": binary.is_file(),
        "version": version,
        "git_sha": match.group(1) if match else None,
    }


def jsonrpc_result(raw: str) -> dict[str, Any]:
    candidates = [line[5:].strip() for line in raw.splitlines() if line.startswith("data:")]
    if not candidates:
        candidates = [raw.strip()]
    for candidate in candidates:
        try:
            payload = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict) and isinstance(payload.get("result"), dict):
            return payload["result"]
    raise ValueError("daemon response did not contain a JSON-RPC result")


def probe_daemon(port: int, timeout: float) -> dict[str, Any]:
    url = f"http://127.0.0.1:{port}/mcp"
    body = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {"full": True}}
    ).encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "accept": "application/json, text/event-stream",
            "content-type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            result = jsonrpc_result(response.read().decode("utf-8", errors="replace"))
    except (urllib.error.URLError, TimeoutError, ValueError) as error:
        return {"port": port, "url": url, "reachable": False, "error": str(error)}
    runtime = result.get("surface_generation", {}).get("runtime", {})
    return {
        "port": port,
        "url": url,
        "reachable": True,
        "active_surface": result.get("active_surface"),
        "tool_count": result.get("tool_count"),
        "tool_count_total": result.get("tool_count_total"),
        "binary_git_sha": runtime.get("binary_git_sha"),
        "binary_build_time": runtime.get("binary_build_time"),
        "tool_schema_fingerprint": result.get("surface_generation", {}).get(
            "tool_schema_fingerprint"
        ),
    }


def build_snapshot(binary: Path, ports: list[int], timeout: float) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "source": source_identity(),
        "binary": binary_identity(binary),
        "daemons": [probe_daemon(port, timeout) for port in ports],
    }


def snapshot_is_current(snapshot: dict[str, Any]) -> bool:
    source_sha = snapshot["source"].get("short_head")
    daemons = snapshot.get("daemons", [])
    if not daemons or not all(daemon.get("reachable") for daemon in daemons):
        return False
    return all(
        str(daemon.get("binary_git_sha") or "").startswith(str(source_sha or "missing"))
        and isinstance(daemon.get("tool_count"), int)
        for daemon in daemons
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--ports", default="7838,7839")
    parser.add_argument("--timeout", type=float, default=5.0)
    parser.add_argument("--write", nargs="?", const=str(DEFAULT_OUTPUT))
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--source-only", action="store_true")
    args = parser.parse_args()
    ports = [] if args.source_only else [int(value) for value in args.ports.split(",") if value]
    snapshot = build_snapshot(args.binary, ports, args.timeout)
    rendered = json.dumps(snapshot, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.write:
        output = Path(args.write)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    if args.check and not snapshot_is_current(snapshot):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
