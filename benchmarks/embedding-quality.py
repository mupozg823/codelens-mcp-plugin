#!/usr/bin/env python3
"""Embedding quality benchmark for the actual CodeLens runtime."""

import argparse
import collections
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from benchmark_runtime_common import (
    parse_output_json,
    tool_payload_succeeded,
    validate_expected_file_suffixes,
)


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
    parser.add_argument(
        "--embed-model",
        default=os.environ.get("CODELENS_EMBED_MODEL", ""),
        help="Override CODELENS_EMBED_MODEL for this benchmark run",
    )
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if hybrid/lexical MRR floors are not met",
    )
    parser.add_argument(
        "--min-hybrid-mrr",
        type=float,
        default=0.65,
        help="Floor for hybrid MRR@N under --check (default 0.65; self baseline v1.9.36 = 0.681)",
    )
    parser.add_argument(
        "--min-lexical-mrr",
        type=float,
        default=0.50,
        help="Floor for lexical MRR@N under --check",
    )
    return parser.parse_args()


ARGS = parse_args()
SOURCE_PROJECT = os.path.abspath(ARGS.project_path)
PROJECT = SOURCE_PROJECT
BIN = os.path.abspath(ARGS.binary)
DATASET = os.path.abspath(ARGS.dataset)
RUN_ENV = os.environ.copy()
if ARGS.embed_model:
    RUN_ENV["CODELENS_EMBED_MODEL"] = ARGS.embed_model
if "CODELENS_MODEL_DIR" not in RUN_ENV:
    repo_model_dir = (
        Path(__file__).resolve().parent.parent / "crates" / "codelens-engine" / "models"
    )
    if (repo_model_dir / "codesearch" / "model.onnx").exists():
        RUN_ENV["CODELENS_MODEL_DIR"] = str(repo_model_dir)


def compute_file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def resolve_runtime_model_dir():
    configured = RUN_ENV.get("CODELENS_EMBED_MODEL", "")
    if configured and configured != "MiniLM-L12-CodeSearchNet-INT8":
        return None
    exe_dir = Path(BIN).resolve().parent
    candidates = [
        (
            Path(RUN_ENV["CODELENS_MODEL_DIR"]).expanduser().resolve() / "codesearch"
            if "CODELENS_MODEL_DIR" in RUN_ENV
            else None
        ),
        exe_dir / "models" / "codesearch",
        Path.home() / ".cache" / "codelens" / "models" / "codesearch",
        Path(__file__).resolve().parent.parent
        / "crates"
        / "codelens-engine"
        / "models"
        / "codesearch",
    ]
    for candidate in candidates:
        if candidate is not None and (candidate / "model.onnx").exists():
            return candidate
    return None


def payload_data(payload):
    return payload.get("data") if isinstance(payload.get("data"), dict) else payload


def snapshot_runtime_model(capabilities_payload=None):
    data = payload_data(capabilities_payload or {}) or {}
    runtime = {
        "embedding_model": data.get("embedding_model"),
        "runtime_preference": data.get("embedding_runtime_preference"),
        "backend": data.get("embedding_runtime_backend"),
        "threads": data.get("embedding_threads"),
        "max_length": data.get("embedding_max_length"),
        "fallback_reason": data.get("embedding_runtime_fallback_reason"),
        "requested_embed_model": ARGS.embed_model or None,
    }
    model_dir = resolve_runtime_model_dir()
    if model_dir is None:
        return runtime
    model_path = model_dir / "model.onnx"
    config_path = model_dir / "config.json"
    config = (
        json.loads(config_path.read_text(encoding="utf-8"))
        if config_path.exists()
        else {}
    )
    runtime.update(
        {
            "model_dir": str(model_dir),
            "model_path": str(model_path),
            "config_path": str(config_path),
            "sha256": compute_file_sha256(model_path),
            "size_bytes": model_path.stat().st_size,
            "num_hidden_layers": config.get("num_hidden_layers"),
            "hidden_size": config.get("hidden_size"),
        }
    )
    return runtime


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
        env=RUN_ENV,
    )
    elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
    output = result.stdout.strip()
    payload = parse_output_json(output)
    return {
        "elapsed_ms": elapsed_ms,
        "returncode": result.returncode,
        "payload": payload,
        "stderr": result.stderr.strip(),
    }


def tool_succeeded(result):
    payload = result.get("payload")
    return result.get("returncode") == 0 and tool_payload_succeeded(payload)


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


_IGNORE_PATTERNS = shutil.ignore_patterns(
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
)


def copy_project_for_benchmark(source_project: str) -> str:
    # Deterministic copy: walks directory entries in sorted order so that the
    # indexer observes the same filesystem insertion order on every run.
    # shutil.copytree backs onto os.scandir whose order is filesystem-defined;
    # that ±0.003 MRR wobble is observable and we remove it here.
    source = Path(source_project).resolve()
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-embed-quality-"))
    bench_project = temp_root / source.name

    def _copy_dir(src: Path, dst: Path) -> None:
        dst.mkdir(parents=True, exist_ok=True)
        entries = sorted(src.iterdir(), key=lambda p: p.name)
        ignored = _IGNORE_PATTERNS(str(src), [e.name for e in entries])
        for entry in entries:
            if entry.name in ignored:
                continue
            target = dst / entry.name
            if entry.is_symlink():
                link_target = os.readlink(entry)
                os.symlink(link_target, target)
            elif entry.is_dir():
                _copy_dir(entry, target)
            else:
                shutil.copy2(entry, target, follow_symlinks=False)

    _copy_dir(source, bench_project)
    return str(bench_project)


