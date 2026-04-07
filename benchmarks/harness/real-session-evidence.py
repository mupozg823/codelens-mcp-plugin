#!/usr/bin/env python3
"""Fail-closed real-session harness evidence for promotion decisions."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import subprocess
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
ROOT = BENCH_DIR.parent
DEFAULT_HARNESS_REPORT_DIR = Path.home() / ".codex" / "harness" / "reports"
DEFAULT_SESSION_GLOB = str(DEFAULT_HARNESS_REPORT_DIR / "session-entries" / "*.json")
DEFAULT_OUTPUT_JSON = BENCH_DIR / "real-session-evidence.json"
DEFAULT_OUTPUT_MD = BENCH_DIR / "real-session-evidence.md"
HARNESS_EVAL_SCRIPT = SCRIPT_DIR / "harness-eval.py"
PAPER_BENCHMARK_SCRIPT = BENCH_DIR / "paper-benchmark.py"
COVERAGE_GAP_QUEUE_SCRIPT = SCRIPT_DIR / "coverage-gap-queue.py"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


PAPER = load_module(PAPER_BENCHMARK_SCRIPT, "paper_benchmark_module")


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--harness-report", default="")
    parser.add_argument("--retrieval-report", required=True)
    parser.add_argument("--binary", default="")
    parser.add_argument("--mode", default="routed-on")
    parser.add_argument("--repo", action="append", default=[])
    parser.add_argument("--task-kind", action="append", default=[])
    parser.add_argument("--agent", default="")
    parser.add_argument("--min-real-session-tasks", type=int, default=20)
    parser.add_argument("--min-real-session-scopes", type=int, default=3)
    parser.add_argument("--session-entry-glob", action="append", default=[])
    parser.add_argument("--no-default-session-glob", action="store_true")
    parser.add_argument(
        "--no-refresh-existing-report",
        action="store_true",
        help="Use the provided harness report as-is even if fresher session entries exist.",
    )
    parser.add_argument("--output-json", default=str(DEFAULT_OUTPUT_JSON))
    parser.add_argument("--output-md", default=str(DEFAULT_OUTPUT_MD))
    return parser.parse_args()


def run(cmd: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=ROOT,
        env=env or os.environ.copy(),
        text=True,
        capture_output=True,
        check=False,
    )


def require_success(result: subprocess.CompletedProcess[str], name: str) -> None:
    if result.returncode == 0:
        return
    raise SystemExit(f"{name} failed\nstdout:\n{result.stdout}\n\nstderr:\n{result.stderr}")


def session_entry_paths(args) -> list[Path]:
    patterns = list(args.session_entry_glob)
    if not args.no_default_session_glob:
        patterns = [DEFAULT_SESSION_GLOB, *patterns] if patterns else [DEFAULT_SESSION_GLOB]
    if not patterns:
        return []
    common = load_module(SCRIPT_DIR / "harness_eval_common.py", "harness_eval_common_module")
    return common.resolve_session_entry_paths(patterns)


def latest_session_entry_mtime(args) -> float | None:
    paths = session_entry_paths(args)
    if not paths:
        return None
    return max(path.stat().st_mtime for path in paths if path.exists())


def report_has_real_sessions(path: Path) -> bool:
    if not path.exists():
        return False
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return False
    return any(
        entry.get("source_kind") == "real-session"
        for entry in report.get("entries", [])
    )


def should_refresh_report(path: Path | None, args) -> bool:
    if path is None or not path.exists():
        return True
    if args.no_refresh_existing_report:
        return False
    if not report_has_real_sessions(path):
        return True
    latest_session_mtime = latest_session_entry_mtime(args)
    if latest_session_mtime is None:
        return False
    return latest_session_mtime > path.stat().st_mtime


def run_harness_eval(args, output_json: Path, output_md: Path) -> Path:
    cmd = [
        "python3",
        str(HARNESS_EVAL_SCRIPT),
        "--skip-synthetic",
        "--output-json",
        str(output_json),
        "--output-md",
        str(output_md),
    ]
    if args.binary:
        cmd.extend(["--binary", args.binary])
    for repo in args.repo:
        cmd.extend(["--repo", repo])
    if args.no_default_session_glob:
        cmd.append("--no-default-session-glob")
    for pattern in args.session_entry_glob:
        cmd.extend(["--session-entry-glob", pattern])
    require_success(run(cmd), "harness-eval.py")
    return output_json


def build_real_session_result(
    harness_report: dict,
    retrieval_report: dict,
    args,
    harness_path: Path,
) -> dict:
    filtered_entries = PAPER.filter_harness_entries(harness_report.get("entries", []), args)
    selected_entries = [
        entry for entry in filtered_entries if entry.get("source_kind") == "real-session"
    ]
    source_kind_counts = dict(
        Counter(entry.get("source_kind", "unknown") for entry in filtered_entries)
    )
    harness_metrics = PAPER.build_harness_metrics(selected_entries)
    retrieval_metrics = PAPER.build_retrieval_metrics(retrieval_report)
    scope_counts = PAPER.cohort_scope_counts(selected_entries)

    promotion_failures = []
    if harness_metrics["measured_task_count"] < args.min_real_session_tasks:
        promotion_failures.append(
            "insufficient real-session measured tasks: "
            f"{harness_metrics['measured_task_count']} < {args.min_real_session_tasks}"
        )
    if (
        scope_counts["distinct_repo_count"] < args.min_real_session_scopes
        and scope_counts["distinct_task_kind_count"] < args.min_real_session_scopes
    ):
        promotion_failures.append(
            "insufficient real-session cohort diversity: "
            f"repos={scope_counts['distinct_repo_count']}, "
            f"task_kinds={scope_counts['distinct_task_kind_count']}, "
            f"need at least {args.min_real_session_scopes} in either dimension"
        )

    return {
        "schema_version": "codelens-real-session-evidence-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "inputs": {
            "harness_report": str(harness_path),
            "retrieval_report": str(Path(args.retrieval_report).expanduser().resolve()),
        },
        "filters": {
            "mode": args.mode,
            "source_kind": "real-session",
            "repo": args.repo,
            "task_kind": args.task_kind,
            "agent": args.agent or None,
        },
        "selected_cohort": {
            "source_kind": "real-session",
            "filtered_entry_count": len(filtered_entries),
            "selected_entry_count": len(selected_entries),
            "distinct_repo_count": scope_counts["distinct_repo_count"],
            "distinct_task_kind_count": scope_counts["distinct_task_kind_count"],
            "source_kind_counts": source_kind_counts,
        },
        "harness_metrics": harness_metrics,
        "retrieval_metrics": retrieval_metrics,
        "promotion_eligibility": {
            "requires_real_session": True,
            "minimum_real_session_tasks": args.min_real_session_tasks,
            "minimum_real_session_scopes": args.min_real_session_scopes,
            "promotion_eligible": not promotion_failures,
            "failures": promotion_failures,
        },
        "headline_metrics": {
            "task_success_rate": harness_metrics["task_success_rate"],
            "tokens_per_successful_task": harness_metrics["tokens_per_successful_task"],
            "latency_per_successful_task_ms": harness_metrics["latency_per_successful_task_ms"],
            f"get_ranked_context_mrr_at_{retrieval_metrics['ranking_cutoff']}": retrieval_metrics["ranked_context_mrr_at_k"],
        },
        "real_session_diagnostics": {
            "real_session_entry_count": sum(
                1
                for entry in harness_report.get("entries", [])
                if entry.get("source_kind") == "real-session"
            ),
            "synthetic_entry_count": sum(
                1
                for entry in harness_report.get("entries", [])
                if entry.get("source_kind") == "synthetic"
            ),
            "excluded_policy_real_session_count": harness_report.get(
                "excluded_policy_real_session_count"
            ),
            "duplicate_real_session_count": len(
                harness_report.get("duplicate_real_sessions", [])
            ),
        },
        "coverage_gap_queue": None,
    }


def attach_coverage_gap_queue(result: dict, output_json: Path) -> None:
    queue_json = output_json.parent / "coverage-gap-queue.json"
    queue_md = output_json.parent / "coverage-gap-queue.md"
    cmd = [
        "python3",
        str(COVERAGE_GAP_QUEUE_SCRIPT),
        "--output-json",
        str(queue_json),
        "--output-md",
        str(queue_md),
    ]
    queue_result = run(cmd)
    require_success(queue_result, "coverage-gap-queue.py")
    queue_payload = json.loads(queue_json.read_text(encoding="utf-8"))
    result["coverage_gap_queue"] = {
        "path_json": str(queue_json),
        "path_markdown": str(queue_md),
        "queue_count": len(queue_payload.get("queue", [])),
        "scenario_pack_json": queue_payload.get("scenario_pack_json"),
        "scenario_pack_markdown": queue_payload.get("scenario_pack_markdown"),
    }


def render_markdown(result: dict) -> str:
    markdown = PAPER.render_markdown(result)
    queue = result.get("coverage_gap_queue")
    if not queue:
        return markdown
    lines = markdown.rstrip().splitlines()
    lines.extend(
        [
            "",
            "## Coverage Gap Queue",
            "",
            f"- Queue JSON: `{queue['path_json']}`",
            f"- Queue Markdown: `{queue['path_markdown']}`",
            f"- Queue count: `{queue['queue_count']}`",
            f"- Scenario pack JSON: `{queue['scenario_pack_json']}`",
            f"- Scenario pack Markdown: `{queue['scenario_pack_markdown']}`",
            "",
        ]
    )
    return "\n".join(lines) + "\n"


def main():
    args = parse_args()
    output_json = Path(args.output_json).expanduser().resolve()
    output_md = Path(args.output_md).expanduser().resolve()
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    retrieval_path = Path(args.retrieval_report).expanduser().resolve()
    if not retrieval_path.exists():
        raise SystemExit(f"retrieval report not found: {retrieval_path}")

    harness_report_path = (
        Path(args.harness_report).expanduser().resolve()
        if args.harness_report
        else None
    )
    if should_refresh_report(harness_report_path, args):
        harness_report_path = run_harness_eval(
            args,
            output_json.parent / "harness-eval-real-session.json",
            output_json.parent / "harness-eval-real-session.md",
        )
    assert harness_report_path is not None

    harness_report = json.loads(harness_report_path.read_text(encoding="utf-8"))
    retrieval_report = json.loads(retrieval_path.read_text(encoding="utf-8"))
    result = build_real_session_result(
        harness_report,
        retrieval_report,
        args,
        harness_report_path,
    )
    if not result["promotion_eligibility"]["promotion_eligible"]:
        attach_coverage_gap_queue(result, output_json)

    output_json.write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    output_md.write_text(render_markdown(result), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
