#!/usr/bin/env python3
"""Benchmark profile-aware HTTP surface payloads across two CodeLens binaries."""

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import time
from datetime import datetime, timezone
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import benchmark_runtime_common as runtime_common


DEFAULT_PROFILES = (
    "planner-readonly",
    "reviewer-graph",
    "refactor-full",
)
DEPRECATED_ALIASES = {
    "audit_security_context",
    "analyze_change_impact",
    "assess_change_readiness",
}
SCENARIOS = (
    {
        "name": "deferred_tools_list",
        "deferred_tool_loading": True,
        "method": "tools/list",
        "params": None,
        "client_name": "CodexHarness",
    },
    {
        "name": "surface_tools_list",
        "deferred_tool_loading": False,
        "method": "tools/list",
        "params": None,
        "client_name": "GenericHarness",
    },
    {
        "name": "prepare_harness_session",
        "deferred_tool_loading": True,
        "method": "tools/call",
        "tool_name": "prepare_harness_session",
        "arguments": {
            "auto_refresh_stale": False,
        },
        "client_name": "CodexHarness",
    },
)


def resolve_binary(explicit: str) -> Path:
    candidates = []
    if explicit:
        candidates.append(Path(explicit).expanduser())
    env_bin = os.environ.get("CODELENS_BIN")
    if env_bin:
        candidates.append(Path(env_bin).expanduser())
    candidates.extend(
        [
            ROOT / "target" / "release" / "codelens-mcp",
            ROOT / "target" / "debug" / "codelens-mcp",
        ]
    )
    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()
    raise FileNotFoundError("unable to find codelens-mcp binary")


def safe_pct_delta(baseline: float, candidate: float) -> float | None:
    if baseline == 0:
        return None
    return round(((candidate / baseline) - 1) * 100, 1)


def fmt_pct(delta: float | None) -> str:
    if delta is None:
        return "n/a"
    return f"{delta}%"


def median_int(values: list[int]) -> int:
    if not values:
        return 0
    return int(round(statistics.median(values)))


def tool_names_from_list_response(response: dict) -> list[str]:
    if not isinstance(response, dict):
        return []
    result = response.get("result", {})
    if not isinstance(result, dict):
        return []
    tools = result.get("tools", [])
    if not isinstance(tools, list):
        return []
    names = []
    for tool in tools:
        if isinstance(tool, dict):
            name = tool.get("name")
            if isinstance(name, str):
                names.append(name)
    return names


def timed_http_call(callable_):
    started = time.monotonic()
    result = callable_()
    elapsed_ms = int((time.monotonic() - started) * 1000)
    return result, elapsed_ms


