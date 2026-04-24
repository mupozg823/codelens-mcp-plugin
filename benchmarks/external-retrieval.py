#!/usr/bin/env python3
"""Exact-label retrieval benchmark on disjoint external repositories."""

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
DEFAULT_DATASET = Path(__file__).resolve().parent / "external-retrieval-dataset.json"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary",
        default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY)),
    )
    parser.add_argument("--dataset", default=str(DEFAULT_DATASET))
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--output", default="benchmarks/external-retrieval-results.json")
    parser.add_argument("--markdown-output", default="")
    parser.add_argument("--max-results", type=int, default=10)
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    return parser.parse_args()


ARGS = parse_args()
BIN = str(Path(ARGS.binary).expanduser().resolve())
DATASET_PATH = Path(ARGS.dataset).expanduser().resolve()


def compute_file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def resolve_runtime_model_dir() -> Path:
    model_dir = resolve_codelens_model_dir(
        BIN,
        env=os.environ,
        repo_root=Path(__file__).resolve().parent.parent,
        allow_builtin_override=False,
    )
    if model_dir is not None:
        return model_dir
    raise SystemExit(
        "Cannot resolve runtime model dir. Checked CODELENS_MODEL_DIR, executable models, "
        "user cache, and repo model roots."
    )


def snapshot_runtime_model() -> dict:
    model_dir = resolve_runtime_model_dir()
    model_path = model_dir / "model.onnx"
    config_path = model_dir / "config.json"
    config = json.loads(config_path.read_text(encoding="utf-8")) if config_path.exists() else {}
    return {
        "model_dir": str(model_dir),
        "model_path": str(model_path),
        "config_path": str(config_path),
        "sha256": compute_file_sha256(model_path),
        "size_bytes": model_path.stat().st_size,
        "num_hidden_layers": config.get("num_hidden_layers"),
        "hidden_size": config.get("hidden_size"),
    }


def load_dataset():
    raw = json.loads(DATASET_PATH.read_text(encoding="utf-8"))
    repos = list(raw.get("repos", []))
    if not repos:
        raise SystemExit(f"external dataset has no repos: {DATASET_PATH}")
    return raw, repos


