#!/usr/bin/env python3
"""Daemon hot-path latency gate for CodeLens retrieval tools."""

from __future__ import annotations

import argparse
import http.client
import json
import os
import statistics
import time
from pathlib import Path

import benchmark_runtime_common as runtime_common


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument("--binary", default=os.environ.get("CODELENS_BIN", "target/release/codelens-mcp"))
    parser.add_argument("--preset", default="full")
    parser.add_argument("--query", default="embedding model and semantic search")
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--runs", type=int, default=20)
    parser.add_argument("--skip-index", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--max-ranked-context-p95-ms", type=float, default=250.0)
    parser.add_argument("--max-semantic-search-p95-ms", type=float, default=900.0)
    parser.add_argument("--require-runtime-backend", default="")
    parser.add_argument("--output-json", default="")
    parser.add_argument("--markdown-output", default="")
    return parser.parse_args()


def percentile(values: list[float], pct: float) -> float:
    ordered = sorted(values)
    idx = (len(ordered) - 1) * pct / 100.0
    lo = int(idx)
    hi = min(lo + 1, len(ordered) - 1)
    frac = idx - lo
    return ordered[lo] * (1 - frac) + ordered[hi] * frac


def mcp_call(conn: http.client.HTTPConnection, method: str, params: dict, request_id: int) -> tuple[float, int, dict]:
    body = json.dumps(
        {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params},
        separators=(",", ":"),
    ).encode("utf-8")
    headers = {
        "content-type": "application/json",
        "accept": "application/json",
        "mcp-protocol-version": "2025-11-25",
    }
    started = time.perf_counter()
    conn.request("POST", "/mcp", body=body, headers=headers)
    response = conn.getresponse()
    raw = response.read()
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    if response.status != 200:
        raise RuntimeError(f"{method} returned HTTP {response.status}: {raw[:500]!r}")
    parsed = json.loads(raw)
    if "error" in parsed:
        raise RuntimeError(f"{method} returned JSON-RPC error: {parsed['error']!r}")
    return elapsed_ms, len(raw), parsed


def tool_call(conn: http.client.HTTPConnection, name: str, arguments: dict, request_id: int) -> tuple[float, int, dict]:
    return mcp_call(conn, "tools/call", {"name": name, "arguments": arguments}, request_id)


def tool_structured_content(response: dict) -> dict:
    result = response.get("result", {})
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return structured
    content = result.get("content")
    if isinstance(content, list) and content:
        text = content[0].get("text") if isinstance(content[0], dict) else None
        if isinstance(text, str):
            try:
                parsed = json.loads(text)
            except json.JSONDecodeError:
                return {}
            if isinstance(parsed, dict):
                data = parsed.get("data")
                return data if isinstance(data, dict) else parsed
    return {}


def summarize(samples: list[float], bytes_out: int) -> dict:
    return {
        "runs": len(samples),
        "bytes": bytes_out,
        "p50_ms": round(statistics.median(samples), 2),
        "p95_ms": round(percentile(samples, 95), 2),
        "min_ms": round(min(samples), 2),
        "max_ms": round(max(samples), 2),
    }


def benchmark_tool(
    conn: http.client.HTTPConnection,
    name: str,
    arguments: dict,
    *,
    warmups: int,
    runs: int,
    request_id_base: int,
) -> dict:
    for idx in range(warmups):
        tool_call(conn, name, arguments, request_id_base + idx)
    samples = []
    bytes_out = 0
    for idx in range(runs):
        elapsed, bytes_out, _ = tool_call(conn, name, arguments, request_id_base + warmups + idx)
        samples.append(elapsed)
    return summarize(samples, bytes_out)


def render_markdown(result: dict) -> str:
    lines = [
        "# CodeLens Daemon Latency Gate",
        "",
        f"- Project: `{result['project']}`",
        f"- Binary: `{result['binary']}`",
        f"- Model dir: `{result['model_dir']}`",
        f"- Runtime backend: `{result['runtime']['embedding_runtime_backend']}`",
        f"- Runtime preference: `{result['runtime']['embedding_runtime_preference']}`",
        "",
        "| Tool | p50 ms | p95 ms | max ms | bytes |",
        "|---|---:|---:|---:|---:|",
    ]
    for name, row in result["tools"].items():
        lines.append(
            f"| `{name}` | {row['p50_ms']} | {row['p95_ms']} | {row['max_ms']} | {row['bytes']} |"
        )
    lines.append("")
    lines.append(f"- Gate passed: `{result['gate']['passed']}`")
    for failure in result["gate"]["failures"]:
        lines.append(f"- Failure: {failure}")
    return "\n".join(lines) + "\n"


def main() -> None:
    args = parse_args()
    binary = Path(args.binary).expanduser().resolve()
    project = Path(args.project_path).expanduser().resolve()
    env = os.environ.copy()
    model_dir = runtime_common.resolve_codelens_model_dir(binary, env=env, repo_root=Path.cwd())
    if model_dir is None:
        raise SystemExit(
            "semantic model assets unavailable; package models/codesearch or set CODELENS_MODEL_DIR"
        )
    env["CODELENS_MODEL_DIR"] = str(model_dir)

    base_url, port, proc = runtime_common.start_http_daemon(
        binary, project, preset=args.preset, env=env
    )
    if not base_url:
        raise SystemExit("failed to start CodeLens HTTP daemon")
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=120)
    try:
        mcp_call(
            conn,
            "initialize",
            {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "daemon-latency-gate", "version": "1.0"}},
            1,
        )
        if not args.skip_index:
            tool_call(conn, "index_embeddings", {}, 2)
        tools = {
            "get_ranked_context_hybrid": benchmark_tool(
                conn,
                "get_ranked_context",
                {"query": args.query, "max_tokens": 800, "include_body": False},
                warmups=args.warmups,
                runs=args.runs,
                request_id_base=100,
            ),
            "get_ranked_context_lexical": benchmark_tool(
                conn,
                "get_ranked_context",
                {"query": args.query, "max_tokens": 800, "include_body": False, "disable_semantic": True},
                warmups=args.warmups,
                runs=args.runs,
                request_id_base=200,
            ),
            "semantic_search": benchmark_tool(
                conn,
                "semantic_search",
                {"query": args.query, "max_results": 5},
                warmups=args.warmups,
                runs=args.runs,
                request_id_base=300,
            ),
        }
        _, _, capabilities = tool_call(conn, "get_capabilities", {}, 900)
        runtime = tool_structured_content(capabilities)
        runtime_backend = runtime.get("embedding_runtime_backend")
        runtime_preference = runtime.get("embedding_runtime_preference")
    finally:
        conn.close()
        runtime_common.stop_http_daemon(proc)

    failures = []
    if tools["get_ranked_context_hybrid"]["p95_ms"] > args.max_ranked_context_p95_ms:
        failures.append(
            "get_ranked_context_hybrid p95 "
            f"{tools['get_ranked_context_hybrid']['p95_ms']}ms > {args.max_ranked_context_p95_ms}ms"
        )
    if tools["semantic_search"]["p95_ms"] > args.max_semantic_search_p95_ms:
        failures.append(
            f"semantic_search p95 {tools['semantic_search']['p95_ms']}ms > {args.max_semantic_search_p95_ms}ms"
        )
    if args.require_runtime_backend and runtime_backend != args.require_runtime_backend:
        failures.append(
            "embedding_runtime_backend "
            f"{runtime_backend!r} != required {args.require_runtime_backend!r}"
        )
    result = {
        "project": str(project),
        "binary": str(binary),
        "model_dir": str(model_dir),
        "runtime": {
            "embedding_runtime_backend": runtime_backend,
            "embedding_runtime_preference": runtime_preference,
        },
        "tools": tools,
        "gate": {
            "passed": not failures,
            "failures": failures,
            "thresholds": {
                "max_ranked_context_p95_ms": args.max_ranked_context_p95_ms,
                "max_semantic_search_p95_ms": args.max_semantic_search_p95_ms,
            },
        },
    }
    text = json.dumps(result, indent=2)
    print(text)
    if args.output_json:
        Path(args.output_json).write_text(text + "\n", encoding="utf-8")
    if args.markdown_output:
        Path(args.markdown_output).write_text(render_markdown(result), encoding="utf-8")
    if args.check and failures:
        raise SystemExit("daemon latency gate failed:\n  - " + "\n  - ".join(failures))


if __name__ == "__main__":
    main()
