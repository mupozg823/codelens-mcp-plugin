#!/usr/bin/env python3
"""Exact-label role/adversarial retrieval benchmark for CodeLens."""

from __future__ import annotations

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
    resolve_codelens_model_dir,
    tool_payload_succeeded,
    validate_expected_file_suffixes,
)


DEFAULT_BINARY = (
    Path(__file__).resolve().parent.parent / "target" / "debug" / "codelens-mcp"
)
DEFAULT_DATASET = Path(__file__).resolve().parent / "role-retrieval-dataset.json"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument(
        "--binary",
        default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY)),
    )
    parser.add_argument("--dataset", default=str(DEFAULT_DATASET))
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--output", default="benchmarks/role-retrieval-results.json")
    parser.add_argument("--markdown-output", default="")
    parser.add_argument("--max-results", type=int, default=10)
    parser.add_argument(
        "--embed-model",
        default=os.environ.get("CODELENS_EMBED_MODEL", ""),
        help="Override CODELENS_EMBED_MODEL for this benchmark run",
    )
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    return parser.parse_args()


ARGS = parse_args()
SOURCE_PROJECT = str(Path(ARGS.project_path).expanduser().resolve())
PROJECT = SOURCE_PROJECT
BIN = str(Path(ARGS.binary).expanduser().resolve())
DATASET_PATH = Path(ARGS.dataset).expanduser().resolve()
RUN_ENV = os.environ.copy()
if ARGS.embed_model:
    RUN_ENV["CODELENS_EMBED_MODEL"] = ARGS.embed_model
if "CODELENS_MODEL_DIR" not in RUN_ENV:
    repo_model_dir = (
        Path(__file__).resolve().parent.parent / "crates" / "codelens-engine" / "models"
    )
    if resolve_codelens_model_dir(BIN, env={"CODELENS_MODEL_DIR": str(repo_model_dir)}):
        RUN_ENV["CODELENS_MODEL_DIR"] = str(repo_model_dir)


def compute_file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def resolve_runtime_model_dir() -> Path:
    return resolve_codelens_model_dir(
        BIN,
        env=RUN_ENV,
        repo_root=Path(__file__).resolve().parent.parent,
    )


def payload_data(payload):
    return payload.get("data") if isinstance(payload.get("data"), dict) else payload


def snapshot_runtime_model(capabilities_payload=None) -> dict:
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
    config = json.loads(config_path.read_text(encoding="utf-8")) if config_path.exists() else {}
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


