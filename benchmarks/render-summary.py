#!/usr/bin/env python3
"""Render benchmark_results.json into a CI-friendly markdown summary."""

import argparse
import json
from pathlib import Path


def fmt_int(value):
    try:
        return f"{int(value):,}"
    except Exception:
        return str(value)


def fmt_pct(value):
    try:
        return f"{float(value):.1f}%"
    except Exception:
        return str(value)


def render_summary(data):
    project = data.get("project", "unknown")
    totals = data.get("totals", {})
    workflow_results = data.get("workflow_results", [])
    results = data.get("results", [])
    queue = data.get("queue_observability", {})

    lines = []
    a = lines.append

    a(f"# Token Efficiency Summary: {project}")
    a("")
    a("## Totals")
    a("")
    a("| Metric | Value |")
    a("|---|---:|")
    a(f"| Baseline tokens | {fmt_int(totals.get('baseline_tokens', 0))} |")
    a(f"| CodeLens tokens | {fmt_int(totals.get('codelens_tokens', 0))} |")
    a(f"| Savings | {fmt_pct(totals.get('savings_pct', 0))} |")

    if workflow_results:
        a("")
        a("## Profile / Composite Workflows")
        a("")
        a("| Scenario | Balanced Tokens | Profile Tokens | Savings | Calls | Low-level Chain | p95 Latency |")
        a("|---|---:|---:|---:|---:|---:|---:|")
        for result in workflow_results:
            baseline = result.get("baseline", {})
            compressed = result.get("compressed", {})
            a(
                f"| {result.get('scenario', 'unknown')} | "
                f"{fmt_int(baseline.get('total_tokens', 0))} | "
                f"{fmt_int(compressed.get('total_tokens', 0))} | "
                f"{fmt_pct(result.get('savings_pct', 0))} | "
                f"{baseline.get('tool_call_count', 0)} -> {compressed.get('tool_call_count', 0)} | "
                f"{baseline.get('low_level_chain_count', 0)} -> {compressed.get('low_level_chain_count', 0)} | "
                f"{baseline.get('p95_latency_ms', 0)}ms -> {compressed.get('p95_latency_ms', 0)}ms |"
            )

    if queue:
        a("")
        a("## Queue Observability")
        a("")
        if queue.get("supported"):
            session = queue.get("session", {})
            checks = queue.get("checks", {})
            a("| Metric | Value |")
            a("|---|---:|")
            a(f"| Jobs enqueued | {fmt_int(session.get('analysis_jobs_enqueued', 0))} |")
            a(f"| Jobs started | {fmt_int(session.get('analysis_jobs_started', 0))} |")
            a(f"| Jobs completed | {fmt_int(session.get('analysis_jobs_completed', 0))} |")
            a(f"| Jobs failed | {fmt_int(session.get('analysis_jobs_failed', 0))} |")
            a(f"| Jobs cancelled | {fmt_int(session.get('analysis_jobs_cancelled', 0))} |")
            a(f"| Queue depth (current) | {fmt_int(session.get('analysis_queue_depth', 0))} |")
            a(f"| Queue depth (max) | {fmt_int(session.get('analysis_queue_max_depth', 0))} |")
            a(f"| Active workers (peak) | {fmt_int(session.get('peak_active_analysis_workers', 0))} |")
            a(f"| Worker limit | {fmt_int(session.get('analysis_worker_limit', 0))} |")
            a(f"| Transport mode | {session.get('analysis_transport_mode', 'unknown')} |")
            a(f"| Observed queued state | {queue.get('saw_queued')} |")
            a(f"| Observed running state | {queue.get('saw_running')} |")
            a(
                f"| Queue gate | {'PASS' if queue.get('gate_passed') else 'FAIL'} |"
            )
            a(
                f"| Queue thresholds | depth>={checks.get('min_queue_depth', 0)}, "
                f"workers>={checks.get('min_peak_workers', 0)}, "
                f"success>={checks.get('min_queue_success_rate', 0.0):.2f}, "
                f"failures<={checks.get('max_queue_failures', 0)} |"
            )
            if queue.get("gate_failures"):
                a("")
                a("Queue gate failures:")
                for failure in queue["gate_failures"]:
                    a(f"- {failure}")
        else:
            a(f"- skipped: {queue.get('reason', 'unavailable')}")

    if results:
        a("")
        a("## Tool Benchmarks")
        a("")
        a("| Task | Baseline Tokens | CodeLens Tokens | Savings |")
        a("|---|---:|---:|---:|")
        for result in results:
            baseline_tokens = result.get("baseline_tokens", 0)
            codelens_tokens = result.get("codelens_tokens", 0)
            if baseline_tokens and codelens_tokens < baseline_tokens:
                savings = (1 - codelens_tokens / baseline_tokens) * 100
            elif baseline_tokens:
                savings = -((codelens_tokens / baseline_tokens) - 1) * 100
            else:
                savings = 0
            a(
                f"| {result.get('task', 'unknown')} | "
                f"{fmt_int(baseline_tokens)} | "
                f"{fmt_int(codelens_tokens)} | "
                f"{fmt_pct(savings)} |"
            )

    a("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--input",
        default="benchmarks/benchmark_results.json",
        help="Path to benchmark_results.json",
    )
    parser.add_argument(
        "--output",
        default="",
        help="Optional path to write markdown output",
    )
    args = parser.parse_args()

    data = json.loads(Path(args.input).read_text())
    markdown = render_summary(data)
    if args.output:
        Path(args.output).write_text(markdown)
    print(markdown)


if __name__ == "__main__":
    main()
