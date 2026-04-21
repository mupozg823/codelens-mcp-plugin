#!/usr/bin/env python3
"""Scenario runners for the token-efficiency benchmark."""

from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass

import benchmark_runtime_common as runtime_common


@dataclass(frozen=True)
class QueueGateThresholds:
    min_queue_depth: int
    min_peak_workers: int
    min_queue_success_rate: float
    max_queue_failures: int


def run_sequence(label, steps, runtime: runtime_common.BenchmarkRuntime, low_level_tools):
    """Run a tool sequence and compute workflow-visible token/latency metrics."""
    outputs = []
    total_tokens = 0
    total_ms = 0
    retries = 0
    low_level_calls = 0
    for step in steps:
        timeout = step.get("timeout", 20)
        output, tokens, elapsed_ms, payload = runtime.codelens(
            step["cmd"],
            step["args"],
            timeout=timeout,
            preset=step.get("preset"),
            profile=step.get("profile"),
        )
        if not output and not payload:
            retries += 1
            output, tokens, elapsed_ms, payload = runtime.codelens(
                step["cmd"],
                step["args"],
                timeout=timeout,
                preset=step.get("preset"),
                profile=step.get("profile"),
            )
        outputs.append(
            {
                "tool": step["cmd"],
                "surface": step.get("profile") or f"preset:{step.get('preset', 'balanced')}",
                "elapsed_ms": elapsed_ms,
                "tokens": tokens,
                "success": bool(payload and payload.get("success")),
                "data": payload.get("data", {}) if payload else {},
            }
        )
        total_tokens += tokens
        total_ms += elapsed_ms
        if step["cmd"] in low_level_tools:
            low_level_calls += 1
    return {
        "label": label,
        "tool_call_count": len(steps),
        "low_level_chain_count": low_level_calls if low_level_calls > 1 else 0,
        "total_tokens": total_tokens,
        "total_ms": total_ms,
        "retry_count": retries,
        "p95_latency_ms": runtime.percentile_95([entry["elapsed_ms"] for entry in outputs]),
        "steps": outputs,
    }


def summarize_quality_contract(workflow_results):
    summaries = []
    for result in workflow_results:
        compressed_steps = result.get("compressed", {}).get("steps", [])
        if not compressed_steps:
            continue
        data = compressed_steps[-1].get("data") or {}
        quality_focus = data.get("quality_focus") or []
        recommended_checks = data.get("recommended_checks") or []
        performance_watchpoints = data.get("performance_watchpoints") or []
        summaries.append(
            {
                "scenario": result.get("scenario", "unknown"),
                "has_quality_contract": all(
                    key in data
                    for key in (
                        "quality_focus",
                        "recommended_checks",
                        "performance_watchpoints",
                    )
                ),
                "quality_focus_count": len(quality_focus),
                "recommended_check_count": len(recommended_checks),
                "performance_watchpoint_count": len(performance_watchpoints),
            }
        )
    present = sum(1 for item in summaries if item["has_quality_contract"])
    watchpoints = sum(item["performance_watchpoint_count"] for item in summaries)
    checks = sum(item["recommended_check_count"] for item in summaries)
    return {
        "scenarios": summaries,
        "quality_contract_present_rate": (present / len(summaries)) if summaries else 0.0,
        "recommended_checks_total": checks,
        "performance_watchpoints_total": watchpoints,
    }


def summarize_verifier_contract(workflow_results):
    summaries = []
    total_blockers = 0
    total_verifier_checks = 0
    blocked_mutation_ready = 0
    caution_test_readiness = 0
    for result in workflow_results:
        compressed_steps = result.get("compressed", {}).get("steps", [])
        if not compressed_steps:
            continue
        data = compressed_steps[-1].get("data") or {}
        readiness = data.get("readiness") or {}
        verifier_checks = data.get("verifier_checks") or []
        blocker_count = int(data.get("blocker_count") or len(data.get("blockers") or []))
        scenario_summary = {
            "scenario": result.get("scenario", "unknown"),
            "has_verifier_contract": all(
                key in data for key in ("blockers", "blocker_count", "readiness", "verifier_checks")
            ),
            "blocker_count": blocker_count,
            "verifier_check_count": len(verifier_checks),
            "diagnostics_ready": readiness.get("diagnostics_ready", "unknown"),
            "reference_safety": readiness.get("reference_safety", "unknown"),
            "test_readiness": readiness.get("test_readiness", "unknown"),
            "mutation_ready": readiness.get("mutation_ready", "unknown"),
        }
        summaries.append(scenario_summary)
        total_blockers += blocker_count
        total_verifier_checks += len(verifier_checks)
        if scenario_summary["mutation_ready"] == "blocked":
            blocked_mutation_ready += 1
        if scenario_summary["test_readiness"] == "caution":
            caution_test_readiness += 1
    present = sum(1 for item in summaries if item["has_verifier_contract"])
    return {
        "scenarios": summaries,
        "verifier_contract_present_rate": (present / len(summaries)) if summaries else 0.0,
        "blocker_total": total_blockers,
        "verifier_checks_total": total_verifier_checks,
        "blocked_mutation_ready_scenarios": blocked_mutation_ready,
        "caution_test_readiness_scenarios": caution_test_readiness,
    }


