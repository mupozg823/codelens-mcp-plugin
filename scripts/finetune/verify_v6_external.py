#!/usr/bin/env python3
"""Deprecated heuristic external verification.

Do not use this script as promotion evidence. It relies on keyword-hit heuristics,
not exact expected-symbol labels. Use `benchmarks/external-retrieval.py` instead.
"""

import json
import os
import subprocess
import time
from pathlib import Path

BIN = str(Path(__file__).parent.parent.parent / "target" / "release" / "codelens-mcp")
PROJECT = "/Users/bagjaeseog/Downloads/claudex/claw-dev"

# NL queries → expected symbols (manually chosen from claw-dev structure)
TEST_QUERIES = [
    {
        "query": "agent configuration and setup",
        "expected_keywords": ["agent", "config"],
    },
    {
        "query": "handle user input from command line",
        "expected_keywords": ["cli", "input", "command"],
    },
    {
        "query": "send request to anthropic API",
        "expected_keywords": ["anthropic", "proxy", "provider", "request"],
    },
    {"query": "define available tools", "expected_keywords": ["tool"]},
    {
        "query": "type definitions and interfaces",
        "expected_keywords": ["type", "interface"],
    },
    {"query": "run tests", "expected_keywords": ["test"]},
    {
        "query": "process streaming response",
        "expected_keywords": ["stream", "response"],
    },
    {
        "query": "error handling and retry logic",
        "expected_keywords": ["error", "retry", "handle"],
    },
    {
        "query": "parse and validate configuration",
        "expected_keywords": ["config", "parse", "valid"],
    },
    {
        "query": "render terminal output",
        "expected_keywords": ["render", "terminal", "output", "cli"],
    },
]


def run_tool(cmd, args, model_dir=None, timeout=120):
    env = os.environ.copy()
    if model_dir:
        env["CODELENS_MODEL_DIR"] = model_dir
    argv = [
        BIN,
        PROJECT,
        "--preset",
        "balanced",
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    try:
        result = subprocess.run(
            argv, capture_output=True, text=True, timeout=timeout, env=env
        )
        if result.returncode != 0:
            return None
        output = result.stdout.strip()
        if output:
            return json.loads(output.splitlines()[-1])
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        pass
    return None


def index_project(model_dir=None):
    print(f"  Indexing {PROJECT}...")
    t0 = time.time()
    result = run_tool("index_embeddings", {}, model_dir=model_dir, timeout=300)
    elapsed = time.time() - t0
    if result and result.get("success"):
        count = result.get("data", {}).get("indexed_symbols", 0)
        print(f"  Indexed {count} symbols in {elapsed:.1f}s")
        return count
    print(f"  Indexing failed: {result}")
    return 0


def evaluate_queries(model_dir=None):
    results = []
    for test in TEST_QUERIES:
        query = test["query"]
        keywords = test["expected_keywords"]

        result = run_tool(
            "semantic_search", {"query": query, "max_results": 5}, model_dir=model_dir
        )
        if not result or not result.get("success"):
            results.append({"query": query, "hit": False, "reason": "search failed"})
            continue

        matches = result.get("data", {}).get("results", [])
        if not matches:
            matches = result.get("data", {}).get("matches", [])

        # Check if any result contains expected keywords
        hit = False
        matched_symbol = ""
        for m in matches:
            symbol_name = str(m.get("name", "") or m.get("symbol", "")).lower()
            file_path = str(m.get("file", "") or m.get("file_path", "")).lower()
            combined = symbol_name + " " + file_path

            if any(kw.lower() in combined for kw in keywords):
                hit = True
                matched_symbol = m.get("name", "") or m.get("symbol", "")
                break

        results.append(
            {
                "query": query,
                "hit": hit,
                "matched": matched_symbol,
                "total_results": len(matches),
                "top_results": [
                    m.get("name", "") or m.get("symbol", "") for m in matches[:3]
                ],
            }
        )

    return results


def print_results(label, results):
    hits = sum(1 for r in results if r["hit"])
    total = len(results)
    print(f"\n=== {label}: {hits}/{total} ({hits/total*100:.0f}%) ===")
    for r in results:
        status = "✓" if r["hit"] else "✗"
        matched = f" → {r['matched']}" if r.get("matched") else ""
        top = ", ".join(r.get("top_results", [])[:3])
        print(f"  {status} {r['query'][:50]}{matched}")
        if not r["hit"] and top:
            print(f"    got: {top}")


def main():
    print("=== V6 External Project Verification ===")
    print(f"Project: {PROJECT}")

    # Test with V6 model
    v6_dir = "/tmp/codelens-v6"
    print(f"\n--- V6 Model (internet-only, runtime format) ---")
    index_project(model_dir=v6_dir)
    v6_results = evaluate_queries(model_dir=v6_dir)
    print_results("V6 (internet-only)", v6_results)

    # Test with baseline model
    print(f"\n--- Baseline Model (bundled) ---")
    index_project(model_dir=None)
    baseline_results = evaluate_queries(model_dir=None)
    print_results("Baseline", baseline_results)

    # Compare
    v6_hits = sum(1 for r in v6_results if r["hit"])
    base_hits = sum(1 for r in baseline_results if r["hit"])
    total = len(TEST_QUERIES)
    print(f"\n=== COMPARISON ===")
    print(f"  V6:       {v6_hits}/{total} ({v6_hits/total*100:.0f}%)")
    print(f"  Baseline: {base_hits}/{total} ({base_hits/total*100:.0f}%)")
    diff = v6_hits - base_hits
    if diff > 0:
        print(f"  V6 wins by +{diff}")
    elif diff < 0:
        print(f"  Baseline wins by +{-diff}")
    else:
        print(f"  Tied")


if __name__ == "__main__":
    main()