def load_dataset():
    dataset = json.loads(Path(DATASET).read_text(encoding="utf-8"))
    validate_expected_file_suffixes(
        dataset,
        DATASET,
        lambda _row: PROJECT,
        row_label=lambda row: row.get("query") or row.get("expected_symbol"),
    )
    return dataset


def query_type_for_item(item):
    return item.get("query_type", "uncategorized")


def candidate_rows(method_name, payload):
    payload = payload or {}
    data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
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
        tool_result = require_tool_success(
            tool_name,
            run_tool(tool_name, args_factory(item), timeout=180),
            context=item["query"],
        )
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
    runtime_model = result.get("runtime_model") or {}
    a("# Embedding Quality Summary")
    a("")
    a(f"- Project: `{result['project']}`")
    a(f"- Binary: `{result['binary']}`")
    a(f"- Embedding model: `{result['embedding_model']}`")
    if result.get("requested_embed_model"):
        a(f"- Requested embed model override: `{result['requested_embed_model']}`")
    if runtime_model:
        a(
            f"- Runtime backend: `{runtime_model.get('backend')}`, "
            f"preference=`{runtime_model.get('runtime_preference')}`, "
            f"max_length=`{runtime_model.get('max_length')}`"
        )
        if runtime_model.get("model_path"):
            a(
                f"- Runtime model: `{runtime_model.get('num_hidden_layers', '?')}L`, "
                f"`{runtime_model.get('size_bytes', 0) // (1024 * 1024)}MB`, "
                f"`sha256:{str(runtime_model.get('sha256', ''))[:16]}`"
            )
            a(f"- Runtime model path: `{runtime_model.get('model_path')}`")
    a(f"- Dataset size: {result['dataset_size']}")
    a(f"- Ranking cutoff: top-{result['ranking_cutoff']}")
    a("")
    a("## Metrics")
    a("")
    a(f"| Method | MRR@{result['ranking_cutoff']} | Acc@1 | Acc@3 | Acc@5 | Avg ms |")
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
        for query_type, metrics in sorted(
            result["hybrid_uplift_by_query_type"].items()
        ):
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
    capabilities = require_tool_success(
        "get_capabilities", run_tool("get_capabilities", {})
    )
    capability_data = payload_data(capabilities.get("payload") or {}) or {}
    embedding_model = capability_data.get("embedding_model")
    runtime_model = snapshot_runtime_model(capabilities.get("payload") or {})

    require_tool_success(
        "index_embeddings", run_tool("index_embeddings", {}, timeout=1800)
    )

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
        method
        for method in methods
        if method["method"] == "get_ranked_context_no_semantic"
    )
    hybrid = next(
        method for method in methods if method["method"] == "get_ranked_context"
    )
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
        "project": SOURCE_PROJECT,
        "benchmark_project": PROJECT,
        "isolated_copy": bool(ARGS.isolated_copy),
        "binary": BIN,
        "requested_embed_model": ARGS.embed_model or None,
        "embedding_model": embedding_model,
        "runtime_model": runtime_model,
        "dataset_path": DATASET,
        "dataset_size": len(dataset),
        "ranking_cutoff": ARGS.max_results,
        "metric_labels": {
            "mrr": f"MRR@{ARGS.max_results}",
            "acc1": "Acc@1",
            "acc3": "Acc@3",
            "acc5": "Acc@5",
        },
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

    if ARGS.check:
        import sys

        failures = []
        if hybrid["mrr"] < ARGS.min_hybrid_mrr:
            failures.append(
                f"hybrid MRR {hybrid['mrr']:.3f} < floor {ARGS.min_hybrid_mrr:.3f}"
            )
        if lexical["mrr"] < ARGS.min_lexical_mrr:
            failures.append(
                f"lexical MRR {lexical['mrr']:.3f} < floor {ARGS.min_lexical_mrr:.3f}"
            )
        if failures:
            print("\nEmbedding-quality gate failed:")
            for failure in failures:
                print(f"- {failure}")
            sys.exit(1)
        print(
            f"\nEmbedding-quality gate passed: "
            f"hybrid MRR {hybrid['mrr']:.3f} >= {ARGS.min_hybrid_mrr:.3f}, "
            f"lexical MRR {lexical['mrr']:.3f} >= {ARGS.min_lexical_mrr:.3f}"
        )


if __name__ == "__main__":
    cleanup_dir = None
    if ARGS.isolated_copy:
        PROJECT = copy_project_for_benchmark(SOURCE_PROJECT)
        cleanup_dir = str(Path(PROJECT).parent)
    try:
        main()
    finally:
        if cleanup_dir and not ARGS.keep_isolated_copy:
            shutil.rmtree(cleanup_dir, ignore_errors=True)