def compare_workflows(name, baseline_steps, compressed_steps, runtime: runtime_common.BenchmarkRuntime, low_level_tools):
    baseline = run_sequence(f"{name} baseline", baseline_steps, runtime, low_level_tools)
    compressed = run_sequence(f"{name} compressed", compressed_steps, runtime, low_level_tools)
    savings_pct = 0.0
    if baseline["total_tokens"] > 0:
        savings_pct = round(
            (1 - compressed["total_tokens"] / baseline["total_tokens"]) * 100, 1
        )
    return {
        "scenario": name,
        "baseline": baseline,
        "compressed": compressed,
        "savings_pct": savings_pct,
    }


def run_queue_observability_benchmark(
    runtime: runtime_common.BenchmarkRuntime,
    thresholds: QueueGateThresholds,
    key_file,
    test_file,
):
    base_url, port, proc = runtime.start_http_daemon()
    if not base_url:
        runtime.stop_http_daemon(proc)
        return {"supported": False, "reason": "http transport unavailable"}
    try:
        first = runtime.mcp_http_tool_call(
            base_url,
            "start_analysis_job",
            {
                "kind": "impact_report",
                "path": key_file or test_file or ".",
                "debug_step_delay_ms": 80,
                "profile_hint": "reviewer-graph",
            },
            request_id=11,
        )
        second = runtime.mcp_http_tool_call(
            base_url,
            "start_analysis_job",
            {
                "kind": "impact_report",
                "path": test_file or key_file or ".",
                "debug_step_delay_ms": 20,
                "profile_hint": "reviewer-graph",
            },
            request_id=12,
        )
        third = runtime.mcp_http_tool_call(
            base_url,
            "start_analysis_job",
            {
                "kind": "impact_report",
                "path": key_file or test_file or ".",
                "debug_step_delay_ms": 20,
                "profile_hint": "reviewer-graph",
            },
            request_id=13,
        )
        first_payload = runtime.extract_tool_payload(first)
        second_payload = runtime.extract_tool_payload(second)
        third_payload = runtime.extract_tool_payload(third)
        first_id = first_payload["data"]["job_id"]
        second_id = second_payload["data"]["job_id"]
        third_id = third_payload["data"]["job_id"]
        saw_queued = False
        saw_running = False

        for idx in range(100):
            first_status = runtime.mcp_http_tool_call(
                base_url,
                "get_analysis_job",
                {"job_id": first_id},
                request_id=20 + idx,
            )
            first_data = runtime.extract_tool_payload(first_status).get("data", {})
            if first_data.get("status") in {"running", "completed"}:
                saw_running = True
                break
            time.sleep(0.05)

        final_jobs = {}
        all_terminal = False
        for idx in range(300):
            first_status = runtime.mcp_http_tool_call(
                base_url,
                "get_analysis_job",
                {"job_id": first_id},
                request_id=100 + idx,
            )
            second_status = runtime.mcp_http_tool_call(
                base_url,
                "get_analysis_job",
                {"job_id": second_id},
                request_id=200 + idx,
            )
            third_status = runtime.mcp_http_tool_call(
                base_url,
                "get_analysis_job",
                {"job_id": third_id},
                request_id=300 + idx,
            )
            first_data = runtime.extract_tool_payload(first_status).get("data", {})
            second_data = runtime.extract_tool_payload(second_status).get("data", {})
            third_data = runtime.extract_tool_payload(third_status).get("data", {})
            saw_running = saw_running or any(
                job.get("status") == "running"
                for job in (first_data, second_data, third_data)
            )
            saw_queued = saw_queued or any(
                job.get("status") == "queued"
                for job in (first_data, second_data, third_data)
            )
            final_jobs = {
                first_id: first_data,
                second_id: second_data,
                third_id: third_data,
            }
            if all(
                job.get("status") in {"completed", "failed", "cancelled"}
                for job in final_jobs.values()
            ):
                all_terminal = True
                break
            time.sleep(0.1)
        metrics_resp = runtime.mcp_http_tool_call(
            base_url,
            "get_tool_metrics",
            {},
            request_id=999,
        )
        metrics_payload = runtime.extract_tool_payload(metrics_resp).get("data", {})
        session = metrics_payload.get("session", {})
        derived = metrics_payload.get("derived_kpis", {})
        expected_jobs = 3
        for idx in range(50):
            terminal_count = (
                session.get("analysis_jobs_completed", 0)
                + session.get("analysis_jobs_failed", 0)
                + session.get("analysis_jobs_cancelled", 0)
            )
            if not all_terminal or (
                terminal_count >= expected_jobs
                and session.get("active_analysis_workers", 0) == 0
            ):
                break
            time.sleep(0.1)
            metrics_resp = runtime.mcp_http_tool_call(
                base_url,
                "get_tool_metrics",
                {},
                request_id=1000 + idx,
            )
            metrics_payload = runtime.extract_tool_payload(metrics_resp).get("data", {})
            session = metrics_payload.get("session", {})
            derived = metrics_payload.get("derived_kpis", {})
        queue_failures = session.get("analysis_jobs_failed", 0)
        queue_max_depth = session.get("analysis_queue_max_depth", 0)
        peak_workers = session.get("peak_active_analysis_workers", 0)
        success_rate = derived.get("analysis_job_success_rate", 0.0)
        queue_failures_list = []
        if queue_max_depth < thresholds.min_queue_depth:
            queue_failures_list.append(
                f"queue depth {queue_max_depth} < required {thresholds.min_queue_depth}"
            )
        if peak_workers < thresholds.min_peak_workers:
            queue_failures_list.append(
                f"peak workers {peak_workers} < required {thresholds.min_peak_workers}"
            )
        if queue_failures > thresholds.max_queue_failures:
            queue_failures_list.append(
                f"queue failures {queue_failures} > allowed {thresholds.max_queue_failures}"
            )
        if success_rate < thresholds.min_queue_success_rate:
            queue_failures_list.append(
                f"queue success rate {success_rate:.2f} < required {thresholds.min_queue_success_rate:.2f}"
            )
        if not all_terminal:
            queue_failures_list.append("timed out waiting for analysis jobs to reach a terminal state")
        return {
            "supported": True,
            "saw_running": saw_running,
            "saw_queued": saw_queued,
            "all_terminal": all_terminal,
            "job_statuses": {
                job_id: job.get("status", "unknown") for job_id, job in final_jobs.items()
            },
            "session": {
                "analysis_jobs_enqueued": session.get("analysis_jobs_enqueued", 0),
                "analysis_jobs_started": session.get("analysis_jobs_started", 0),
                "analysis_jobs_completed": session.get("analysis_jobs_completed", 0),
                "analysis_jobs_failed": session.get("analysis_jobs_failed", 0),
                "analysis_jobs_cancelled": session.get("analysis_jobs_cancelled", 0),
                "analysis_queue_depth": session.get("analysis_queue_depth", 0),
                "analysis_queue_max_depth": session.get("analysis_queue_max_depth", 0),
                "analysis_queue_weighted_depth": session.get(
                    "analysis_queue_weighted_depth", 0
                ),
                "analysis_queue_max_weighted_depth": session.get(
                    "analysis_queue_max_weighted_depth", 0
                ),
                "analysis_queue_priority_promotions": session.get(
                    "analysis_queue_priority_promotions", 0
                ),
                "active_analysis_workers": session.get("active_analysis_workers", 0),
                "peak_active_analysis_workers": session.get(
                    "peak_active_analysis_workers", 0
                ),
                "analysis_worker_limit": session.get("analysis_worker_limit", 0),
                "analysis_cost_budget": session.get("analysis_cost_budget", 0),
                "analysis_transport_mode": session.get("analysis_transport_mode", "unknown"),
            },
            "derived_kpis": {
                "analysis_job_success_rate": success_rate
            },
            "checks": {
                "min_queue_depth": thresholds.min_queue_depth,
                "min_peak_workers": thresholds.min_peak_workers,
                "min_queue_success_rate": thresholds.min_queue_success_rate,
                "max_queue_failures": thresholds.max_queue_failures,
            },
            "gate_passed": len(queue_failures_list) == 0 and saw_running and saw_queued,
            "gate_failures": queue_failures_list,
            "port": port,
        }
    except Exception as exc:
        return {"supported": False, "reason": str(exc)}
    finally:
        runtime.stop_http_daemon(proc)


