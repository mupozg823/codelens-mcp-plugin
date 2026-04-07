#!/usr/bin/env python3
"""Paper-facing benchmark aggregation for CodeLens harness + retrieval metrics."""

from __future__ import annotations

import argparse
import glob
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path


DEFAULT_HARNESS_REPORT_GLOB = str(Path.home() / ".codex" / "harness" / "reports" / "*.json")
DEFAULT_RETRIEVAL_REPORT = (
    Path(__file__).resolve().parent / "embedding-quality-results.json"
)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--harness-report", default="")
    parser.add_argument("--retrieval-report", default=str(DEFAULT_RETRIEVAL_REPORT))
    parser.add_argument("--mode", default="routed-on")
    parser.add_argument(
        "--source-kind",
        default="auto",
        choices=["auto", "real-session", "synthetic", "all"],
    )
    parser.add_argument("--repo", action="append", default=[])
    parser.add_argument("--task-kind", action="append", default=[])
    parser.add_argument("--agent", default="")
    parser.add_argument("--min-real-session-tasks", type=int, default=20)
    parser.add_argument("--min-real-session-scopes", type=int, default=3)
    parser.add_argument("--output-json", default="benchmarks/paper-benchmark-results.json")
    parser.add_argument("--output-md", default="benchmarks/paper-benchmark-summary.md")
    return parser.parse_args()


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def latest_harness_report() -> Path:
    candidates = [
        Path(path)
        for path in glob.glob(DEFAULT_HARNESS_REPORT_GLOB)
        if Path(path).is_file()
    ]
    if not candidates:
        raise SystemExit(
            "no harness report found; run benchmarks/harness-eval.py first or pass --harness-report"
        )
    return max(candidates, key=lambda path: path.stat().st_mtime)


def resolve_harness_report(path_arg: str) -> Path:
    if path_arg:
        path = Path(path_arg).expanduser().resolve()
        if not path.exists():
            raise SystemExit(f"harness report not found: {path}")
        return path
    return latest_harness_report().resolve()


def total_tokens(entry: dict) -> int:
    return int(entry.get("token_in") or 0) + int(entry.get("token_out") or 0)


def mean(values: list[float | int]) -> float | None:
    if not values:
        return None
    return sum(values) / len(values)


def filter_harness_entries(entries: list[dict], args) -> list[dict]:
    filtered = []
    requested_repos = set(args.repo)
    requested_tasks = set(args.task_kind)
    for entry in entries:
        if args.mode and entry.get("mode") != args.mode:
            continue
        if requested_repos:
            repo_values = {entry.get("repo"), entry.get("repo_id"), entry.get("repo_label")}
            if not any(value in requested_repos for value in repo_values if value):
                continue
        if requested_tasks and entry.get("task_kind") not in requested_tasks:
            continue
        if args.agent and entry.get("agent") != args.agent:
            continue
        filtered.append(entry)
    return filtered


def choose_source_kind(entries: list[dict], requested: str) -> tuple[str, list[dict], dict]:
    counts = Counter(entry.get("source_kind", "unknown") for entry in entries)
    if requested != "auto":
        if requested == "all":
            return "all", entries, dict(counts)
        selected = [entry for entry in entries if entry.get("source_kind") == requested]
        if not selected:
            raise SystemExit(f"no harness entries matched source_kind={requested}")
        return requested, selected, dict(counts)
    if counts.get("real-session"):
        return (
            "real-session",
            [entry for entry in entries if entry.get("source_kind") == "real-session"],
            dict(counts),
        )
    if counts.get("synthetic"):
        return (
            "synthetic",
            [entry for entry in entries if entry.get("source_kind") == "synthetic"],
            dict(counts),
        )
    if not entries:
        raise SystemExit("no harness entries matched the requested filters")
    return "all", entries, dict(counts)


def build_harness_metrics(entries: list[dict]) -> dict:
    measured = [entry for entry in entries if entry.get("success") is not None]
    successful = [entry for entry in measured if entry.get("success") is True]
    acceptance = [
        entry.get("acceptance_passed")
        for entry in entries
        if entry.get("acceptance_passed") is not None
    ]
    verify = [
        entry.get("verify_passed")
        for entry in entries
        if entry.get("verify_passed") is not None
    ]
    quality_scores = [
        float(entry["quality_score"])
        for entry in entries
        if entry.get("quality_score") is not None
    ]
    elapsed = [
        float(entry["elapsed_ms"])
        for entry in entries
        if entry.get("elapsed_ms") is not None
    ]
    success_elapsed = [
        float(entry["elapsed_ms"])
        for entry in successful
        if entry.get("elapsed_ms") is not None
    ]
    return {
        "entry_count": len(entries),
        "measured_task_count": len(measured),
        "successful_task_count": len(successful),
        "task_success_rate": (
            len(successful) / len(measured) if measured else None
        ),
        "tokens_per_successful_task": mean([total_tokens(entry) for entry in successful]),
        "latency_per_successful_task_ms": mean(success_elapsed),
        "avg_total_tokens": mean([total_tokens(entry) for entry in entries]),
        "avg_elapsed_ms": mean(elapsed),
        "avg_quality_score": mean(quality_scores),
        "acceptance_pass_rate": (
            sum(1 for value in acceptance if value) / len(acceptance)
            if acceptance
            else None
        ),
        "verify_pass_rate": (
            sum(1 for value in verify if value) / len(verify)
            if verify
            else None
        ),
    }


