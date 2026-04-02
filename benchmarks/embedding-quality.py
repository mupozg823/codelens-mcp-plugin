#!/usr/bin/env python3
"""Embedding quality benchmark for the actual CodeLens runtime."""

import argparse
import collections
import json
import os
import subprocess
import time
from pathlib import Path


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
    parser.add_argument(
        "--dataset",
        default=os.path.join(
            os.path.dirname(__file__), "embedding-quality-dataset.json"
        ),
    )
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--output", default="benchmarks/embedding-quality-results.json")
    parser.add_argument("--markdown-output", default="")
    parser.add_argument("--max-results", type=int, default=10)
    return parser.parse_args()


ARGS = parse_args()
PROJECT = os.path.abspath(ARGS.project_path)
BIN = os.path.abspath(ARGS.binary)
DATASET = os.path.abspath(ARGS.dataset)


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


def load_dataset():
    return json.loads(Path(DATASET).read_text(encoding="utf-8"))


def query_type_for_item(item):
    return item.get("query_type", "uncategorized")


def candidate_rows(method_name, payload):
    data = (payload or {}).get("data", {})
    if method_name == "semantic_search":
        return [
            {"name": row.get("symbol_name"), "file": row.get("file_path")}
            for row in data.get("results", [])
        ]
    if method_name in {"get_ranked_context", "get_ranked_context_no_semantic"}:
        return [
            {"name": row.get("name"), "file": row.get("file")}
            for row in data.get("symbols", [])
        ]
    return []


def find_rank(expected_symbol, expected_file_suffix, rows):
    for index, row in enumerate(rows, start=1):
        if row.get("name") != expected_symbol:
            continue
        if expected_file_suffix and not str(row.get("file", "")).endswith(
            expected_file_suffix
        ):
            continue
        return index
    return None


def mrr_component(rank):
    return 0.0 if rank is None else 1.0 / rank


def acc_at(rank, k):
    return 0.0 if rank is None else float(rank <= k)


def evaluate_method(name, dataset, tool_name, args_factory):
    rows = []
    for item in dataset:
        tool_result = run_tool(tool_name, args_factory(item), timeout=180)
        payload = tool_result.get("payload") or {}
        candidates = candidate_rows(name, payload)
        rank = find_rank(
            item["expected_symbol"], item.get("expected_file_suffix"), candidates
        )
        rows.append(
            {
                "query": item["query"],
                "query_type": query_type_for_item(item),
                "expected_symbol": item["expected_symbol"],
                "expected_file_suffix": item.get("expected_file_suffix"),
                "rank": rank,
                "elapsed_ms": tool_result["elapsed_ms"],
                "candidate_count": len(candidates),
                "top_candidate": candidates[0] if candidates else None,
            }
        )

    total = len(rows)
    by_type = {}
    grouped = collections.defaultdict(list)
    for row in rows:
        grouped[row["query_type"]].append(row)
    for query_type, group in sorted(grouped.items()):
        type_total = len(group)
        by_type[query_type] = {
            "count": type_total,
            "mrr": sum(mrr_component(row["rank"]) for row in group) / type_total,
            "acc1": sum(acc_at(row["rank"], 1) for row in group) / type_total,
            "acc3": sum(acc_at(row["rank"], 3) for row in group) / type_total,
            "acc5": sum(acc_at(row["rank"], 5) for row in group) / type_total,
            "avg_elapsed_ms": sum(row["elapsed_ms"] for row in group) / type_total,
        }

    return {
        "method": name,
        "mrr": sum(mrr_component(row["rank"]) for row in rows) / total,
        "acc1": sum(acc_at(row["rank"], 1) for row in rows) / total,
        "acc3": sum(acc_at(row["rank"], 3) for row in rows) / total,
        "acc5": sum(acc_at(row["rank"], 5) for row in rows) / total,
        "avg_elapsed_ms": sum(row["elapsed_ms"] for row in rows) / total,
        "by_query_type": by_type,
        "rows": rows,
    }