def run_watcher_observability_benchmark(runtime: runtime_common.BenchmarkRuntime):
    base_url, port, proc = runtime.start_http_daemon()
    if not base_url:
        runtime.stop_http_daemon(proc)
        return {"supported": False, "reason": "http transport unavailable"}
    try:
        session_id, _, _ = runtime.initialize_http_session(base_url, request_id=1998)
        if not session_id:
            return {"supported": False, "reason": "missing session id"}
        metrics_resp = runtime.mcp_http_tool_call(
            base_url,
            "get_tool_metrics",
            {},
            request_id=1999,
            session_id=session_id,
        )
        metrics_payload = runtime.extract_tool_payload(metrics_resp).get("data", {})
        session = metrics_payload.get("session", {})
        derived = metrics_payload.get("derived_kpis", {})
        watch_resp = runtime.mcp_http_tool_call(
            base_url,
            "get_watch_status",
            {},
            request_id=2000,
            session_id=session_id,
        )
        watch_payload = runtime.extract_tool_payload(watch_resp).get("data", {})
        return {
            "supported": True,
            "session": {
                "watcher_running": session.get("watcher_running", False),
                "watcher_events_processed": session.get("watcher_events_processed", 0),
                "watcher_files_reindexed": session.get("watcher_files_reindexed", 0),
                "watcher_lock_contention_batches": session.get(
                    "watcher_lock_contention_batches", 0
                ),
                "watcher_index_failures": session.get("watcher_index_failures", 0),
                "watcher_index_failures_total": session.get(
                    "watcher_index_failures_total", 0
                ),
                "watcher_stale_index_failures": session.get(
                    "watcher_stale_index_failures", 0
                ),
                "watcher_persistent_index_failures": session.get(
                    "watcher_persistent_index_failures", 0
                ),
                "watcher_pruned_missing_failures": session.get(
                    "watcher_pruned_missing_failures", 0
                ),
            },
            "watch_status": {
                "running": watch_payload.get("running", False),
                "events_processed": watch_payload.get("events_processed", 0),
                "files_reindexed": watch_payload.get("files_reindexed", 0),
                "lock_contention_batches": watch_payload.get(
                    "lock_contention_batches", 0
                ),
                "index_failures": watch_payload.get("index_failures", 0),
                "index_failures_total": watch_payload.get("index_failures_total", 0),
                "stale_index_failures": watch_payload.get("stale_index_failures", 0),
                "persistent_index_failures": watch_payload.get(
                    "persistent_index_failures", 0
                ),
                "pruned_missing_failures": watch_payload.get(
                    "pruned_missing_failures", 0
                ),
            },
            "derived_kpis": {
                "watcher_lock_contention_rate": derived.get(
                    "watcher_lock_contention_rate", 0.0
                ),
                "watcher_recent_failure_share": derived.get(
                    "watcher_recent_failure_share", 0.0
                ),
            },
            "port": port,
        }
    except Exception as exc:
        return {"supported": False, "reason": str(exc)}
    finally:
        runtime.stop_http_daemon(proc)