def cohort_scope_counts(entries: list[dict]) -> dict:
    repo_ids = {
        entry.get("repo_id") or entry.get("repo") or entry.get("repo_label")
        for entry in entries
        if entry.get("repo_id") or entry.get("repo") or entry.get("repo_label")
    }
    task_kinds = {
        entry.get("task_kind")
        for entry in entries
        if entry.get("task_kind")
    }
    return {
        "distinct_repo_count": len(repo_ids),
        "distinct_task_kind_count": len(task_kinds),
    }


def find_method(report: dict, method_name: str) -> dict:
    for method in report.get("methods", []):
        if method.get("method") == method_name:
            return method
    raise SystemExit(f"retrieval report missing method={method_name}")


def build_retrieval_metrics(report: dict) -> dict:
    ranked_context = find_method(report, "get_ranked_context")
    lexical = find_method(report, "get_ranked_context_no_semantic")
    ranking_cutoff = int(report.get("ranking_cutoff") or 10)
    return {
        "embedding_model": report.get("embedding_model"),
        "dataset_path": report.get("dataset_path"),
        "dataset_size": report.get("dataset_size"),
        "ranking_cutoff": ranking_cutoff,
        "ranked_context_mrr_at_k": ranked_context.get("mrr"),
        "ranked_context_acc1": ranked_context.get("acc1"),
        "ranked_context_acc3": ranked_context.get("acc3"),
        "ranked_context_acc5": ranked_context.get("acc5"),
        "ranked_context_avg_elapsed_ms": ranked_context.get("avg_elapsed_ms"),
        "lexical_ranked_context_mrr_at_k": lexical.get("mrr"),
        "hybrid_mrr_delta": (ranked_context.get("mrr") or 0.0) - (lexical.get("mrr") or 0.0),
        "hybrid_acc1_delta": (ranked_context.get("acc1") or 0.0) - (lexical.get("acc1") or 0.0),
    }


