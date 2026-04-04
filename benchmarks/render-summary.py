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
    watcher = data.get("watcher_observability", {})
    quality_contract = data.get("quality_contract", {})
    verifier_contract = data.get("verifier_contract", {})
    gate_observability = data.get("gate_observability", {})
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

    if watcher:
        a("")
        a("## Watcher Observability")
        a("")
        if watcher.get("supported"):
            session = watcher.get("session", {})
            status = watcher.get("watch_status", {})
            derived = watcher.get("derived_kpis", {})
            a("| Metric | Value |")
            a("|---|---:|")
            a(f"| Watcher running | {session.get('watcher_running')} |")
            a(f"| Events processed | {fmt_int(session.get('watcher_events_processed', 0))} |")
            a(f"| Files reindexed | {fmt_int(session.get('watcher_files_reindexed', 0))} |")
            a(f"| Lock contention batches | {fmt_int(session.get('watcher_lock_contention_batches', 0))} |")
            a(f"| Recent index failures | {fmt_int(session.get('watcher_index_failures', 0))} |")
            a(f"| Total unresolved failures | {fmt_int(session.get('watcher_index_failures_total', 0))} |")
            a(f"| Stale failures | {fmt_int(session.get('watcher_stale_index_failures', 0))} |")
            a(f"| Persistent failures | {fmt_int(session.get('watcher_persistent_index_failures', 0))} |")
            a(f"| Pruned missing failures | {fmt_int(session.get('watcher_pruned_missing_failures', 0))} |")
            a(f"| Lock contention rate | {derived.get('watcher_lock_contention_rate', 0.0):.4f} |")
            a(f"| Recent failure share | {derived.get('watcher_recent_failure_share', 0.0):.4f} |")
            a(f"| Watch status parity | running={status.get('running')}, contention={fmt_int(status.get('lock_contention_batches', 0))}, recent={fmt_int(status.get('index_failures', 0))}, total={fmt_int(status.get('index_failures_total', 0))} |")
        else:
            a(f"- skipped: {watcher.get('reason', 'unavailable')}")

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

    if verifier_contract:
        a("")
        a("## Verifier Contract")
        a("")
        a("| Metric | Value |")
        a("|---|---:|")
        a(
            f"| Present rate | {fmt_pct(verifier_contract.get('verifier_contract_present_rate', 0) * 100)} |"
        )
        a(
            f"| Blockers emitted | {fmt_int(verifier_contract.get('blocker_total', 0))} |"
        )
        a(
            f"| Verifier checks emitted | {fmt_int(verifier_contract.get('verifier_checks_total', 0))} |"
        )
        a(
            f"| Blocked mutation-ready scenarios | {fmt_int(verifier_contract.get('blocked_mutation_ready_scenarios', 0))} |"
        )
        a(
            f"| Caution test-readiness scenarios | {fmt_int(verifier_contract.get('caution_test_readiness_scenarios', 0))} |"
        )
        scenarios = verifier_contract.get("scenarios", [])
        if scenarios:
            a("")
            a("| Scenario | Contract | Blockers | Checks | Diagnostics | References | Tests | Mutation |")
            a("|---|---:|---:|---:|---|---|---|---|")
            for item in scenarios:
                a(
                    f"| {item.get('scenario', 'unknown')} | "
                    f"{'yes' if item.get('has_verifier_contract') else 'no'} | "
                    f"{fmt_int(item.get('blocker_count', 0))} | "
                    f"{fmt_int(item.get('verifier_check_count', 0))} | "
                    f"{item.get('diagnostics_ready', 'unknown')} | "
                    f"{item.get('reference_safety', 'unknown')} | "
                    f"{item.get('test_readiness', 'unknown')} | "
                    f"{item.get('mutation_ready', 'unknown')} |"
                )

    if gate_observability:
        a("")
        a("## Execution Gates")
        a("")
        if gate_observability.get("supported"):
            mutation = gate_observability.get("mutation_gate", {})
            mutation_session = mutation.get("session", {})
            mutation_checks = mutation.get("checks", {})
            deferred = gate_observability.get("deferred_gate", {})
            deferred_session = deferred.get("session", {})
            deferred_checks = deferred.get("checks", {})
            a("| Metric | Value |")
            a("|---|---:|")
            a(
                f"| Mutation preflight denies | {fmt_int(mutation_session.get('mutation_preflight_gate_denied_count', 0))} |"
            )
            a(
                f"| Mutation caution count | {fmt_int(mutation_session.get('mutation_with_caution_count', 0))} |"
            )
            a(
                f"| Rename symbol-preflight denies | {fmt_int(mutation_session.get('rename_without_symbol_preflight_count', 0))} |"
            )
            a(
                f"| Mutation gate deny rate | {fmt_pct(mutation.get('derived_kpis', {}).get('mutation_preflight_gate_deny_rate', 0.0) * 100)} |"
            )
            a(
                f"| Missing preflight denied | {mutation_checks.get('missing_preflight_denied')} |"
            )
            a(
                f"| Preflight mutation allowed | {mutation_checks.get('preflight_mutation_allowed')} |"
            )
            a(
                f"| Rename requires symbol preflight | {mutation_checks.get('rename_requires_symbol_preflight')} |"
            )
            a(
                f"| Deferred namespace expansions | {fmt_int(deferred_session.get('deferred_namespace_expansion_count', 0))} |"
            )
            a(
                f"| Deferred hidden tool denies | {fmt_int(deferred_session.get('deferred_hidden_tool_call_denied_count', 0))} |"
            )
            a(
                f"| Deferred hidden-call deny rate | {fmt_pct(deferred.get('derived_kpis', {}).get('deferred_hidden_tool_call_deny_rate', 0.0) * 100)} |"
            )
            a(
                f"| Hidden namespace denied | {deferred_checks.get('hidden_namespace_denied')} |"
            )
            a(
                f"| Hidden tier denied | {deferred_checks.get('hidden_tier_denied')} |"
            )
            a(
                f"| Filesystem namespace loaded | {deferred_checks.get('filesystem_namespace_loaded')} |"
            )
            a(
                f"| Primitive tier loaded | {deferred_checks.get('primitive_tier_loaded')} |"
            )
        else:
            a(f"- skipped: {gate_observability.get('reason', 'unavailable')}")

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