def run_mutation_gate_benchmark(runtime: runtime_common.BenchmarkRuntime):
    base_url, port, proc = runtime.start_http_daemon(profile="refactor-full")
    if not base_url:
        runtime.stop_http_daemon(proc)
        return {"supported": False, "reason": "http transport unavailable"}
    rename_file = os.path.join(runtime.project, ".codelens-bench-rename.py")
    create_target = os.path.join(runtime.project, ".codelens-bench-created.txt")
    try:
        with open(rename_file, "w", encoding="utf-8") as handle:
            handle.write("def old_name():\n    return 1\n")

        session_id, _, _ = runtime.initialize_http_session(
            base_url,
            profile="refactor-full",
            request_id=3000,
        )
        if not session_id:
            return {"supported": False, "reason": "missing session id"}

        blocked = runtime.mcp_http_tool_call(
            base_url,
            "create_text_file",
            {
                "relative_path": ".codelens-bench-created.txt",
                "content": "hello",
                "overwrite": True,
            },
            request_id=3001,
            session_id=session_id,
        )
        blocked_payload = runtime.extract_tool_payload(blocked)

        preflight = runtime.mcp_http_tool_call(
            base_url,
            "verify_change_readiness",
            {
                "task": "create benchmark file",
                "changed_files": [".codelens-bench-created.txt"],
            },
            request_id=3002,
            session_id=session_id,
        )
        preflight_payload = runtime.extract_tool_payload(preflight)

        allowed = runtime.mcp_http_tool_call(
            base_url,
            "create_text_file",
            {
                "relative_path": ".codelens-bench-created.txt",
                "content": "hello",
                "overwrite": True,
            },
            request_id=3003,
            session_id=session_id,
        )
        allowed_payload = runtime.extract_tool_payload(allowed)

        rename_generic = runtime.mcp_http_tool_call(
            base_url,
            "verify_change_readiness",
            {
                "task": "rename old_name in .codelens-bench-rename.py",
                "changed_files": [".codelens-bench-rename.py"],
            },
            request_id=3004,
            session_id=session_id,
        )
        rename_generic_payload = runtime.extract_tool_payload(rename_generic)

        rename_blocked = runtime.mcp_http_tool_call(
            base_url,
            "rename_symbol",
            {
                "file_path": ".codelens-bench-rename.py",
                "symbol_name": "old_name",
                "new_name": "new_name",
                "dry_run": True,
            },
            request_id=3005,
            session_id=session_id,
        )
        rename_blocked_payload = runtime.extract_tool_payload(rename_blocked)

        metrics_resp = runtime.mcp_http_tool_call(
            base_url, "get_tool_metrics", {}, request_id=3006, session_id=session_id
        )
        metrics_payload = runtime.extract_tool_payload(metrics_resp).get("data", {})
        session = metrics_payload.get("session", {})
        derived = metrics_payload.get("derived_kpis", {})
        return {
            "supported": True,
            "session": {
                "mutation_without_preflight_count": session.get(
                    "mutation_without_preflight_count", 0
                ),
                "mutation_preflight_gate_denied_count": session.get(
                    "mutation_preflight_gate_denied_count", 0
                ),
                "stale_preflight_reject_count": session.get(
                    "stale_preflight_reject_count", 0
                ),
                "mutation_with_caution_count": session.get(
                    "mutation_with_caution_count", 0
                ),
                "rename_without_symbol_preflight_count": session.get(
                    "rename_without_symbol_preflight_count", 0
                ),
            },
            "derived_kpis": {
                "mutation_preflight_gate_deny_rate": derived.get(
                    "mutation_preflight_gate_deny_rate", 0.0
                ),
            },
            "checks": {
                "missing_preflight_denied": blocked_payload.get("success") is False
                and "fresh preflight" in (blocked_payload.get("error") or ""),
                "preflight_mutation_allowed": allowed_payload.get("success") is True,
                "rename_requires_symbol_preflight": rename_blocked_payload.get("success")
                is False
                and "symbol-aware preflight" in (rename_blocked_payload.get("error") or ""),
                "preflight_mutation_ready": (
                    preflight_payload.get("data", {})
                    .get("readiness", {})
                    .get("mutation_ready")
                ),
                "rename_generic_mutation_ready": (
                    rename_generic_payload.get("data", {})
                    .get("readiness", {})
                    .get("mutation_ready")
                ),
            },
            "port": port,
        }
    except Exception as exc:
        return {"supported": False, "reason": str(exc)}
    finally:
        for path in (rename_file, create_target):
            try:
                if os.path.exists(path):
                    os.remove(path)
            except OSError:
                pass
        runtime.stop_http_daemon(proc)


