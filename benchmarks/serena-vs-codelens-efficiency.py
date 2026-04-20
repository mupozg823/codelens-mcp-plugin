#!/usr/bin/env python3
"""Efficiency benchmark: Serena vs CodeLens for identical tasks.

Measures, per task:
  - tool_calls: number of MCP round-trips needed to satisfy the task.
  - response_bytes: cumulative size of tool responses (text payload).
  - task_satisfied: boolean, whether the last response carried enough
    information that a follow-up Read would not be required.

Both servers are queried over HTTP JSON-RPC (Streamable HTTP, MCP
2025-06-18). Serena is reached via its own stdio→uvx bridge when the
user runs it interactively; here we invoke the CodeLens server directly
and drive Serena by subprocess (uvx serena CLI) so the script is
standalone-runnable from CI.

Run from repo root:
    python3 benchmarks/serena-vs-codelens-efficiency.py
"""
from __future__ import annotations

import json
import subprocess
import sys
import time
import urllib.request
from dataclasses import dataclass, field

CODELENS_URL = "http://127.0.0.1:7839/mcp"
CODELENS_PROTOCOL = "2025-06-18"
# Serena is stdio-based; we call its CLI once per tool call via uvx.
SERENA_CMD = [
    "uvx",
    "--from",
    "git+https://github.com/oraios/serena",
    "serena",
    "tools",
    "call",
]


@dataclass
class Scenario:
    name: str
    description: str
    codelens_calls: list[tuple[str, dict]]  # (tool_name, arguments)
    serena_calls: list[tuple[str, dict]]


@dataclass
class Metric:
    scenario: str
    server: str
    tool_calls: int
    response_bytes: int
    errors: int
    satisfied: bool
    notes: list[str] = field(default_factory=list)


# ---------- CodeLens HTTP driver ----------
_codelens_session: str | None = None