def run_tool(cmd: str, arguments: dict, timeout: int = 180):
    argv = [
        BIN,
        PROJECT,
        "--preset",
        ARGS.preset,
        "--cmd",
        cmd,
        "--args",
        json.dumps(arguments),
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
    payload = parse_output_json(result.stdout)
    return {
        "elapsed_ms": elapsed_ms,
        "returncode": result.returncode,
        "payload": payload,
        "stderr": result.stderr.strip(),
    }


def tool_succeeded(result) -> bool:
    payload = result.get("payload")
    return result.get("returncode") == 0 and tool_payload_succeeded(payload)


def require_tool_success(name: str, result: dict, context: str = "") -> dict:
    if tool_succeeded(result):
        return result
    parts = [f"{name} failed"]
    if context:
        parts.append(f"context={context}")
    parts.append(f"returncode={result.get('returncode')}")
    payload = result.get("payload")
    if payload is not None:
        parts.append(f"payload={json.dumps(payload, ensure_ascii=False)}")
    stderr = result.get("stderr")
    if stderr:
        parts.append(f"stderr={stderr}")
    raise SystemExit(" | ".join(parts))


def copy_project_for_benchmark(source_project: str) -> str:
    source = Path(source_project).resolve()
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-role-retrieval-"))
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


def load_dataset():
    raw = json.loads(DATASET_PATH.read_text(encoding="utf-8"))
    rows = raw.get("rows") if isinstance(raw, dict) else raw
    if not isinstance(rows, list) or not rows:
        raise SystemExit(f"role dataset has no rows: {DATASET_PATH}")
    validate_expected_file_suffixes(
        rows,
        DATASET_PATH,
        lambda _row: PROJECT,
        row_label=lambda row: row.get("query") or row.get("expected_symbol"),
    )
    return raw, rows


def candidate_rows(method_name: str, payload: dict) -> list[dict]:
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


def find_rank(expected_symbol: str, expected_file_suffix: str, rows: list[dict]):
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


def acc_at(rank, k: int):
    return 0.0 if rank is None else float(rank <= k)


def aggregate_rows(rows: list[dict]) -> dict:
    total = len(rows)
    by_query_type = {}
    by_role = {}
    by_group = {}
    grouped_types = collections.defaultdict(list)
    grouped_roles = collections.defaultdict(list)
    grouped_groups = collections.defaultdict(list)
    for row in rows:
        grouped_types[row["query_type"]].append(row)
        grouped_roles[row["role"]].append(row)
        grouped_groups[row["adversarial_group"]].append(row)

    for query_type, group in sorted(grouped_types.items()):
        count = len(group)
        by_query_type[query_type] = {
            "count": count,
            "mrr": sum(mrr_component(row["rank"]) for row in group) / count,
            "acc1": sum(acc_at(row["rank"], 1) for row in group) / count,
            "acc3": sum(acc_at(row["rank"], 3) for row in group) / count,
            "acc5": sum(acc_at(row["rank"], 5) for row in group) / count,
            "avg_elapsed_ms": sum(row["elapsed_ms"] for row in group) / count,
        }
    for role, group in sorted(grouped_roles.items()):
        count = len(group)
        by_role[role] = {
            "count": count,
            "mrr": sum(mrr_component(row["rank"]) for row in group) / count,
            "acc1": sum(acc_at(row["rank"], 1) for row in group) / count,
            "acc3": sum(acc_at(row["rank"], 3) for row in group) / count,
            "acc5": sum(acc_at(row["rank"], 5) for row in group) / count,
            "avg_elapsed_ms": sum(row["elapsed_ms"] for row in group) / count,
        }
    for group_id, group in sorted(grouped_groups.items()):
        count = len(group)
        by_group[group_id] = {
            "count": count,
            "mrr": sum(mrr_component(row["rank"]) for row in group) / count,
            "acc1": sum(acc_at(row["rank"], 1) for row in group) / count,
        }
    return {
        "mrr": sum(mrr_component(row["rank"]) for row in rows) / total if total else None,
        "acc1": sum(acc_at(row["rank"], 1) for row in rows) / total if total else None,
        "acc3": sum(acc_at(row["rank"], 3) for row in rows) / total if total else None,
        "acc5": sum(acc_at(row["rank"], 5) for row in rows) / total if total else None,
        "avg_elapsed_ms": (
            sum(row["elapsed_ms"] for row in rows) / total if total else None
        ),
        "by_query_type": by_query_type,
        "by_role": by_role,
        "by_adversarial_group": by_group,
        "rows": rows,
    }


def evaluate_method(name: str, rows: list[dict], tool_name: str, args_factory):
    evaluated = []
    for item in rows:
        tool_result = require_tool_success(
            tool_name,
            run_tool(tool_name, args_factory(item), timeout=240),
            context=item["query"],
        )
        payload = tool_result.get("payload") or {}
        candidates = candidate_rows(name, payload)
        rank = find_rank(
            item["expected_symbol"],
            item["expected_file_suffix"],
            candidates,
        )
        evaluated.append(
            {
                "query": item["query"],
                "query_type": item.get("query_type", "uncategorized"),
                "expected_symbol": item["expected_symbol"],
                "expected_file_suffix": item.get("expected_file_suffix", ""),
                "role": item.get("role", "unknown"),
                "adversarial_group": item.get("adversarial_group", "unknown"),
                "rank": rank,
                "elapsed_ms": tool_result["elapsed_ms"],
                "candidate_count": len(candidates),
                "top_candidate": candidates[0] if candidates else None,
            }
        )
    summary = aggregate_rows(evaluated)
    summary["method"] = name
    return summary


def render_markdown(result: dict) -> str:
    lines = []
    a = lines.append
    runtime_model = result.get("runtime_model") or {}
    a("# Role Retrieval Summary")
    a("")
    a(f"- Project: `{result['project']}`")
    a(f"- Binary: `{result['binary']}`")
    a(f"- Embedding model: `{result.get('embedding_model')}`")
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
    a(f"- Dataset: `{result['dataset_path']}`")
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
    return "\n".join(lines) + "\n"


def main():
    global PROJECT
    dataset_meta, rows = load_dataset()
    cleanup_dir = None
    if ARGS.isolated_copy:
        PROJECT = copy_project_for_benchmark(SOURCE_PROJECT)
        cleanup_dir = Path(PROJECT).parent

    capabilities = require_tool_success("get_capabilities", run_tool("get_capabilities", {}))
    capability_data = payload_data(capabilities.get("payload") or {}) or {}
    runtime_model = snapshot_runtime_model(capabilities.get("payload") or {})
    require_tool_success("index_embeddings", run_tool("index_embeddings", {}, timeout=600))
    methods = [
        evaluate_method(
            "semantic_search",
            rows,
            "semantic_search",
            lambda item: {"query": item["query"], "max_results": ARGS.max_results},
        ),
        evaluate_method(
            "get_ranked_context",
            rows,
            "get_ranked_context",
            lambda item: {
                "query": item["query"],
                "max_tokens": 1200,
                "include_body": False,
            },
        ),
        evaluate_method(
            "get_ranked_context_no_semantic",
            rows,
            "get_ranked_context",
            lambda item: {
                "query": item["query"],
                "max_tokens": 1200,
                "include_body": False,
                "disable_semantic": True,
            },
        ),
    ]
    result = {
        "schema_version": "codelens-role-retrieval-v1",
        "project": PROJECT,
        "binary": BIN,
        "requested_embed_model": ARGS.embed_model or None,
        "embedding_model": capability_data.get("embedding_model"),
        "runtime_model": runtime_model,
        "dataset_path": str(DATASET_PATH),
        "dataset_size": len(rows),
        "ranking_cutoff": ARGS.max_results,
        "metric_labels": {
            "mrr": f"MRR@{ARGS.max_results}",
            "acc1": "Acc@1",
            "acc3": "Acc@3",
            "acc5": "Acc@5",
        },
        "methods": methods,
        "role_counts": dict(
            sorted(collections.Counter(row.get("role", "unknown") for row in rows).items())
        ),
        "adversarial_group_count": len(
            {row.get("adversarial_group", "unknown") for row in rows}
        ),
    }
    output_path = Path(ARGS.output).expanduser()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    if ARGS.markdown_output:
        Path(ARGS.markdown_output).expanduser().write_text(
            render_markdown(result),
            encoding="utf-8",
        )
    print(json.dumps(result, ensure_ascii=False, indent=2))
    if cleanup_dir and not ARGS.keep_isolated_copy:
        shutil.rmtree(cleanup_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
