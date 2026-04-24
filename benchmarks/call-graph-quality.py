#!/usr/bin/env python3
"""Call-graph quality benchmark for release-vs-candidate validation.

The benchmark intentionally stays thin: it invokes the existing CodeLens CLI
surface, scores returned edges, and reports confidence honesty failures.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from benchmark_runtime_common import parse_output_json, percentile_95, tool_payload_succeeded


DEFAULT_BINARY = ROOT / "target" / "release" / "codelens-mcp"
DEFAULT_DATASET = SCRIPT_DIR / "call-graph-quality-dataset.json"
HONESTY_LIMITS = {
    "unresolved": 0.25,
    "path_proximity": 0.60,
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY)))
    parser.add_argument("--dataset", default=str(DEFAULT_DATASET))
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--output", default=str(SCRIPT_DIR / "call-graph-quality-results.json"))
    parser.add_argument("--markdown-output", default="")
    parser.add_argument("--timeout-seconds", type=int, default=240)
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    parser.add_argument(
        "--require-all-paths",
        action="store_true",
        help="Fail when any dataset repo path is unavailable.",
    )
    return parser.parse_args()


def load_dataset(path: Path) -> dict:
    dataset = json.loads(path.read_text(encoding="utf-8"))
    rows = dataset.get("rows")
    if not isinstance(rows, list) or not rows:
        raise SystemExit(f"call-graph dataset has no rows: {path}")
    return dataset


def resolve_project_path(path: str) -> Path:
    candidate = Path(path).expanduser()
    if candidate.is_absolute():
        return candidate.resolve()
    return (Path.cwd() / candidate).resolve()


def remove_relative_path(root: Path, relative_path: str) -> None:
    target = (root / relative_path).resolve()
    try:
        target.relative_to(root.resolve())
    except ValueError as exc:
        raise SystemExit(f"ignore_paths entry escapes benchmark root: {relative_path}") from exc
    if not target.exists():
        return
    if target.is_dir():
        shutil.rmtree(target)
    else:
        target.unlink()


def copy_project_for_benchmark(source_project: Path, ignore_paths: list[str]) -> Path:
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-callgraph-"))
    benchmark_project = temp_root / source_project.name
    shutil.copytree(
        source_project,
        benchmark_project,
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
    for relative_path in ignore_paths:
        remove_relative_path(benchmark_project, relative_path)
    return benchmark_project


def data_payload(payload: dict | None) -> dict:
    if not isinstance(payload, dict):
        return {}
    data = payload.get("data")
    return data if isinstance(data, dict) else payload


def edge_name(edge: dict) -> str:
    for key in ("name", "function", "function_name", "callee_name"):
        value = edge.get(key)
        if isinstance(value, str):
            return value
    return ""


def edge_file(edge: dict) -> str:
    for key in ("file", "resolved_file", "file_path", "path"):
        value = edge.get(key)
        if isinstance(value, str):
            return value
    return ""


def edge_confidence(edge: dict) -> float:
    value = edge.get("confidence", 0.0)
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0


def edge_resolution(edge: dict) -> str:
    for key in ("resolution", "resolution_strategy"):
        value = edge.get(key)
        if isinstance(value, str):
            return value
    return ""


def extract_edges(tool: str, payload: dict | None) -> list[dict]:
    data = data_payload(payload)
    key = "callers" if tool == "get_callers" else "callees"
    rows = data.get(key, [])
    if not isinstance(rows, list):
        return []
    return [row for row in rows if isinstance(row, dict)]


def matches_edge(spec: dict, edge: dict) -> bool:
    expected_name = spec.get("name")
    if expected_name and edge_name(edge) != expected_name:
        return False
    file_value = edge_file(edge)
    file_suffix = spec.get("file_suffix")
    if file_suffix and not file_value.endswith(str(file_suffix)):
        return False
    file_contains = spec.get("resolved_file_contains") or spec.get("file_contains")
    if file_contains and str(file_contains) not in file_value:
        return False
    return True


def score_expected_edges(expected_edges: list[dict], edges: list[dict]) -> dict:
    found = []
    missing = []
    first_rank = None
    for expected in expected_edges:
        matched_rank = None
        matched_edge = None
        for index, edge in enumerate(edges, start=1):
            if not matches_edge(expected, edge):
                continue
            min_confidence = expected.get("min_confidence")
            if min_confidence is not None and edge_confidence(edge) < float(min_confidence):
                continue
            matched_rank = index
            matched_edge = edge
            break
        if matched_rank is None:
            missing.append(expected)
            continue
        found.append(
            {
                "expected": expected,
                "rank": matched_rank,
                "edge": compact_edge(matched_edge or {}),
            }
        )
        first_rank = matched_rank if first_rank is None else min(first_rank, matched_rank)
    total = len(expected_edges)
    return {
        "expected_total": total,
        "expected_found_count": len(found),
        "edge_recall_at_k": (len(found) / total) if total else None,
        "first_expected_rank": first_rank,
        "mrr_first_expected_edge": (1.0 / first_rank) if first_rank else (None if not total else 0.0),
        "expected_found": found,
        "expected_missing": missing,
    }


def compact_edge(edge: dict) -> dict:
    return {
        "name": edge_name(edge),
        "file": edge_file(edge),
        "line": edge.get("line") or edge.get("resolved_line"),
        "confidence": edge_confidence(edge),
        "resolution": edge_resolution(edge),
    }


def confidence_honesty_failures(edges: list[dict]) -> list[dict]:
    failures = []
    for index, edge in enumerate(edges, start=1):
        resolution = edge_resolution(edge)
        limit = HONESTY_LIMITS.get(resolution)
        confidence = edge_confidence(edge)
        if limit is None or confidence <= limit:
            continue
        failures.append(
            {
                "rank": index,
                "edge": compact_edge(edge),
                "reason": f"{resolution} confidence {confidence:.3f} exceeds {limit:.2f}",
            }
        )
    return failures


def forbidden_high_confidence_failures(row: dict, edges: list[dict]) -> list[dict]:
    failures = []
    for spec in row.get("forbidden_high_confidence_edges", []) or []:
        threshold = float(spec.get("min_confidence", 0.61))
        for index, edge in enumerate(edges, start=1):
            if not matches_edge(spec, edge):
                continue
            confidence = edge_confidence(edge)
            if confidence < threshold:
                continue
            failures.append(
                {
                    "rank": index,
                    "forbidden": spec,
                    "edge": compact_edge(edge),
                    "reason": f"forbidden edge reported at confidence {confidence:.3f}",
                }
            )
    return failures


def edge_rates(edges: list[dict]) -> dict:
    total = len(edges)
    if not total:
        return {
            "unresolved_rate": 0.0,
            "fallback_rate": 0.0,
        }
    unresolved = sum(1 for edge in edges if edge_resolution(edge) == "unresolved")
    fallback = sum(
        1
        for edge in edges
        if edge_resolution(edge) in {"unresolved", "path_proximity", "fallback_name_match"}
    )
    return {
        "unresolved_rate": unresolved / total,
        "fallback_rate": fallback / total,
    }


def run_tool(binary: Path, project: Path, row: dict, preset: str, timeout_seconds: int) -> dict:
    args = {
        "function_name": row["function_name"],
        "max_results": int(row.get("max_results", 20)),
    }
    if row.get("file_path"):
        args["file_path"] = row["file_path"]
    argv = [
        str(binary),
        str(project),
        "--preset",
        preset,
        "--cmd",
        row["tool"],
        "--args",
        json.dumps(args),
    ]
    started = time.perf_counter()
    result = subprocess.run(
        argv,
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
    )
    elapsed_ms = round((time.perf_counter() - started) * 1000, 2)
    payload = parse_output_json(result.stdout)
    return {
        "argv": argv,
        "elapsed_ms": elapsed_ms,
        "returncode": result.returncode,
        "payload": payload,
        "stderr": result.stderr.strip(),
    }


def evaluate_row(row: dict, tool_result: dict) -> dict:
    payload = tool_result.get("payload")
    edges = extract_edges(row["tool"], payload)
    expected_score = score_expected_edges(row.get("expected_edges", []) or [], edges)
    honesty_failures = confidence_honesty_failures(edges)
    forbidden_failures = forbidden_high_confidence_failures(row, edges)
    rates = edge_rates(edges)
    tool_ok = tool_result.get("returncode") == 0 and tool_payload_succeeded(payload)
    status = "passed"
    if not tool_ok:
        status = "tool_failed"
    elif honesty_failures or forbidden_failures or expected_score["expected_missing"]:
        status = "failed"
    return {
        "id": row.get("id"),
        "repo_id": row.get("repo_id"),
        "tool": row["tool"],
        "function_name": row["function_name"],
        "file_path": row.get("file_path"),
        "benchmark_project": row.get("_benchmark_project"),
        "ignore_paths": row.get("ignore_paths", []),
        "status": status,
        "elapsed_ms": tool_result.get("elapsed_ms", 0),
        "returncode": tool_result.get("returncode"),
        "edge_count": len(edges),
        "top_edges": [compact_edge(edge) for edge in edges[:10]],
        "stderr": tool_result.get("stderr", ""),
        **expected_score,
        **rates,
        "confidence_honesty_failures": honesty_failures,
        "forbidden_high_confidence_failures": forbidden_failures,
    }


def skipped_row(row: dict, project_path: Path) -> dict:
    return {
        "id": row.get("id"),
        "repo_id": row.get("repo_id"),
        "tool": row.get("tool"),
        "function_name": row.get("function_name"),
        "status": "skipped",
        "skip_reason": f"missing repo path: {project_path}",
        "elapsed_ms": 0,
        "edge_count": 0,
        "expected_total": len(row.get("expected_edges", []) or []),
        "expected_found_count": 0,
        "edge_recall_at_k": None,
        "first_expected_rank": None,
        "mrr_first_expected_edge": None,
        "expected_found": [],
        "expected_missing": row.get("expected_edges", []) or [],
        "unresolved_rate": 0.0,
        "fallback_rate": 0.0,
        "confidence_honesty_failures": [],
        "forbidden_high_confidence_failures": [],
    }


def aggregate_results(dataset: dict, rows: list[dict]) -> dict:
    measured = [row for row in rows if row.get("status") != "skipped"]
    elapsed = [float(row.get("elapsed_ms", 0)) for row in measured]
    expected_total = sum(int(row.get("expected_total") or 0) for row in measured)
    expected_found = sum(int(row.get("expected_found_count") or 0) for row in measured)
    mrr_values = [
        float(row["mrr_first_expected_edge"])
        for row in measured
        if row.get("mrr_first_expected_edge") is not None
    ]
    edge_total = sum(int(row.get("edge_count") or 0) for row in measured)
    unresolved_weighted = sum(
        float(row.get("unresolved_rate", 0.0)) * int(row.get("edge_count") or 0)
        for row in measured
    )
    fallback_weighted = sum(
        float(row.get("fallback_rate", 0.0)) * int(row.get("edge_count") or 0)
        for row in measured
    )
    honesty_count = sum(len(row.get("confidence_honesty_failures", [])) for row in measured)
    forbidden_count = sum(
        len(row.get("forbidden_high_confidence_failures", [])) for row in measured
    )
    failed_rows = [row["id"] for row in measured if row.get("status") in {"failed", "tool_failed"}]
    minimum_available_rows = int(dataset.get("minimum_available_rows", 1))
    return {
        "configured_row_count": len(rows),
        "available_row_count": len(measured),
        "skipped_row_count": len(rows) - len(measured),
        "minimum_available_rows": minimum_available_rows,
        "edge_recall_at_k": (expected_found / expected_total) if expected_total else None,
        "mrr_first_expected_edge": (
            sum(mrr_values) / len(mrr_values) if mrr_values else None
        ),
        "avg_elapsed_ms": (sum(elapsed) / len(elapsed) if elapsed else None),
        "p95_elapsed_ms": percentile_95([int(value) for value in elapsed]) if elapsed else None,
        "unresolved_rate": unresolved_weighted / edge_total if edge_total else 0.0,
        "fallback_rate": fallback_weighted / edge_total if edge_total else 0.0,
        "confidence_honesty_failure_count": honesty_count,
        "forbidden_high_confidence_failure_count": forbidden_count,
        "failed_row_ids": failed_rows,
        "quality_gate_passed": (
            len(measured) >= minimum_available_rows
            and not failed_rows
            and honesty_count == 0
            and forbidden_count == 0
        ),
    }


def render_markdown(report: dict) -> str:
    metrics = report["metrics"]
    lines = [
        "# Call-Graph Quality Benchmark",
        "",
        f"- Binary: `{report['binary']}`",
        f"- Dataset: `{report['dataset_path']}`",
        f"- Quality gate passed: `{metrics['quality_gate_passed']}`",
        f"- Rows: `{metrics['available_row_count']} / {metrics['configured_row_count']}`",
        "",
        "## Metrics",
        "",
        "| Metric | Value |",
        "| --- | ---: |",
        f"| edge_recall_at_k | {format_metric(metrics.get('edge_recall_at_k'))} |",
        f"| mrr_first_expected_edge | {format_metric(metrics.get('mrr_first_expected_edge'))} |",
        f"| avg_elapsed_ms | {format_metric(metrics.get('avg_elapsed_ms'))} |",
        f"| p95_elapsed_ms | {format_metric(metrics.get('p95_elapsed_ms'))} |",
        f"| unresolved_rate | {format_metric(metrics.get('unresolved_rate'))} |",
        f"| fallback_rate | {format_metric(metrics.get('fallback_rate'))} |",
        f"| confidence_honesty_failure_count | {metrics.get('confidence_honesty_failure_count', 0)} |",
        f"| forbidden_high_confidence_failure_count | {metrics.get('forbidden_high_confidence_failure_count', 0)} |",
        "",
        "## Rows",
        "",
        "| Row | Status | Recall | MRR | Top edge |",
        "| --- | --- | ---: | ---: | --- |",
    ]
    for row in report.get("rows", []):
        top = row.get("top_edges", [{}])[0] if row.get("top_edges") else {}
        top_label = ""
        if top:
            top_label = f"{top.get('name')} / {top.get('resolution')} / {top.get('confidence'):.2f}"
        lines.append(
            f"| {row.get('id')} | {row.get('status')} | "
            f"{format_metric(row.get('edge_recall_at_k'))} | "
            f"{format_metric(row.get('mrr_first_expected_edge'))} | {top_label or '-'} |"
        )
    return "\n".join(lines) + "\n"


def format_metric(value: Any) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.3f}"
    return str(value)


def main() -> None:
    args = parse_args()
    binary = Path(args.binary).expanduser().resolve()
    dataset_path = Path(args.dataset).expanduser().resolve()
    dataset = load_dataset(dataset_path)
    rows = []
    missing_paths = []
    project_cache: dict[tuple[str, tuple[str, ...]], Path] = {}
    cleanup_dirs: list[Path] = []
    for row in dataset["rows"]:
        source_project = resolve_project_path(row["path"])
        if not source_project.exists():
            missing_paths.append(str(source_project))
            rows.append(skipped_row(row, source_project))
            continue
        ignore_paths = [str(path) for path in row.get("ignore_paths", [])]
        cache_key = (str(source_project), tuple(ignore_paths))
        project_path = source_project
        if args.isolated_copy or ignore_paths:
            if cache_key not in project_cache:
                project_cache[cache_key] = copy_project_for_benchmark(source_project, ignore_paths)
                cleanup_dirs.append(project_cache[cache_key].parent)
            project_path = project_cache[cache_key]
        row = dict(row)
        row["_benchmark_project"] = str(project_path)
        result = run_tool(
            binary,
            project_path,
            row,
            preset=args.preset,
            timeout_seconds=args.timeout_seconds,
        )
        rows.append(evaluate_row(row, result))

    if args.require_all_paths and missing_paths:
        raise SystemExit("missing call-graph dataset paths:\n" + "\n".join(missing_paths))

    report = {
        "schema_version": "codelens-call-graph-quality-result-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "binary": str(binary),
        "dataset_path": str(dataset_path),
        "preset": args.preset,
        "metrics": aggregate_results(dataset, rows),
        "rows": rows,
    }
    output_path = Path(args.output).expanduser()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    if args.markdown_output:
        markdown_path = Path(args.markdown_output).expanduser()
        markdown_path.parent.mkdir(parents=True, exist_ok=True)
        markdown_path.write_text(render_markdown(report), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if not args.keep_isolated_copy:
        for temp_root in cleanup_dirs:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    main()