def measure_scenario(
    *,
    binary: Path,
    project: str,
    profile: str,
    scenario: dict,
    iterations: int,
    count_tokens,
) -> dict:
    base_url, port, proc = runtime_common.start_http_daemon(
        binary,
        project,
        preset="full",
    )
    if not base_url:
        runtime_common.stop_http_daemon(proc)
        return {
            "supported": False,
            "profile": profile,
            "scenario": scenario["name"],
            "reason": "http transport unavailable",
        }

    init_tokens = []
    init_ms = []
    response_tokens = []
    response_ms = []
    tool_counts = []
    tool_counts_total = []
    alias_sets = []
    failures = []

    try:
        for index in range(iterations):
            try:
                init_response, init_elapsed = timed_http_call(
                    lambda: runtime_common.initialize_http_session(
                        base_url,
                        profile=profile,
                        deferred_tool_loading=scenario["deferred_tool_loading"],
                        client_name=scenario["client_name"],
                        request_id=1000 + index,
                        timeout_seconds=runtime_common.DEFAULT_HTTP_BOOTSTRAP_TIMEOUT_SECONDS,
                    )
                )
                session_id, init_payload, _ = init_response
                if not session_id:
                    raise RuntimeError("missing session id")

                init_tokens.append(runtime_common.count_json_tokens(init_payload, count_tokens))
                init_ms.append(init_elapsed)

                if scenario["method"] == "tools/list":
                    response, call_elapsed = timed_http_call(
                        lambda: runtime_common.mcp_http_call(
                            base_url,
                            "tools/list",
                            params=scenario["params"],
                            request_id=2000 + index,
                            headers={"mcp-session-id": session_id},
                            timeout_seconds=runtime_common.DEFAULT_HTTP_BOOTSTRAP_TIMEOUT_SECONDS,
                        )
                    )
                    names = tool_names_from_list_response(response)
                    result_payload = response.get("result", {}) if isinstance(response, dict) else {}
                    tool_counts.append(int(result_payload.get("tool_count", len(names))))
                    tool_counts_total.append(
                        int(result_payload.get("tool_count_total", result_payload.get("tool_count", len(names))))
                    )
                    alias_sets.append(sorted(DEPRECATED_ALIASES.intersection(names)))
                    response_tokens.append(runtime_common.count_json_tokens(response, count_tokens))
                    response_ms.append(call_elapsed)
                else:
                    response, call_elapsed = timed_http_call(
                        lambda: runtime_common.mcp_http_tool_call(
                            base_url,
                            scenario["tool_name"],
                            dict(scenario["arguments"]),
                            request_id=3000 + index,
                            session_id=session_id,
                            timeout_seconds=runtime_common.DEFAULT_HTTP_TOOL_TIMEOUT_SECONDS,
                        )
                    )
                    payload = runtime_common.extract_tool_payload(response)
                    if payload.get("success") is False:
                        raise RuntimeError(payload.get("error") or "tool call failed")
                    response_tokens.append(runtime_common.count_json_tokens(response, count_tokens))
                    response_ms.append(call_elapsed)
            except Exception as exc:  # pragma: no cover - benchmark should report, not crash
                failures.append(str(exc))

        if failures and len(failures) == iterations:
            return {
                "supported": False,
                "profile": profile,
                "scenario": scenario["name"],
                "reason": failures[0],
                "failures": failures,
                "port": port,
            }

        return {
            "supported": True,
            "profile": profile,
            "scenario": scenario["name"],
            "deferred_tool_loading": scenario["deferred_tool_loading"],
            "iterations": iterations,
            "completed_iterations": len(response_tokens),
            "initialize_tokens_p50": median_int(init_tokens),
            "initialize_latency_ms_p50": median_int(init_ms),
            "initialize_latency_ms_p95": runtime_common.percentile_95(init_ms),
            "response_tokens_p50": median_int(response_tokens),
            "response_latency_ms_p50": median_int(response_ms),
            "response_latency_ms_p95": runtime_common.percentile_95(response_ms),
            "total_tokens_p50": median_int(
                [a + b for a, b in zip(init_tokens, response_tokens, strict=False)]
            ),
            "tool_count_p50": median_int(tool_counts),
            "tool_count_total_p50": median_int(tool_counts_total),
            "visible_aliases": sorted({alias for aliases in alias_sets for alias in aliases}),
            "alias_visible_iterations": sum(1 for aliases in alias_sets if aliases),
            "port": port,
            "failures": failures,
        }
    finally:
        runtime_common.stop_http_daemon(proc)


def compare_results(baseline: dict, candidate: dict) -> dict:
    comparison = {
        "supported": baseline.get("supported") and candidate.get("supported"),
        "baseline": baseline,
        "candidate": candidate,
    }
    if not comparison["supported"]:
        return comparison

    comparison["delta"] = {
        "response_tokens_pct": safe_pct_delta(
            float(baseline.get("response_tokens_p50", 0)),
            float(candidate.get("response_tokens_p50", 0)),
        ),
        "total_tokens_pct": safe_pct_delta(
            float(baseline.get("total_tokens_p50", 0)),
            float(candidate.get("total_tokens_p50", 0)),
        ),
        "response_latency_ms_p50_pct": safe_pct_delta(
            float(baseline.get("response_latency_ms_p50", 0)),
            float(candidate.get("response_latency_ms_p50", 0)),
        ),
        "tool_count_total_pct": safe_pct_delta(
            float(baseline.get("tool_count_total_p50", 0)),
            float(candidate.get("tool_count_total_p50", 0)),
        ),
    }
    comparison["alias_cleanup"] = {
        "baseline_visible_aliases": baseline.get("visible_aliases", []),
        "candidate_visible_aliases": candidate.get("visible_aliases", []),
        "candidate_hides_aliases": not candidate.get("visible_aliases"),
    }
    return comparison


def run_binary_matrix(
    *,
    binary: Path,
    project: str,
    profiles: tuple[str, ...],
    iterations: int,
    count_tokens,
) -> dict:
    results = []
    for profile in profiles:
        for scenario in SCENARIOS:
            results.append(
                measure_scenario(
                    binary=binary,
                    project=project,
                    profile=profile,
                    scenario=scenario,
                    iterations=iterations,
                    count_tokens=count_tokens,
                )
            )
    return {
        "binary": str(binary),
        "results": results,
    }


def summarize_report(report: dict) -> dict:
    comparisons = report.get("comparisons", [])
    token_wins = 0
    latency_wins = 0
    alias_clean = 0
    supported = 0
    for item in comparisons:
        if not item.get("supported"):
            continue
        supported += 1
        delta = item.get("delta", {})
        if (delta.get("response_tokens_pct") or 0) < 0:
            token_wins += 1
        if (delta.get("response_latency_ms_p50_pct") or 0) < 0:
            latency_wins += 1
        if item.get("alias_cleanup", {}).get("candidate_hides_aliases"):
            alias_clean += 1
    return {
        "supported_comparisons": supported,
        "token_improvement_count": token_wins,
        "latency_improvement_count": latency_wins,
        "alias_hidden_count": alias_clean,
    }