def run_deferred_gate_benchmark(runtime: runtime_common.BenchmarkRuntime):
    base_url, port, proc = runtime.start_http_daemon(profile="reviewer-graph")
    if not base_url:
        runtime.stop_http_daemon(proc)
        return {"supported": False, "reason": "http transport unavailable"}
    file_path = os.path.join(runtime.project, ".codelens-bench-deferred.py")
    try:
        with open(file_path, "w", encoding="utf-8") as handle:
            handle.write("def beta():\n    return 2\n")

        session_id, _, _ = runtime.initialize_http_session(
            base_url,
            profile="reviewer-graph",
            deferred_tool_loading=True,
            request_id=3100,
        )
        if not session_id:
            return {"supported": False, "reason": "missing session id"}

        blocked_namespace = runtime.mcp_http_tool_call(
            base_url,
            "read_file",
            {"file_path": file_path},
            request_id=3101,
            session_id=session_id,
        )
        blocked_namespace_payload = runtime.extract_tool_payload(blocked_namespace)

        namespace_expand = runtime.mcp_http_call(
            base_url,
            "tools/list",
            {"namespace": "filesystem"},
            request_id=3102,
            headers={"mcp-session-id": session_id},
        )

        blocked_tier = runtime.mcp_http_tool_call(
            base_url,
            "find_symbol",
            {"name": "beta", "file_path": file_path, "include_body": False},
            request_id=3103,
            session_id=session_id,
        )
        blocked_tier_payload = runtime.extract_tool_payload(blocked_tier)

        tier_expand = runtime.mcp_http_call(
            base_url,
            "tools/list",
            {"tier": "primitive"},
            request_id=3104,
            headers={"mcp-session-id": session_id},
        )

        session_resource = runtime.mcp_http_resource_read(
            base_url,
            "codelens://session/http",
            request_id=3105,
            session_id=session_id,
        )
        session_resource_body = json.dumps(session_resource, ensure_ascii=False)

        metrics_resp = runtime.mcp_http_tool_call(
            base_url, "get_tool_metrics", {}, request_id=3106, session_id=session_id
        )
        metrics_payload = runtime.extract_tool_payload(metrics_resp).get("data", {})
        session = metrics_payload.get("session", {})
        derived = metrics_payload.get("derived_kpis", {})
        return {
            "supported": True,
            "session": {
                "deferred_namespace_expansion_count": session.get(
                    "deferred_namespace_expansion_count", 0
                ),
                "deferred_hidden_tool_call_denied_count": session.get(
                    "deferred_hidden_tool_call_denied_count", 0
                ),
            },
            "derived_kpis": {
                "deferred_hidden_tool_call_deny_rate": derived.get(
                    "deferred_hidden_tool_call_deny_rate", 0.0
                ),
            },
            "checks": {
                "hidden_namespace_denied": blocked_namespace_payload.get("success") is False
                and "hidden by deferred loading" in (blocked_namespace_payload.get("error") or ""),
                "hidden_tier_denied": blocked_tier_payload.get("success") is False
                and "tier `primitive`" in (blocked_tier_payload.get("error") or ""),
                "filesystem_namespace_loaded": "filesystem"
                in json.dumps(namespace_expand, ensure_ascii=False),
                "primitive_tier_loaded": "primitive"
                in json.dumps(tier_expand, ensure_ascii=False),
                "session_reports_loaded_namespace": "filesystem" in session_resource_body,
                "session_reports_loaded_tier": "primitive" in session_resource_body,
            },
            "port": port,
        }
    except Exception as exc:
        return {"supported": False, "reason": str(exc)}
    finally:
        try:
            if os.path.exists(file_path):
                os.remove(file_path)
        except OSError:
            pass
        runtime.stop_http_daemon(proc)


def run_gate_observability_benchmark(runtime: runtime_common.BenchmarkRuntime):
    mutation_gate = run_mutation_gate_benchmark(runtime)
    deferred_gate = run_deferred_gate_benchmark(runtime)
    if not mutation_gate.get("supported") and not deferred_gate.get("supported"):
        reasons = []
        if mutation_gate.get("reason"):
            reasons.append(f"mutation={mutation_gate['reason']}")
        if deferred_gate.get("reason"):
            reasons.append(f"deferred={deferred_gate['reason']}")
        return {
            "supported": False,
            "reason": ", ".join(reasons) if reasons else "unavailable",
            "mutation_gate": mutation_gate,
            "deferred_gate": deferred_gate,
        }
    return {
        "supported": True,
        "mutation_gate": mutation_gate,
        "deferred_gate": deferred_gate,
    }