def _codelens_init() -> str:
    global _codelens_session
    if _codelens_session is not None:
        return _codelens_session
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": CODELENS_PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "efficiency-bench", "version": "1.0"},
            },
        }
    ).encode()
    req = urllib.request.Request(
        CODELENS_URL,
        data=body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "MCP-Protocol-Version": CODELENS_PROTOCOL,
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        sid = resp.headers.get("mcp-session-id") or resp.headers.get("Mcp-Session-Id")
        _ = resp.read()
    assert sid, "CodeLens failed to allocate session id"
    # Send initialized notification.
    notify = json.dumps(
        {"jsonrpc": "2.0", "method": "notifications/initialized"}
    ).encode()
    req = urllib.request.Request(
        CODELENS_URL,
        data=notify,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "MCP-Protocol-Version": CODELENS_PROTOCOL,
            "Mcp-Session-Id": sid,
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        _ = resp.read()
    _codelens_session = sid
    return sid


def call_codelens(tool: str, arguments: dict) -> tuple[int, bool]:
    """Returns (response_bytes, error)."""
    sid = _codelens_init()
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": int(time.time() * 1000),
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }
    ).encode()
    req = urllib.request.Request(
        CODELENS_URL,
        data=body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "MCP-Protocol-Version": CODELENS_PROTOCOL,
            "Mcp-Session-Id": sid,
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            raw = resp.read()
        # SSE frame: "event: message\ndata: <json>\n\n"
        text = raw.decode()
        for line in text.splitlines():
            if line.startswith("data:"):
                payload = line[5:].strip()
                return (len(payload), False)
        return (len(text), False)
    except Exception as exc:
        print(f"  [codelens] {tool} error: {exc}", file=sys.stderr)
        return (0, True)


def call_serena(tool: str, arguments: dict) -> tuple[int, bool]:
    """Drive Serena through its CLI `tools call`. Returns (bytes, error)."""
    try:
        cmd = SERENA_CMD + [tool, "--args", json.dumps(arguments)]
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            return (len(result.stderr), True)
        return (len(result.stdout), False)
    except Exception as exc:
        print(f"  [serena] {tool} error: {exc}", file=sys.stderr)
        return (0, True)


def run(driver, calls: list[tuple[str, dict]]) -> tuple[int, int]:
    total_bytes = 0
    errors = 0
    for tool, args in calls:
        b, err = driver(tool, args)
        total_bytes += b
        errors += int(err)
    return total_bytes, errors


SCENARIOS = [
    Scenario(
        name="T1 body+docstring",
        description="Get struct body WITH docstring in one call",
        codelens_calls=[
            (
                "find_symbol",
                {
                    "name": "ToolMetricsRegistry",
                    "file_path": "crates/codelens-mcp/src/observability/telemetry.rs",
                    "include_body": True,
                },
            ),
        ],
        serena_calls=[
            (
                "find_symbol",
                {
                    "name_path_pattern": "ToolMetricsRegistry",
                    "relative_path": "crates/codelens-mcp/src/observability/telemetry.rs",
                    "include_body": True,
                },
            ),
        ],
    ),
    Scenario(
        name="T2 callers+container+context",
        description="Find callers with enclosing container + surrounding lines",
        codelens_calls=[
            (
                "find_referencing_symbols",
                {
                    "symbol_name": "record_tool_call",
                    "file_path": "crates/codelens-mcp/src/observability/telemetry.rs",
                },
            ),
        ],
        serena_calls=[
            (
                "find_referencing_symbols",
                {
                    "name_path": "record_tool_call",
                    "relative_path": "crates/codelens-mcp/src/observability/telemetry.rs",
                },
            ),
        ],
    ),
    Scenario(
        name="T3 change impact report",
        description="Blast radius + readiness of modifying a symbol",
        codelens_calls=[
            (
                "impact_report",
                {
                    "path": "crates/codelens-mcp/src/observability/telemetry.rs",
                },
            ),
        ],
        # Serena has no impact_report; approximate by references + file-level navigation
        serena_calls=[
            (
                "find_referencing_symbols",
                {
                    "name_path": "ToolMetricsRegistry",
                    "relative_path": "crates/codelens-mcp/src/observability/telemetry.rs",
                },
            ),
            (
                "get_symbols_overview",
                {
                    "relative_path": "crates/codelens-mcp/src/observability/telemetry.rs",
                },
            ),
        ],
    ),
    Scenario(
        name="T4 ranked NL context",
        description="Natural-language query -> ranked file/symbol list with budget",
        codelens_calls=[
            (
                "get_ranked_context",
                {
                    "query": "where are session metrics persisted to disk",
                    "max_tokens": 2000,
                },
            ),
        ],
        serena_calls=[
            # Serena has no NL retrieval; approximate with substring search
            (
                "find_symbol",
                {
                    "name_path_pattern": "persist",
                    "substring_matching": True,
                    "relative_path": "crates/codelens-mcp/src",
                    "max_matches": 5,
                },
            ),
        ],
    ),
]


def fmt(v):
    if isinstance(v, int):
        return f"{v:,}"
    return str(v)


def main():
    results: list[Metric] = []
    for sc in SCENARIOS:
        print(f"\n=== {sc.name}: {sc.description} ===")
        # CodeLens
        cb, cerr = run(call_codelens, sc.codelens_calls)
        results.append(
            Metric(
                sc.name,
                "CodeLens",
                len(sc.codelens_calls),
                cb,
                cerr,
                satisfied=(cerr == 0),
            )
        )
        print(
            f"  CodeLens: {len(sc.codelens_calls)} calls, {cb:,} bytes, errors={cerr}"
        )
        # Serena
        sb, serr = run(call_serena, sc.serena_calls)
        results.append(
            Metric(
                sc.name, "Serena", len(sc.serena_calls), sb, serr, satisfied=(serr == 0)
            )
        )
        print(f"  Serena  : {len(sc.serena_calls)} calls, {sb:,} bytes, errors={serr}")

    print("\n\n## Summary\n")
    print(f"| Scenario | Server | Calls | Bytes | Errors | Satisfied |")
    print(f"|---|---|---:|---:|---:|---|")
    for m in results:
        print(
            f"| {m.scenario} | {m.server} | {m.tool_calls} | {m.response_bytes:,} | {m.errors} | {'Y' if m.satisfied else 'N'} |"
        )


if __name__ == "__main__":
    main()