def render_markdown(report: dict) -> str:
    lines = [
        "# HTTP Surface Benchmark",
        "",
        f"- Project: `{report['project']}`",
        f"- Baseline: `{report['baseline_binary']}`",
        f"- Candidate: `{report['candidate_binary']}`",
        f"- Iterations: `{report['iterations']}`",
        "",
        "| Profile | Scenario | Resp tokens | Total tokens | Resp p50 ms | Tool count | Aliases |",
        "| --- | --- | ---: | ---: | ---: | ---: | --- |",
    ]
    for item in report.get("comparisons", []):
        baseline = item.get("baseline", {})
        candidate = item.get("candidate", {})
        delta = item.get("delta", {})
        aliases = ",".join(candidate.get("visible_aliases", [])) or "-"
        if not item.get("supported"):
            lines.append(
                f"| {baseline.get('profile') or candidate.get('profile')} | "
                f"{baseline.get('scenario') or candidate.get('scenario')} | unsupported | unsupported | unsupported | unsupported | unsupported |"
            )
            continue
        lines.append(
            f"| {candidate['profile']} | {candidate['scenario']} | "
            f"{baseline['response_tokens_p50']} -> {candidate['response_tokens_p50']} "
            f"({fmt_pct(delta.get('response_tokens_pct'))}) | "
            f"{baseline['total_tokens_p50']} -> {candidate['total_tokens_p50']} "
            f"({fmt_pct(delta.get('total_tokens_pct'))}) | "
            f"{baseline['response_latency_ms_p50']} -> {candidate['response_latency_ms_p50']} "
            f"({fmt_pct(delta.get('response_latency_ms_p50_pct'))}) | "
            f"{baseline.get('tool_count_total_p50', 0)} -> {candidate.get('tool_count_total_p50', 0)} | "
            f"{aliases} |"
        )
    summary = report.get("summary", {})
    lines.extend(
        [
            "",
            "## Summary",
            "",
            f"- Supported comparisons: `{summary.get('supported_comparisons', 0)}`",
            f"- Response token improvements: `{summary.get('token_improvement_count', 0)}`",
            f"- Response latency improvements: `{summary.get('latency_improvement_count', 0)}`",
            f"- Candidate alias-hidden comparisons: `{summary.get('alias_hidden_count', 0)}`",
            "",
        ]
    )
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument("--baseline-binary", required=True)
    parser.add_argument("--candidate-binary", default="")
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument(
        "--output-json",
        default=str(SCRIPT_DIR / "results" / "http-surface-benchmark.json"),
    )
    parser.add_argument("--markdown-output", default="")
    args = parser.parse_args()

    project = os.path.abspath(args.project_path)
    profiles = DEFAULT_PROFILES
    count_tokens, token_warning = runtime_common.build_token_counter()
    if token_warning:
        print(token_warning, file=sys.stderr)

    baseline_binary = resolve_binary(args.baseline_binary)
    candidate_binary = resolve_binary(args.candidate_binary)
    baseline_matrix = run_binary_matrix(
        binary=baseline_binary,
        project=project,
        profiles=profiles,
        iterations=args.iterations,
        count_tokens=count_tokens,
    )
    candidate_matrix = run_binary_matrix(
        binary=candidate_binary,
        project=project,
        profiles=profiles,
        iterations=args.iterations,
        count_tokens=count_tokens,
    )

    baseline_by_key = {
        (item["profile"], item["scenario"]): item for item in baseline_matrix["results"]
    }
    candidate_by_key = {
        (item["profile"], item["scenario"]): item for item in candidate_matrix["results"]
    }
    comparisons = []
    for profile in profiles:
        for scenario in SCENARIOS:
            key = (profile, scenario["name"])
            comparisons.append(
                compare_results(
                    baseline_by_key.get(key, {"supported": False, "profile": profile, "scenario": scenario["name"]}),
                    candidate_by_key.get(key, {"supported": False, "profile": profile, "scenario": scenario["name"]}),
                )
            )

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "project": project,
        "baseline_binary": str(baseline_binary),
        "candidate_binary": str(candidate_binary),
        "iterations": args.iterations,
        "profiles": list(profiles),
        "baseline": baseline_matrix,
        "candidate": candidate_matrix,
        "comparisons": comparisons,
    }
    report["summary"] = summarize_report(report)

    output_path = Path(args.output_json).expanduser()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")

    markdown = render_markdown(report)
    if args.markdown_output:
        markdown_path = Path(args.markdown_output).expanduser()
        markdown_path.parent.mkdir(parents=True, exist_ok=True)
        markdown_path.write_text(markdown + "\n")

    print(markdown)


if __name__ == "__main__":
    main()
