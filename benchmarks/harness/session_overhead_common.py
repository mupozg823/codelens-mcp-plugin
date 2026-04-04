#!/usr/bin/env python3
"""Harness-consumer session overhead benchmarks for CodeLens workflows."""

from __future__ import annotations

import benchmark_runtime_common as runtime_common


def run_session_overhead_scenario(
    scenario_name,
    workflow_result,
    profile,
    tool_name,
    arguments,
    runtime: runtime_common.BenchmarkRuntime,
):
    base_url, port, proc = runtime.start_http_daemon(profile=profile)
    if not base_url:
        runtime.stop_http_daemon(proc)
        return {"supported": False, "scenario": scenario_name, "reason": "http transport unavailable"}
    try:
        session_id, init_response, _ = runtime.initialize_http_session(
            base_url,
            profile=profile,
            deferred_tool_loading=True,
            request_id=4000,
        )
        if not session_id:
            return {"supported": False, "scenario": scenario_name, "reason": "missing session id"}

        list_response = runtime.mcp_http_call(
            base_url,
            "tools/list",
            request_id=4001,
            headers={"mcp-session-id": session_id},
        )
        tool_response = runtime.mcp_http_tool_call(
            base_url,
            tool_name,
            arguments,
            request_id=4002,
            session_id=session_id,
        )

        list_result = list_response.get("result", {}) if isinstance(list_response, dict) else {}
        tool_payload = runtime.extract_tool_payload(tool_response)
        tool_data = tool_payload.get("data", {}) if isinstance(tool_payload, dict) else {}
        baseline_tokens = int(workflow_result.get("baseline", {}).get("total_tokens", 0))
        direct_profile_tokens = int(workflow_result.get("compressed", {}).get("total_tokens", 0))
        init_tokens = runtime.count_json_tokens(init_response)
        list_tokens = runtime.count_json_tokens(list_response)
        tool_tokens = runtime.count_json_tokens(tool_response)
        total_session_tokens = init_tokens + list_tokens + tool_tokens
        bootstrap_tokens = init_tokens + list_tokens

        overhead_pct = 0.0
        if direct_profile_tokens > 0:
            overhead_pct = round(
                ((total_session_tokens / direct_profile_tokens) - 1) * 100, 1
            )

        savings_pct = 0.0
        if baseline_tokens > 0:
            savings_pct = round(
                (1 - total_session_tokens / baseline_tokens) * 100, 1
            )

        return {
            "supported": True,
            "scenario": scenario_name,
            "profile": profile,
            "tool_name": tool_name,
            "deferred_loading": True,
            "port": port,
            "bootstrap_tokens": bootstrap_tokens,
            "tool_response_tokens": tool_tokens,
            "total_session_tokens": total_session_tokens,
            "baseline_tokens": baseline_tokens,
            "direct_profile_tokens": direct_profile_tokens,
            "session_overhead_vs_direct_pct": overhead_pct,
            "session_savings_vs_baseline_pct": savings_pct,
            "tool_count": list_result.get("tool_count", len(list_result.get("tools", []))),
            "tool_count_total": list_result.get(
                "tool_count_total",
                list_result.get("tool_count", len(list_result.get("tools", []))),
            ),
            "effective_namespaces": list_result.get("effective_namespaces", []),
            "preferred_namespaces": list_result.get("preferred_namespaces", []),
            "loaded_tiers": list_result.get("loaded_tiers", []),
            "tool_success": bool(tool_payload.get("success")),
            "analysis_id": tool_data.get("analysis_id") or tool_payload.get("analysis_id"),
        }
    except Exception as exc:
        return {"supported": False, "scenario": scenario_name, "reason": str(exc)}
    finally:
        runtime.stop_http_daemon(proc)


def run_session_overhead_benchmark(
    workflow_results,
    runtime: runtime_common.BenchmarkRuntime,
    planner_task,
    key_file,
    test_file,
    test_symbol,
):
    scenarios = []
    workflow_by_name = {
        result.get("scenario"): result for result in workflow_results if result.get("scenario")
    }
    planner_result = workflow_by_name.get("Planner change request")
    if planner_result:
        scenarios.append(
            run_session_overhead_scenario(
                "Planner change request",
                planner_result,
                "planner-readonly",
                "analyze_change_request",
                {"task": planner_task, "profile_hint": "planner-readonly"},
                runtime,
            )
        )
    reviewer_result = workflow_by_name.get("Reviewer impact analysis")
    if reviewer_result and key_file:
        scenarios.append(
            run_session_overhead_scenario(
                "Reviewer impact analysis",
                reviewer_result,
                "reviewer-graph",
                "impact_report",
                {"path": key_file},
                runtime,
            )
        )
    refactor_result = workflow_by_name.get("Refactor safety")
    if refactor_result and test_file:
        scenarios.append(
            run_session_overhead_scenario(
                "Refactor safety",
                refactor_result,
                "refactor-full",
                "refactor_safety_report",
                {
                    "task": f"refactor {test_symbol} safely",
                    "symbol": test_symbol,
                    "path": test_file,
                    "file_path": test_file,
                },
                runtime,
            )
        )

    supported = [scenario for scenario in scenarios if scenario.get("supported")]
    if not supported:
        reasons = [scenario.get("reason") for scenario in scenarios if scenario.get("reason")]
        return {
            "supported": False,
            "reason": ", ".join(reasons) if reasons else "no scenarios available",
            "scenarios": scenarios,
        }

    avg_overhead = sum(
        float(scenario.get("session_overhead_vs_direct_pct", 0.0)) for scenario in supported
    ) / len(supported)
    avg_savings = sum(
        float(scenario.get("session_savings_vs_baseline_pct", 0.0)) for scenario in supported
    ) / len(supported)
    avg_bootstrap = round(
        sum(int(scenario.get("bootstrap_tokens", 0)) for scenario in supported) / len(supported)
    )
    return {
        "supported": True,
        "scenario_count": len(supported),
        "avg_bootstrap_tokens": avg_bootstrap,
        "avg_session_overhead_vs_direct_pct": round(avg_overhead, 1),
        "avg_session_savings_vs_baseline_pct": round(avg_savings, 1),
        "scenarios": scenarios,
    }
