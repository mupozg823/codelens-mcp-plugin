#!/usr/bin/env python3
"""Runtime benchmark for the current embedding path.

Measures the embedding behavior of the actual CodeLens binary and current
configured default model. Results are workload- and hardware-specific.
"""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from statistics import mean, median


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument(
        "--binary",
        default=os.environ.get(
            "CODELENS_BIN",
            os.path.join(
                os.path.dirname(__file__), "..", "target", "debug", "codelens-mcp"
            ),
        ),
    )
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--query", default="find code that manages embedding models")
    parser.add_argument("--ranked-query", default="embedding model and semantic search")
    parser.add_argument("--search-runs", type=int, default=3)
    parser.add_argument("--ranked-runs", type=int, default=3)
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    parser.add_argument("--output", default="")
    return parser.parse_args()


ARGS = parse_args()
SOURCE_PROJECT = os.path.abspath(ARGS.project_path)
PROJECT = SOURCE_PROJECT
BIN = os.path.abspath(ARGS.binary)


def run_tool(cmd, args, timeout=120):
    argv = [
        BIN,
        PROJECT,
        "--preset",
        ARGS.preset,
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    t0 = time.perf_counter()
    result = subprocess.run(
        argv,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
    output = result.stdout.strip()
    payload = None
    if output:
        try:
            payload = json.loads(output.splitlines()[-1])
        except json.JSONDecodeError:
            payload = None
    return {
        "elapsed_ms": elapsed_ms,
        "returncode": result.returncode,
        "payload": payload,
        "stderr": result.stderr.strip(),
    }


def tool_succeeded(result):
    payload = result.get("payload")
    return (
        result.get("returncode") == 0
        and isinstance(payload, dict)
        and payload.get("success") is True
    )


def require_tool_success(name, result, context=""):
    if tool_succeeded(result):
        return result
    message = [f"{name} failed"]
    if context:
        message.append(f"context={context}")
    message.append(f"returncode={result.get('returncode')}")
    payload = result.get("payload")
    if payload is not None:
        message.append(f"payload={json.dumps(payload, ensure_ascii=False)}")
    stderr = result.get("stderr")
    if stderr:
        message.append(f"stderr={stderr}")
    raise SystemExit(" | ".join(message))


def copy_project_for_benchmark(source_project: str) -> str:
    source = Path(source_project).resolve()
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-embed-bench-"))
    bench_project = temp_root / source.name
    shutil.copytree(
        source,
        bench_project,
        symlinks=True,
        ignore=shutil.ignore_patterns(
            ".git",
            ".codelens",
            "target",
            "node_modules",
            ".next",
            "dist",
            "coverage",
            "__pycache__",
            ".venv",
            "venv",
            ".pytest_cache",
        ),
    )
    return str(bench_project)


def collect():
    capabilities_before = require_tool_success(
        "get_capabilities",
        run_tool("get_capabilities", {}),
        context="before index_embeddings",
    )
    index_result = require_tool_success(
        "index_embeddings",
        run_tool("index_embeddings", {}, timeout=1800),
    )
    capabilities_after = require_tool_success(
        "get_capabilities",
        run_tool("get_capabilities", {}),
        context="after index_embeddings",
    )

    semantic_runs = []
    for index in range(ARGS.search_runs):
        semantic_runs.append(
            require_tool_success(
                "semantic_search",
                run_tool("semantic_search", {"query": ARGS.query, "max_results": 5}),
                context=f"run={index + 1}",
            )
        )

    ranked_runs = []
    for index in range(ARGS.ranked_runs):
        ranked_runs.append(
            require_tool_success(
                "get_ranked_context",
                run_tool(
                    "get_ranked_context",
                    {"query": ARGS.ranked_query, "max_tokens": 800, "include_body": False},
                ),
                context=f"run={index + 1}",
            )
        )

    onboard_run = require_tool_success(
        "onboard_project",
        run_tool("onboard_project", {}, timeout=300),
    )

    def percentile(values, p):
        s = sorted(values)
        k = (len(s) - 1) * p / 100
        f = int(k)
        c = f + 1 if f + 1 < len(s) else f
        return round(s[f] + (k - f) * (s[c] - s[f]), 2)

    def summarize_runs(runs):
        payloads = [run["payload"] or {} for run in runs]
        counts = [
            payload.get("data", {}).get("count")
            or len(payload.get("data", {}).get("results", []))
            or len(payload.get("data", {}).get("symbols", []))
            for payload in payloads
        ]
        times = [run["elapsed_ms"] for run in runs]
        return {
            "runs": len(runs),
            "elapsed_ms": times,
            "cold_ms": times[0] if times else None,
            "warm_ms": times[1:] if len(times) > 1 else [],
            "avg_elapsed_ms": round(mean(times), 2),
            "p50_ms": round(median(times), 2),
            "p95_ms": (
                percentile(times, 95)
                if len(times) >= 2
                else times[0] if times else None
            ),
            "max_elapsed_ms": max(times),
            "result_counts": counts,
        }

    after_data = (capabilities_after["payload"] or {}).get("data", {})
    onboard_data = (onboard_run["payload"] or {}).get("data", {})
    semantic_status = onboard_data.get("semantic", {})

    build_profile = "release" if "/release/" in BIN else "debug"

    # Measure artifact reuse: call find_reusable for a recent tool
    reuse_run = run_tool("get_tool_metrics", {})
    if tool_succeeded(reuse_run):
        reuse_data = (reuse_run["payload"] or {}).get("data", {}).get("session", {})
        cache_hits = reuse_data.get("analysis_cache_hits", 0)
        cache_total = reuse_data.get("analysis_cache_hits", 0) + reuse_data.get(
            "analysis_cache_misses", 0
        )
        artifact_reuse = {
            "available": True,
            "cache_hits": cache_hits,
            "cache_total": cache_total,
            "hit_rate": round(cache_hits / cache_total, 3) if cache_total > 0 else None,
        }
    else:
        artifact_reuse = {
            "available": False,
            "error": (reuse_run.get("payload") or {}).get("error") or reuse_run.get("stderr"),
        }

    return {
        "project": SOURCE_PROJECT,
        "benchmark_project": PROJECT,
        "isolated_copy": bool(ARGS.isolated_copy),
        "binary": BIN,
        "build_profile": build_profile,
        "preset": ARGS.preset,
        "embedding_model": after_data.get("embedding_model"),
        "embeddings_loaded_before": (capabilities_before["payload"] or {})
        .get("data", {})
        .get("embeddings_loaded"),
        "embedding_indexed_before": (capabilities_before["payload"] or {})
        .get("data", {})
        .get("embedding_indexed"),
        "embedding_indexed_symbols_before": (capabilities_before["payload"] or {})
        .get("data", {})
        .get("embedding_indexed_symbols"),
        "index_embeddings": {
            "elapsed_ms": index_result["elapsed_ms"],
            "payload": index_result["payload"],
        },
        "embeddings_loaded_after": after_data.get("embeddings_loaded"),
        "embedding_indexed_after": after_data.get("embedding_indexed"),
        "embedding_indexed_symbols_after": after_data.get("embedding_indexed_symbols"),
        "semantic_search": summarize_runs(semantic_runs),
        "get_ranked_context": summarize_runs(ranked_runs),
        "onboard_project": {
            "elapsed_ms": onboard_run["elapsed_ms"],
            "semantic": semantic_status,
        },
        "artifact_reuse": artifact_reuse,
    }


def main():
    global PROJECT
    cleanup_dir = None
    if ARGS.isolated_copy:
        PROJECT = copy_project_for_benchmark(SOURCE_PROJECT)
        cleanup_dir = str(Path(PROJECT).parent)
    try:
        result = collect()
        text = json.dumps(result, ensure_ascii=False, indent=2)
        print(text)
        if ARGS.output:
            with open(ARGS.output, "w", encoding="utf-8") as handle:
                handle.write(text + "\n")
    finally:
        if cleanup_dir and not ARGS.keep_isolated_copy:
            shutil.rmtree(cleanup_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