def render_markdown(result):
    lines = []
    a = lines.append
    a("# Embedding Quality Summary")
    a("")
    a(f"- Project: `{result['project']}`")
    a(f"- Binary: `{result['binary']}`")
    a(f"- Embedding model: `{result['embedding_model']}`")
    a(f"- Dataset size: {result['dataset_size']}")
    a("")
    a("## Metrics")
    a("")
    a("| Method | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |")
    a("|---|---:|---:|---:|---:|---:|")
    for method in result["methods"]:
        a(
            f"| {method['method']} | {method['mrr']:.3f} | {method['acc1']:.0%} | {method['acc3']:.0%} | {method['acc5']:.0%} | {method['avg_elapsed_ms']:.1f} |"
        )
    a("")
    a("## Query Type Breakdown")
    a("")
    a("| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |")
    a("|---|---|---:|---:|---:|---:|---:|---:|")
    for method in result["methods"]:
        for query_type, metrics in method.get("by_query_type", {}).items():
            a(
                f"| {method['method']} | {query_type} | {metrics['count']} | {metrics['mrr']:.3f} | {metrics['acc1']:.0%} | {metrics['acc3']:.0%} | {metrics['acc5']:.0%} | {metrics['avg_elapsed_ms']:.1f} |"
            )
    uplift = result["hybrid_uplift"]
    a("")
    a("## Hybrid Uplift")
    a("")
    a("| KPI | Delta |")
    a("|---|---:|")
    a(f"| MRR uplift | {uplift['mrr_delta']:+.3f} |")
    a(f"| Acc@1 uplift | {uplift['acc1_delta']:+.0%} |")
    a(f"| Acc@3 uplift | {uplift['acc3_delta']:+.0%} |")
    a(f"| Acc@5 uplift | {uplift['acc5_delta']:+.0%} |")
    if result.get("hybrid_uplift_by_query_type"):
        a("")
        a("## Hybrid Uplift by Query Type")
        a("")
        a("| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |")
        a("|---|---:|---:|---:|---:|")
        for query_type, metrics in sorted(result["hybrid_uplift_by_query_type"].items()):
            a(
                f"| {query_type} | {metrics['mrr_delta']:+.3f} | {metrics['acc1_delta']:+.0%} | {metrics['acc3_delta']:+.0%} | {metrics['acc5_delta']:+.0%} |"
            )
    a("")
    a("## Misses")
    a("")
    a("| Method | Query | Rank | Top candidate |")
    a("|---|---|---:|---|")
    for method in result["methods"]:
        for row in method["rows"]:
            if row["rank"] is not None and row["rank"] <= 3:
                continue
            top = row["top_candidate"]
            top_label = ""
            if top:
                top_label = f"{top.get('name')} ({top.get('file')})"
            a(
                f"| {method['method']} | {row['query']} | {row['rank'] if row['rank'] is not None else 'miss'} | {top_label} |"
            )
    a("")
    return "\n".join(lines)


def main():
    dataset = load_dataset()
    capabilities = run_tool("get_capabilities", {})
    embedding_model = ((capabilities.get("payload") or {}).get("data") or {}).get(
        "embedding_model"
    )

    run_tool("index_embeddings", {}, timeout=1800)

    methods = []
    methods.append(
        evaluate_method(
            "semantic_search",
            dataset,
            "semantic_search",
            lambda item: {"query": item["query"], "max_results": ARGS.max_results},
        )
    )
    methods.append(
        evaluate_method(
            "get_ranked_context_no_semantic",
            dataset,
            "get_ranked_context",
            lambda item: {
                "query": item["query"],
                "max_tokens": 1200,
                "include_body": False,
                "disable_semantic": True,
            },
        )
    )
    methods.append(
        evaluate_method(
            "get_ranked_context",
            dataset,
            "get_ranked_context",
            lambda item: {
                "query": item["query"],
                "max_tokens": 1200,
                "include_body": False,
            },
        )
    )

    lexical = next(
        method for method in methods if method["method"] == "get_ranked_context_no_semantic"
    )
    hybrid = next(method for method in methods if method["method"] == "get_ranked_context")
    hybrid_uplift_by_query_type = {}
    for query_type, metrics in hybrid["by_query_type"].items():
        lexical_metrics = lexical["by_query_type"].get(query_type)
        if not lexical_metrics:
            continue
        hybrid_uplift_by_query_type[query_type] = {
            "mrr_delta": metrics["mrr"] - lexical_metrics["mrr"],
            "acc1_delta": metrics["acc1"] - lexical_metrics["acc1"],
            "acc3_delta": metrics["acc3"] - lexical_metrics["acc3"],
            "acc5_delta": metrics["acc5"] - lexical_metrics["acc5"],
        }

    result = {
        "project": PROJECT,
        "binary": BIN,
        "embedding_model": embedding_model,
        "dataset_path": DATASET,
        "dataset_size": len(dataset),
        "methods": methods,
        "hybrid_uplift": {
            "mrr_delta": hybrid["mrr"] - lexical["mrr"],
            "acc1_delta": hybrid["acc1"] - lexical["acc1"],
            "acc3_delta": hybrid["acc3"] - lexical["acc3"],
            "acc5_delta": hybrid["acc5"] - lexical["acc5"],
        },
        "hybrid_uplift_by_query_type": hybrid_uplift_by_query_type,
    }

    output_text = json.dumps(result, ensure_ascii=False, indent=2)
    print(output_text)
    if ARGS.output:
        Path(ARGS.output).write_text(output_text + "\n", encoding="utf-8")
    if ARGS.markdown_output:
        Path(ARGS.markdown_output).write_text(
            render_markdown(result) + "\n", encoding="utf-8"
        )


if __name__ == "__main__":
    main()
