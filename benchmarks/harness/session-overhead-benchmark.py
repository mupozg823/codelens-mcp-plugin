#!/usr/bin/env python3
"""Measure harness-consumer session overhead on top of product benchmark output."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
ROOT = BENCH_DIR.parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))

import benchmark_runtime_common as runtime_common
import session_overhead_common as overhead_common


def build_runtime(project: str, bin_path: Path, count_tokens):
    def codelens(cmd, args, timeout=15, preset=None, profile=None):
        return runtime_common.codelens(
            bin_path,
            project,
            cmd,
            args,
            count_tokens,
            timeout=timeout,
            preset=preset,
            profile=profile,
        )

    def count_json_tokens(payload):
        return runtime_common.count_json_tokens(payload, count_tokens)

    def start_http_daemon(profile=None, preset="full"):
        return runtime_common.start_http_daemon(bin_path, project, profile=profile, preset=preset)

    return runtime_common.BenchmarkRuntime(
        codelens=codelens,
        percentile_95=runtime_common.percentile_95,
        start_http_daemon=start_http_daemon,
        stop_http_daemon=runtime_common.stop_http_daemon,
        mcp_http_call=runtime_common.mcp_http_call,
        initialize_http_session=runtime_common.initialize_http_session,
        mcp_http_tool_call=runtime_common.mcp_http_tool_call,
        mcp_http_resource_read=runtime_common.mcp_http_resource_read,
        extract_tool_payload=runtime_common.extract_tool_payload,
        count_json_tokens=count_json_tokens,
        project=project,
    )


def resolve_binary(explicit: str):
    candidates = []
    if explicit:
        candidates.append(Path(explicit).expanduser())
    env_bin = os.environ.get("CODELENS_BIN")
    if env_bin:
        candidates.append(Path(env_bin).expanduser())
    candidates.extend(
        [
            ROOT / "target" / "release" / "codelens-mcp",
            ROOT / "target" / "debug" / "codelens-mcp",
        ]
    )
    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()
    raise FileNotFoundError("unable to find codelens-mcp binary")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument("--benchmark-json", required=True)
    parser.add_argument("--output-json", required=True)
    parser.add_argument("--binary", default="")
    args = parser.parse_args()

    project = os.path.abspath(args.project_path)
    benchmark = json.loads(Path(args.benchmark_json).expanduser().read_text())
    context = benchmark.get("project_context", {})
    test_symbol = context.get("test_symbol") or "main"
    planner_task = f"understand where to implement changes around {test_symbol}"

    count_tokens, _ = runtime_common.build_token_counter()
    runtime = build_runtime(project, resolve_binary(args.binary), count_tokens)
    session_overhead = overhead_common.run_session_overhead_benchmark(
        benchmark.get("workflow_results", []),
        runtime,
        planner_task,
        context.get("key_file"),
        context.get("test_file"),
        test_symbol,
    )

    output_path = Path(args.output_json).expanduser()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(session_overhead, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"output_json": str(output_path), "supported": session_overhead.get("supported")}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