def run_tool(project: str, cmd: str, arguments: dict, timeout: int = 240):
    argv = [
        BIN,
        project,
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


def remove_relative_path(root: Path, relative_path: str) -> None:
    target = (root / relative_path).resolve()
    try:
        target.relative_to(root.resolve())
    except ValueError as exc:
        raise SystemExit(
            f"ignore_paths entry escapes benchmark root: {relative_path}"
        ) from exc
    if not target.exists():
        return
    if target.is_dir():
        shutil.rmtree(target)
    else:
        target.unlink()


def copy_project_for_benchmark(
    source_project: str, ignore_paths: list[str] | None = None
) -> str:
    source = Path(source_project).resolve()
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-external-retrieval-"))
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
    for relative_path in ignore_paths or []:
        remove_relative_path(bench_project, relative_path)
    return str(bench_project)


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
    by_repo = {}
    by_type_groups = collections.defaultdict(list)
    by_repo_groups = collections.defaultdict(list)
    for row in rows:
        by_type_groups[row["query_type"]].append(row)
        by_repo_groups[row["repo_id"]].append(row)

    for query_type, group in sorted(by_type_groups.items()):
        type_total = len(group)
        by_query_type[query_type] = {
            "count": type_total,
            "mrr": sum(mrr_component(row["rank"]) for row in group) / type_total,
            "acc1": sum(acc_at(row["rank"], 1) for row in group) / type_total,
            "acc3": sum(acc_at(row["rank"], 3) for row in group) / type_total,
            "acc5": sum(acc_at(row["rank"], 5) for row in group) / type_total,
            "avg_elapsed_ms": sum(row["elapsed_ms"] for row in group) / type_total,
        }
    for repo_id, group in sorted(by_repo_groups.items()):
        repo_total = len(group)
        by_repo[repo_id] = {
            "count": repo_total,
            "mrr": sum(mrr_component(row["rank"]) for row in group) / repo_total,
            "acc1": sum(acc_at(row["rank"], 1) for row in group) / repo_total,
            "acc3": sum(acc_at(row["rank"], 3) for row in group) / repo_total,
            "acc5": sum(acc_at(row["rank"], 5) for row in group) / repo_total,
            "avg_elapsed_ms": sum(row["elapsed_ms"] for row in group) / repo_total,
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
        "by_repo": by_repo,
        "rows": rows,
    }


def evaluate_method(name: str, repo_rows: list[dict], tool_name: str, args_factory):
    rows = []
    for repo_row in repo_rows:
        tool_result = require_tool_success(
            tool_name,
            run_tool(repo_row["project"], tool_name, args_factory(repo_row), timeout=240),
            context=f"{repo_row['repo_id']}::{repo_row['query']}",
        )
        payload = tool_result.get("payload") or {}
        candidates = candidate_rows(name, payload)
        rank = find_rank(
            repo_row["expected_symbol"],
            repo_row["expected_file_suffix"],
            candidates,
        )
        rows.append(
            {
                "repo_id": repo_row["repo_id"],
                "repo_label": repo_row["repo_label"],
                "query": repo_row["query"],
                "query_type": repo_row["query_type"],
                "expected_symbol": repo_row["expected_symbol"],
                "expected_file_suffix": repo_row["expected_file_suffix"],
                "rank": rank,
                "elapsed_ms": tool_result["elapsed_ms"],
                "candidate_count": len(candidates),
                "top_candidate": candidates[0] if candidates else None,
            }
        )
    summary = aggregate_rows(rows)
    summary["method"] = name
    return summary


def render_markdown(result: dict) -> str:
    lines = []
    a = lines.append
    runtime_model = result.get("runtime_model") or {}
    a("# External Retrieval Summary")
    a("")
    a(f"- Binary: `{result['binary']}`")
    if runtime_model:
        a(
            f"- Runtime model: `{runtime_model.get('num_hidden_layers', '?')}L`, "
            f"`{runtime_model.get('size_bytes', 0) // (1024 * 1024)}MB`, "
            f"`sha256:{str(runtime_model.get('sha256', ''))[:16]}`"
        )
        a(f"- Runtime model path: `{runtime_model.get('model_path')}`")
    a(f"- Dataset: `{result['dataset_path']}`")
    a(f"- Available repos: {result['available_repo_count']} / {result['configured_repo_count']}")
    a(f"- Evidence sufficient: `{result['sufficient_evidence']}`")
    a(f"- Ranking cutoff: top-{result['ranking_cutoff']}")
    a("")
    a("## Metrics")
    a("")
    a(f"| Method | MRR@{result['ranking_cutoff']} | Acc@1 | Acc@3 | Acc@5 | Avg ms |")
    a("|---|---:|---:|---:|---:|---:|")
    for method in result["methods"]:
        if method["mrr"] is None:
            a(f"| {method['method']} | n/a | n/a | n/a | n/a | n/a |")
            continue
        a(
            f"| {method['method']} | {method['mrr']:.3f} | {method['acc1']:.0%} | {method['acc3']:.0%} | {method['acc5']:.0%} | {method['avg_elapsed_ms']:.1f} |"
        )
    a("")
    a("## Repo Evidence")
    a("")
    a("| Repo | Exists | Queries | Indexed symbols | Isolation | Ignored paths |")
    a("|---|---|---:|---:|---|---|")
    for repo in result["repos"]:
        a(
            f"| {repo['repo_id']} | {repo['path_exists']} | {repo['query_count']} | {repo.get('indexed_symbols', 0)} | {repo['isolated_project'] or 'no'} | {', '.join(repo.get('ignore_paths', [])) or '—'} |"
        )
    a("")
    return "\n".join(lines) + "\n"


def main():
    dataset_meta, repos = load_dataset()
    runtime_model = snapshot_runtime_model()
    available_repo_rows = []
    repo_reports = []
    cleanup_dirs: list[Path] = []
    for repo in repos:
        source_path = Path(repo["path"]).expanduser()
        ignore_paths = [str(path) for path in repo.get("ignore_paths", [])]
        path_exists = source_path.exists()
        project_path = str(source_path.resolve()) if path_exists else None
        isolated_project = None
        indexed_symbols = 0
        index_elapsed_ms = None
        if path_exists and (ARGS.isolated_copy or ignore_paths):
            isolated_project = copy_project_for_benchmark(
                str(source_path.resolve()), ignore_paths=ignore_paths
            )
            cleanup_dirs.append(Path(isolated_project).parent)
            project_path = isolated_project
        if path_exists and project_path:
            index_result = require_tool_success(
                "index_embeddings",
                run_tool(project_path, "index_embeddings", {}, timeout=600),
                context=repo["repo_id"],
            )
            validate_expected_file_suffixes(
                repo.get("queries", []),
                DATASET_PATH,
                lambda _row, project_path=project_path: project_path,
                row_label=lambda row, repo_id=repo["repo_id"]: f"{repo_id}::{row.get('query') or row.get('expected_symbol')}",
            )
            index_payload = index_result["payload"] or {}
            indexed_symbols = (
                index_payload.get("data", {}).get("indexed_symbols") or 0
            )
            index_elapsed_ms = index_result["elapsed_ms"]
            for item in repo.get("queries", []):
                available_repo_rows.append(
                    {
                        "repo_id": repo["repo_id"],
                        "repo_label": repo.get("label", repo["repo_id"]),
                        "project": project_path,
                        "query": item["query"],
                        "query_type": item.get("query_type", "uncategorized"),
                        "expected_symbol": item["expected_symbol"],
                        "expected_file_suffix": item.get("expected_file_suffix", ""),
                    }
                )
        repo_reports.append(
            {
                "repo_id": repo["repo_id"],
                "label": repo.get("label", repo["repo_id"]),
                "path": str(source_path),
                "path_exists": path_exists,
                "query_count": len(repo.get("queries", [])),
                "indexed_symbols": indexed_symbols,
                "index_elapsed_ms": index_elapsed_ms,
                "isolated_project": isolated_project,
                "ignore_paths": ignore_paths,
            }
        )

    methods = [
        evaluate_method(
            "semantic_search",
            available_repo_rows,
            "semantic_search",
            lambda item: {"query": item["query"], "max_results": ARGS.max_results},
        ),
        evaluate_method(
            "get_ranked_context",
            available_repo_rows,
            "get_ranked_context",
            lambda item: {
                "query": item["query"],
                "max_tokens": 1200,
                "include_body": False,
            },
        ),
        evaluate_method(
            "get_ranked_context_no_semantic",
            available_repo_rows,
            "get_ranked_context",
            lambda item: {
                "query": item["query"],
                "max_tokens": 1200,
                "include_body": False,
                "disable_semantic": True,
            },
        ),
    ]
    available_repo_count = sum(1 for repo in repo_reports if repo["path_exists"])
    minimum_repo_count = int(dataset_meta.get("minimum_repo_count", 2))
    minimum_queries_per_repo = int(dataset_meta.get("minimum_queries_per_repo", 10))
    sufficient_evidence = (
        available_repo_count >= minimum_repo_count
        and all(
            repo["query_count"] >= minimum_queries_per_repo
            for repo in repo_reports
            if repo["path_exists"]
        )
    )
    result = {
        "schema_version": "codelens-external-retrieval-v1",
        "binary": BIN,
        "runtime_model": runtime_model,
        "dataset_path": str(DATASET_PATH),
        "configured_repo_count": len(repo_reports),
        "available_repo_count": available_repo_count,
        "available_query_count": len(available_repo_rows),
        "ranking_cutoff": ARGS.max_results,
        "metric_labels": {
            "mrr": f"MRR@{ARGS.max_results}",
            "acc1": "Acc@1",
            "acc3": "Acc@3",
            "acc5": "Acc@5",
        },
        "minimum_repo_count": minimum_repo_count,
        "minimum_queries_per_repo": minimum_queries_per_repo,
        "sufficient_evidence": sufficient_evidence,
        "repos": repo_reports,
        "methods": methods,
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
    if not ARGS.keep_isolated_copy:
        for temp_root in cleanup_dirs:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    main()
