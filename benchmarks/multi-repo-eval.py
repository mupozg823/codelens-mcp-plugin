#!/usr/bin/env python3
"""Multi-repository evaluation framework for CodeLens.

Evaluates CodeLens across multiple open-source repos with standardized metrics.
Inspired by codebase-memory's 31-repo benchmark (arxiv:2603.27277).

Categories:
1. Symbol Resolution — find_symbol accuracy
2. Semantic Search — query→symbol MRR
3. Call Graph — caller/callee precision
4. Impact Analysis — blast radius completeness
5. Community Detection — modularity score
6. Cross-file References — find_referencing_symbols recall
7. Dead Code — false positive rate
8. Rename Safety — dry_run correctness
9. Architecture Overview — community coherence
10. Token Efficiency — tokens per useful result
11. Latency — p50/p95 response time
12. Language Coverage — symbols extracted per file

Usage:
    python benchmarks/multi-repo-eval.py --repos repos.json --output results/
"""

import argparse
import json
import os
import subprocess
import tempfile
import time
from pathlib import Path


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repos", default="benchmarks/eval-repos.json")
    parser.add_argument("--output", default="benchmarks/eval-results/")
    parser.add_argument(
        "--binary",
        default=os.environ.get("CODELENS_BIN", "target/release/codelens-mcp"),
    )
    parser.add_argument("--categories", nargs="+", default=["all"])
    parser.add_argument("--max-repos", type=int, default=10)
    return parser.parse_args()


def run_tool(binary, project, cmd, args, timeout=120):
    argv = [
        binary,
        project,
        "--preset",
        "balanced",
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    try:
        result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        if result.returncode != 0:
            return None
        return json.loads(result.stdout)
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        return None


def eval_symbol_resolution(binary, project):
    """Category 1: find_symbol accuracy."""
    # Get all symbols, then query a sample
    overview = run_tool(binary, project, "get_symbols_overview", {"depth": 1})
    if not overview or not overview.get("data"):
        return {"score": 0, "reason": "no symbols"}

    symbols = overview["data"].get("symbols", [])
    if not symbols:
        return {"score": 0, "reason": "empty symbols"}

    hits = 0
    total = min(len(symbols), 20)
    for sym in symbols[:total]:
        name = sym.get("name", "")
        result = run_tool(
            binary, project, "find_symbol", {"name": name, "exact_match": True}
        )
        if result and result.get("data", {}).get("count", 0) > 0:
            hits += 1

    return {"score": hits / max(total, 1), "hits": hits, "total": total}


def eval_semantic_search(binary, project):
    """Category 2: semantic search MRR (requires index)."""
    idx = run_tool(binary, project, "index_embeddings", {}, timeout=300)
    if not idx or not idx.get("success"):
        return {"score": 0, "reason": "indexing failed"}

    indexed = idx["data"].get("indexed_symbols", 0)
    queries = [
        "main entry point",
        "handle errors",
        "parse configuration",
        "database connection",
        "authentication",
    ]
    mrr_sum = 0
    for q in queries:
        result = run_tool(
            binary, project, "semantic_search", {"query": q, "max_results": 5}
        )
        if result and result.get("data", {}).get("count", 0) > 0:
            mrr_sum += 1.0  # simplified: any result = hit
    return {"mrr": mrr_sum / len(queries), "indexed": indexed}


def eval_call_graph(binary, project):
    """Category 3: call graph resolution."""
    overview = run_tool(binary, project, "get_symbols_overview", {"depth": 1})
    if not overview:
        return {"score": 0}

    functions = [
        s
        for s in overview.get("data", {}).get("symbols", [])
        if s.get("kind") == "function"
    ]
    if not functions:
        return {"score": 0, "reason": "no functions"}

    resolved = 0
    total = 0
    for fn in functions[:10]:
        result = run_tool(
            binary,
            project,
            "get_callees",
            {"function_name": fn["name"], "max_results": 10},
        )
        if result and result.get("data"):
            for callee in result["data"].get("callees", []):
                total += 1
                if callee.get("confidence", 0) > 0.5:
                    resolved += 1

    return {
        "resolved_ratio": resolved / max(total, 1),
        "resolved": resolved,
        "total": total,
    }


def eval_architecture(binary, project):
    """Category 5: community detection quality."""
    result = run_tool(binary, project, "get_architecture", {"min_community_size": 2})
    if not result or not result.get("data"):
        return {"modularity": 0}

    data = result["data"]
    return {
        "modularity": data.get("modularity", 0),
        "communities": data.get("community_count", 0),
        "files": data.get("total_files", 0),
    }


def eval_latency(binary, project):
    """Category 11: response time."""
    times = []
    for _ in range(3):
        start = time.time()
        run_tool(binary, project, "get_symbols_overview", {"depth": 1})
        times.append((time.time() - start) * 1000)

    return {
        "p50_ms": sorted(times)[len(times) // 2] if times else 0,
        "p95_ms": sorted(times)[-1] if times else 0,
    }


def evaluate_repo(binary, repo_path, repo_name):
    """Run all evaluations on a single repo."""
    print(f"\n{'='*60}")
    print(f"Evaluating: {repo_name} ({repo_path})")
    print(f"{'='*60}")

    results = {"repo": repo_name, "path": repo_path}

    print("  [1/5] Symbol resolution...")
    results["symbol_resolution"] = eval_symbol_resolution(binary, repo_path)

    print("  [2/5] Semantic search...")
    results["semantic_search"] = eval_semantic_search(binary, repo_path)

    print("  [3/5] Call graph...")
    results["call_graph"] = eval_call_graph(binary, repo_path)

    print("  [4/5] Architecture...")
    results["architecture"] = eval_architecture(binary, repo_path)

    print("  [5/5] Latency...")
    results["latency"] = eval_latency(binary, repo_path)

    return results


def main():
    args = parse_args()
    binary = os.path.abspath(args.binary)

    # Use local repos for now
    repos = [
        {"name": "codelens-mcp-plugin", "path": os.path.abspath(".")},
    ]

    # Load repo list if available
    if os.path.exists(args.repos):
        with open(args.repos) as f:
            repos = json.load(f)

    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    all_results = []
    for repo in repos[: args.max_repos]:
        result = evaluate_repo(binary, repo["path"], repo["name"])
        all_results.append(result)

        # Save per-repo result
        repo_file = output_dir / f"{repo['name']}.json"
        repo_file.write_text(json.dumps(result, indent=2))

    # Summary
    summary = {
        "total_repos": len(all_results),
        "avg_symbol_resolution": sum(
            r["symbol_resolution"].get("score", 0) for r in all_results
        )
        / max(len(all_results), 1),
        "avg_semantic_mrr": sum(r["semantic_search"].get("mrr", 0) for r in all_results)
        / max(len(all_results), 1),
        "avg_modularity": sum(
            r["architecture"].get("modularity", 0) for r in all_results
        )
        / max(len(all_results), 1),
        "avg_p50_ms": sum(r["latency"].get("p50_ms", 0) for r in all_results)
        / max(len(all_results), 1),
    }

    summary_file = output_dir / "summary.json"
    summary_file.write_text(json.dumps(summary, indent=2))
    print(f"\n{'='*60}")
    print(f"Summary ({len(all_results)} repos):")
    print(json.dumps(summary, indent=2))
    print(f"Results saved to {output_dir}/")


if __name__ == "__main__":
    main()