def render_markdown(result: dict) -> str:
    harness = result["harness_metrics"]
    retrieval = result["retrieval_metrics"]
    cohort = result["selected_cohort"]
    eligibility = result["promotion_eligibility"]
    lines = []
    a = lines.append
    a("# Paper Benchmark Summary")
    a("")
    a(f"- Harness report: `{result['inputs']['harness_report']}`")
    a(f"- Retrieval report: `{result['inputs']['retrieval_report']}`")
    a(f"- Primary mode: `{result['filters']['mode']}`")
    a(f"- Source kind: `{cohort['source_kind']}`")
    a("")
    a("## Headline Metrics")
    a("")
    a("| Metric | Value |")
    a("|---|---:|")
    a(
        f"| Task Success Rate | {harness['task_success_rate']:.1%} |"
        if harness["task_success_rate"] is not None
        else "| Task Success Rate | n/a |"
    )
    a(
        f"| Tokens per Successful Task | {harness['tokens_per_successful_task']:.1f} |"
        if harness["tokens_per_successful_task"] is not None
        else "| Tokens per Successful Task | n/a |"
    )
    a(
        f"| Latency per Successful Task (ms) | {harness['latency_per_successful_task_ms']:.1f} |"
        if harness["latency_per_successful_task_ms"] is not None
        else "| Latency per Successful Task (ms) | n/a |"
    )
    a(
        f"| get_ranked_context MRR@{retrieval['ranking_cutoff']} | {retrieval['ranked_context_mrr_at_k']:.3f} |"
    )
    a("")
    a("## Harness Cohort")
    a("")
    a("| Field | Value |")
    a("|---|---:|")
    a(f"| Entries after mode/filter | {cohort['filtered_entry_count']} |")
    a(f"| Selected entries | {cohort['selected_entry_count']} |")
    a(f"| Distinct repos | {cohort['distinct_repo_count']} |")
    a(f"| Distinct task kinds | {cohort['distinct_task_kind_count']} |")
    a(f"| Source breakdown | `{json.dumps(cohort['source_kind_counts'], ensure_ascii=False, sort_keys=True)}` |")
    a(f"| Successful tasks | {harness['successful_task_count']} |")
    a(f"| Acceptance pass rate | {harness['acceptance_pass_rate']:.1%} |" if harness["acceptance_pass_rate"] is not None else "| Acceptance pass rate | n/a |")
    a(f"| Verify pass rate | {harness['verify_pass_rate']:.1%} |" if harness["verify_pass_rate"] is not None else "| Verify pass rate | n/a |")
    a(f"| Avg quality score | {harness['avg_quality_score']:.3f} |" if harness["avg_quality_score"] is not None else "| Avg quality score | n/a |")
    a("")
    a("## Promotion Eligibility")
    a("")
    a("| Field | Value |")
    a("|---|---:|")
    a(f"| Promotion eligible | `{eligibility['promotion_eligible']}` |")
    a(f"| Real-session required | `{eligibility['requires_real_session']}` |")
    a(f"| Min measured tasks | {eligibility['minimum_real_session_tasks']} |")
    a(f"| Min distinct repos/task kinds | {eligibility['minimum_real_session_scopes']} |")
    if eligibility["failures"]:
        a("")
        a("Failures:")
        for failure in eligibility["failures"]:
            a(f"- {failure}")
        a("")
    a("## Retrieval Support")
    a("")
    a("| Metric | Value |")
    a("|---|---:|")
    a(f"| Embedding model | `{retrieval['embedding_model']}` |")
    a(f"| Dataset size | {retrieval['dataset_size']} |")
    a(f"| get_ranked_context MRR@{retrieval['ranking_cutoff']} | {retrieval['ranked_context_mrr_at_k']:.3f} |")
    a(f"| Lexical-only MRR@{retrieval['ranking_cutoff']} | {retrieval['lexical_ranked_context_mrr_at_k']:.3f} |")
    a(f"| Hybrid MRR delta | {retrieval['hybrid_mrr_delta']:+.3f} |")
    a(f"| Hybrid Acc@1 delta | {retrieval['hybrid_acc1_delta']:+.1%} |")
    a("")
    a("## Protocol")
    a("")
    a("- Main benchmark is harness task completion under `routed-on` mode.")
    a("- Real-session entries are preferred; synthetic entries are reported diagnostically when real-session data is absent.")
    a(
        f"- Retrieval support metric is `get_ranked_context MRR@{retrieval['ranking_cutoff']}` from the runtime benchmark."
    )
    a("- Token and latency metrics are reported per successful task, not per attempted task.")
    a("")
    return "\n".join(lines) + "\n"


def main():
    args = parse_args()
    harness_path = resolve_harness_report(args.harness_report)
    retrieval_path = Path(args.retrieval_report).expanduser().resolve()
    if not retrieval_path.exists():
        raise SystemExit(
            f"retrieval report not found: {retrieval_path}; run benchmarks/embedding-quality.py first"
        )

    harness_report = load_json(harness_path)
    retrieval_report = load_json(retrieval_path)
    filtered_entries = filter_harness_entries(harness_report.get("entries", []), args)
    selected_source_kind, selected_entries, source_kind_counts = choose_source_kind(
        filtered_entries, args.source_kind
    )

    harness_metrics = build_harness_metrics(selected_entries)
    retrieval_metrics = build_retrieval_metrics(retrieval_report)
    scope_counts = cohort_scope_counts(selected_entries)
    promotion_failures = []
    if selected_source_kind != "real-session":
        promotion_failures.append(
            f"selected cohort is {selected_source_kind}, not real-session"
        )
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
    result = {
        "schema_version": "codelens-paper-benchmark-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "inputs": {
            "harness_report": str(harness_path),
            "retrieval_report": str(retrieval_path),
        },
        "filters": {
            "mode": args.mode,
            "source_kind": args.source_kind,
            "repo": args.repo,
            "task_kind": args.task_kind,
            "agent": args.agent or None,
        },
        "selected_cohort": {
            "source_kind": selected_source_kind,
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
            "latency_per_successful_task_ms": harness_metrics[
                "latency_per_successful_task_ms"
            ],
            f"get_ranked_context_mrr_at_{retrieval_metrics['ranking_cutoff']}": retrieval_metrics[
                "ranked_context_mrr_at_k"
            ],
        },
    }

    output_json = Path(args.output_json).expanduser()
    output_md = Path(args.output_md).expanduser()
    output_json.write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    output_md.write_text(render_markdown(result), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
