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
    quality_contract = data.get("quality_contract", {})
    regression_results = []
    if workflow_results:
        workflow_savings = [float(result.get("savings_pct", 0)) for result in workflow_results]
        total_baseline_calls = sum(
            int(result.get("baseline", {}).get("tool_call_count", 0))
            for result in workflow_results
        )
        total_profile_calls = sum(
            int(result.get("compressed", {}).get("tool_call_count", 0))
            for result in workflow_results
        )
        total_baseline_chain = sum(
            int(result.get("baseline", {}).get("low_level_chain_count", 0))
            for result in workflow_results
        )
        total_profile_chain = sum(
            int(result.get("compressed", {}).get("low_level_chain_count", 0))
            for result in workflow_results
        )
        avg_workflow_savings = sum(workflow_savings) / len(workflow_savings)
        best_workflow = max(workflow_results, key=lambda result: result.get("savings_pct", 0))
        slowest_profile = max(
            workflow_results,
            key=lambda result: result.get("compressed", {}).get("p95_latency_ms", 0),
        )
    else:
        workflow_savings = []
        total_baseline_calls = 0
        total_profile_calls = 0
        total_baseline_chain = 0
        total_profile_chain = 0
        avg_workflow_savings = 0.0
        best_workflow = None
        slowest_profile = None

    for result in results:
        baseline_tokens = result.get("baseline_tokens", 0)
        codelens_tokens = result.get("codelens_tokens", 0)
        if baseline_tokens and codelens_tokens >= baseline_tokens:
            regression_results.append(result)

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
        a("## Headline Deltas")
        a("")
        a("| KPI | Value |")
        a("|---|---:|")
        a(f"| Avg workflow savings | {fmt_pct(avg_workflow_savings)} |")
        a(f"| Workflow call reduction | {fmt_int(total_baseline_calls)} -> {fmt_int(total_profile_calls)} |")
        a(f"| Low-level chain reduction | {fmt_int(total_baseline_chain)} -> {fmt_int(total_profile_chain)} |")
        if best_workflow:
            a(
                f"| Best workflow | {best_workflow.get('scenario', 'unknown')} ({fmt_pct(best_workflow.get('savings_pct', 0))}) |"
            )
        if slowest_profile:
            a(
                f"| Slowest compressed workflow | {slowest_profile.get('scenario', 'unknown')} ({fmt_int(slowest_profile.get('compressed', {}).get('p95_latency_ms', 0))}ms p95) |"
            )
        if regression_results:
            a(f"| Point-lookups still worse | {fmt_int(len(regression_results))} |")

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
            a(f"| Queue weighted depth (current) | {fmt_int(session.get('analysis_queue_weighted_depth', 0))} |")
            a(f"| Queue weighted depth (max) | {fmt_int(session.get('analysis_queue_max_weighted_depth', 0))} |")
            a(f"| Queue priority promotions | {fmt_int(session.get('analysis_queue_priority_promotions', 0))} |")
            a(f"| Active workers (peak) | {fmt_int(session.get('peak_active_analysis_workers', 0))} |")
            a(f"| Worker limit | {fmt_int(session.get('analysis_worker_limit', 0))} |")
            a(f"| Cost budget | {fmt_int(session.get('analysis_cost_budget', 0))} |")
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

    if quality_contract:
        a("")
        a("## Quality Contract")
        a("")
        a("| Metric | Value |")
        a("|---|---:|")
        a(
            f"| Present rate | {fmt_pct(quality_contract.get('quality_contract_present_rate', 0) * 100)} |"
        )
        a(
            f"| Recommended checks emitted | {fmt_int(quality_contract.get('recommended_checks_total', 0))} |"
        )
        a(
            f"| Performance watchpoints emitted | {fmt_int(quality_contract.get('performance_watchpoints_total', 0))} |"
        )
        scenarios = quality_contract.get("scenarios", [])
        if scenarios:
            a("")
            a("| Scenario | Contract | Quality Focus | Recommended Checks | Watchpoints |")
            a("|---|---:|---:|---:|---:|")
            for item in scenarios:
                a(
                    f"| {item.get('scenario', 'unknown')} | "
                    f"{'yes' if item.get('has_quality_contract') else 'no'} | "
                    f"{fmt_int(item.get('quality_focus_count', 0))} | "
                    f"{fmt_int(item.get('recommended_check_count', 0))} | "
                    f"{fmt_int(item.get('performance_watchpoint_count', 0))} |"
                )

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

    if regression_results:
        a("")
        a("## Point-Lookup Regressions")
        a("")
        a("| Task | Baseline Tokens | CodeLens Tokens | Overhead |")
        a("|---|---:|---:|---:|")
        for result in regression_results:
            baseline_tokens = result.get("baseline_tokens", 0)
            codelens_tokens = result.get("codelens_tokens", 0)
            overhead = 0.0
            if baseline_tokens:
                overhead = ((codelens_tokens / baseline_tokens) - 1) * 100
            a(
                f"| {result.get('task', 'unknown')} | "
                f"{fmt_int(baseline_tokens)} | "
                f"{fmt_int(codelens_tokens)} | "
                f"+{fmt_pct(overhead)} |"
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
