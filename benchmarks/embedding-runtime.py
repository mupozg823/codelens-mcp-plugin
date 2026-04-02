#!/usr/bin/env python3
"""Runtime benchmark for the current embedding path.

Measures the embedding behavior of the actual CodeLens binary and current
configured default model. Results are workload- and hardware-specific.
"""

import argparse
import json
import os
import subprocess
import time
from statistics import mean


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
    parser.add_argument("--output", default="")
    return parser.parse_args()


ARGS = parse_args()
PROJECT = os.path.abspath(ARGS.project_path)
BIN = os.path.abspath(ARGS.binary)


def run_tool(cmd, args, timeout=120):
    argv = [BIN, PROJECT, "--preset", ARGS.preset, "--cmd", cmd, "--args", json.dumps(args)]
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


def collect():
    capabilities_before = run_tool("get_capabilities", {})
    index_result = run_tool("index_embeddings", {}, timeout=1800)
    capabilities_after = run_tool("get_capabilities", {})

    semantic_runs = [
        run_tool("semantic_search", {"query": ARGS.query, "max_results": 5})
        for _ in range(ARGS.search_runs)
    ]
    ranked_runs = [
        run_tool(
            "get_ranked_context",
            {"query": ARGS.ranked_query, "max_tokens": 800, "include_body": False},
        )
        for _ in range(ARGS.ranked_runs)
    ]
    onboard_run = run_tool("onboard_project", {}, timeout=300)

    def summarize_runs(runs):
        payloads = [run["payload"] or {} for run in runs]
        counts = [
            payload.get("data", {}).get("count")
            or len(payload.get("data", {}).get("results", []))
            or len(payload.get("data", {}).get("symbols", []))
            for payload in payloads
        ]
        return {
            "runs": len(runs),
            "elapsed_ms": [run["elapsed_ms"] for run in runs],
            "avg_elapsed_ms": round(mean(run["elapsed_ms"] for run in runs), 2),
            "max_elapsed_ms": max(run["elapsed_ms"] for run in runs),
            "result_counts": counts,
        }

    after_data = (capabilities_after["payload"] or {}).get("data", {})
    onboard_data = (onboard_run["payload"] or {}).get("data", {})
    semantic_status = onboard_data.get("semantic", {})

    return {
        "project": PROJECT,
        "binary": BIN,
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
    }


def main():
    result = collect()
    text = json.dumps(result, ensure_ascii=False, indent=2)
    print(text)
    if ARGS.output:
        with open(ARGS.output, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")


if __name__ == "__main__":
    main()
