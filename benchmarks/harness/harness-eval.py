#!/usr/bin/env python3
"""Hybrid evaluation for where CodeLens is meaningful inside agent harnesses."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from collections import defaultdict
from datetime import datetime
from pathlib import Path

import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
ROOT = BENCH_DIR.parent
DEFAULT_CONFIG = BENCH_DIR / "harness-eval-config.json"
DEFAULT_REPORT_DIR = Path.home() / ".codex" / "harness" / "reports"
DEFAULT_SESSION_GLOB = str(DEFAULT_REPORT_DIR / "session-entries" / "*.json")
SESSION_OVERHEAD_BENCHMARK = SCRIPT_DIR / "session-overhead-benchmark.py"

WORKFLOW_TASK_MAP = {
    "Planner change request": "onboarding/planning",
    "Reviewer impact analysis": "impact/reviewer",
    "Refactor safety": "refactor preflight",
}
LOCAL_LOOKUP_TASKS = ("Find symbol", "Understand file structure", "Context retrieval")

def default_report_paths(label: str):
    stamp = datetime.now().strftime("%Y-%m-%d")
    suffix = f"-{common.slugify(label)}" if label else ""
    return (
        DEFAULT_REPORT_DIR / f"{stamp}-codelens-eval{suffix}.json",
        DEFAULT_REPORT_DIR / f"{stamp}-codelens-eval{suffix}.md",
    )


def load_config(path: Path):
    return common.load_json(path)


def resolve_binary(explicit: str):
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
    raise FileNotFoundError("unable to find codelens-mcp binary; pass --binary or build the repo")


def run_token_benchmark(project_path: str, binary: Path):
    with tempfile.TemporaryDirectory(prefix="codelens-harness-eval-") as tempdir:
        output_json = Path(tempdir) / "benchmark.json"
        session_overhead_json = Path(tempdir) / "session-overhead.json"
        env = os.environ.copy()
        env["CODELENS_BIN"] = str(binary)
        cmd = [
            sys.executable,
            str(BENCH_DIR / "token-efficiency.py"),
            project_path,
            "--output-json",
            str(output_json),
        ]
        subprocess.run(cmd, cwd=ROOT, env=env, check=True, capture_output=True, text=True)
        benchmark = json.loads(output_json.read_text())
        session_cmd = [
            sys.executable,
            str(SESSION_OVERHEAD_BENCHMARK),
            project_path,
            "--benchmark-json",
            str(output_json),
            "--output-json",
            str(session_overhead_json),
        ]
        subprocess.run(session_cmd, cwd=ROOT, env=env, check=True, capture_output=True, text=True)
        benchmark["harness_session_overhead"] = json.loads(session_overhead_json.read_text())
        return benchmark


def make_entry(
    repo_cfg,
    task_kind: str,
    mode: str,
    agent: str,
    success: bool,
    token_in,
    token_out,
    bootstrap_tokens,
    tool_calls,
    low_level_chain_count,
    elapsed_ms,
    notes,
    recommended_policy="pending",
    source_kind="synthetic",
    acceptance_passed=None,
    verify_passed=None,
    quality_score=None,
):
    return {
        "schema_version": "codelens-harness-eval-entry-v1",
        "source_kind": source_kind,
        "repo": repo_cfg["path"],
        "repo_id": common.normalize_repo_id(repo_cfg),
        "repo_label": repo_cfg.get("label", common.normalize_repo_id(repo_cfg)),
        "task_kind": task_kind,
        "mode": mode,
        "agent": agent,
        "success": success,
        "acceptance_passed": acceptance_passed,
        "verify_passed": verify_passed,
        "quality_score": quality_score,
        "token_in": token_in,
        "token_out": token_out,
        "bootstrap_tokens": bootstrap_tokens,
        "tool_calls": tool_calls,
        "low_level_chain_count": low_level_chain_count,
        "elapsed_ms": elapsed_ms,
        "notes": notes,
        "recommended_policy": recommended_policy,
        "verify_commands": repo_cfg.get("verify_commands", []),
    }


def summarize_local_lookup(repo_cfg, benchmark):
    relevant = [
        item for item in benchmark.get("results", []) if item.get("task", "").startswith(LOCAL_LOOKUP_TASKS)
    ]
    if not relevant:
        return []
    baseline_tokens = sum(int(item.get("baseline_tokens", 0)) for item in relevant)
    baseline_ms = sum(int(item.get("baseline_ms", 0)) for item in relevant)
    naive_tokens = sum(int(item.get("codelens_tokens", 0)) for item in relevant)
    naive_ms = sum(int(item.get("codelens_ms", 0)) for item in relevant)
    notes = "synthetic aggregate over " + ", ".join(item.get("task", "unknown") for item in relevant)
    return [
        make_entry(
            repo_cfg,
            "simple local lookup/edit",
            "baseline",
            "synthetic-proxy",
            True,
            0,
            baseline_tokens,
            0,
            len(relevant),
            0,
            baseline_ms,
            notes,
        ),
        make_entry(
            repo_cfg,
            "simple local lookup/edit",
            "naive-on",
            "synthetic-proxy",
            True,
            0,
            naive_tokens,
            0,
            len(relevant),
            0,
            naive_ms,
            notes,
        ),
        make_entry(
            repo_cfg,
            "simple local lookup/edit",
            "routed-on",
            "synthetic-proxy",
            True,
            0,
            baseline_tokens,
            0,
            0,
            0,
            baseline_ms,
            "routing policy intentionally stays native for local lookup/edit",
            recommended_policy="avoid_codelens_for_simple_local_lookup",
        ),
    ]


def normalize_synthetic_entries(repo_cfg, benchmark):
    entries = []
    session_map = {
        item.get("scenario"): item
        for item in (benchmark.get("harness_session_overhead", {}) or {}).get("scenarios", [])
        if item.get("supported")
    }
    for workflow in benchmark.get("workflow_results", []):
        task_kind = WORKFLOW_TASK_MAP.get(workflow.get("scenario"))
        if not task_kind:
            continue
        baseline = workflow.get("baseline", {})
        compressed = workflow.get("compressed", {})
        session = session_map.get(workflow.get("scenario"), {})
        entries.extend(
            [
                make_entry(
                    repo_cfg,
                    task_kind,
                    "baseline",
                    "synthetic-proxy",
                    True,
                    0,
                    int(baseline.get("total_tokens", 0)),
                    0,
                    int(baseline.get("tool_call_count", 0)),
                    int(baseline.get("low_level_chain_count", 0)),
                    baseline.get("total_ms"),
                    f"proxy baseline from balanced low-level workflow: {workflow.get('scenario')}",
                ),
                make_entry(
                    repo_cfg,
                    task_kind,
                    "naive-on",
                    "synthetic-proxy",
                    True,
                    0,
                    int(compressed.get("total_tokens", 0)),
                    0,
                    int(compressed.get("tool_call_count", 0)),
                    int(compressed.get("low_level_chain_count", 0)),
                    compressed.get("total_ms"),
                    f"direct composite workflow without session bootstrap: {workflow.get('scenario')}",
                ),
                make_entry(
                    repo_cfg,
                    task_kind,
                    "routed-on",
                    "synthetic-proxy",
                    session.get("tool_success") if session else None,
                    int(session.get("bootstrap_tokens", 0)) if session else 0,
                    int(session.get("tool_response_tokens", 0)) if session else 0,
                    int(session.get("bootstrap_tokens", 0)) if session else 0,
                    int(session.get("tool_count", 0)) if session else 0,
                    0,
                    session.get("elapsed_ms"),
                    (
                        f"deferred workflow session; overhead_vs_direct={session.get('session_overhead_vs_direct_pct', 0.0)}%"
                        if session
                        else f"no routed session benchmark available for {workflow.get('scenario')}"
                    ),
                ),
            ]
        )
    entries.extend(summarize_local_lookup(repo_cfg, benchmark))
    return entries

def load_synthetic_entries_from_report(path: Path, representative_repos):
    raw = common.load_json(path.expanduser())
    selected_paths = {
        str(Path(repo_cfg["path"]).expanduser())
        for repo_cfg in representative_repos
    }
    entries = []
    for entry in raw.get("entries", []):
        if entry.get("source_kind") != "synthetic":
            continue
        repo_path = str(Path(entry.get("repo", "")).expanduser())
        if repo_path not in selected_paths:
            continue
        entries.append(entry)
    return entries, raw.get("synthetic_failures", [])


def total_tokens(entry):
    token_in = entry.get("token_in")
    token_out = entry.get("token_out")
    return (token_in or 0) + (token_out or 0)


def mode_stats(entries):
    count = len(entries)
    quality_scores = [float(entry["quality_score"]) for entry in entries if entry.get("quality_score") is not None]
    verify_scores = [entry.get("verify_passed") for entry in entries if entry.get("verify_passed") is not None]
    acceptance_scores = [
        entry.get("acceptance_passed") for entry in entries if entry.get("acceptance_passed") is not None
    ]
    measured_success = [entry.get("success") for entry in entries if entry.get("success") is not None]
    return {
        "count": count,
        "measured_count": len(measured_success),
        "avg_total_tokens": sum(total_tokens(entry) for entry in entries) / count if count else 0.0,
        "avg_bootstrap_tokens": sum(int(entry.get("bootstrap_tokens") or 0) for entry in entries) / count if count else 0.0,
        "avg_quality_score": sum(quality_scores) / len(quality_scores) if quality_scores else None,
        "verify_pass_rate": (
            sum(1 for value in verify_scores if value) / len(verify_scores) if verify_scores else None
        ),
        "acceptance_pass_rate": (
            sum(1 for value in acceptance_scores if value) / len(acceptance_scores)
            if acceptance_scores
            else None
        ),
        "success_rate": (
            sum(1 for value in measured_success if value) / len(measured_success)
            if measured_success
            else None
        ),
        "sample_notes": [entry.get("notes", "") for entry in entries[:2] if entry.get("notes")],
    }


def choose_policy(task_kind: str, stats_by_mode):
    viable = {
        mode: stats
        for mode, stats in stats_by_mode.items()
        if stats.get("success_rate") is not None and stats.get("success_rate", 0.0) > 0.0
    }
    if task_kind == "simple local lookup/edit":
        baseline = viable.get("baseline", {})
        naive = viable.get("naive-on", {})
        if naive.get("avg_total_tokens", 0) <= baseline.get("avg_total_tokens", 0):
            return "native_or_naive_both_ok_but_default_native"
        return "avoid_codelens_for_simple_local_lookup"

    quality_candidates = {
        mode: stats
        for mode, stats in viable.items()
        if stats.get("avg_quality_score") is not None
    }
    if quality_candidates:
        best_mode = sorted(
            quality_candidates.items(),
            key=lambda item: (
                item[1].get("avg_quality_score") or 0.0,
                item[1].get("verify_pass_rate") or 0.0,
                -item[1].get("avg_total_tokens", 0.0),
            ),
            reverse=True,
        )[0][0]
        return {
            "baseline": "prefer_native_baseline",
            "naive-on": "prefer_naive_codelens",
            "routed-on": "prefer_routed_codelens",
        }[best_mode]

    baseline = viable.get("baseline", {})
    naive = viable.get("naive-on", {})
    routed = viable.get("routed-on", {})
    if routed and routed.get("avg_total_tokens", 0) <= baseline.get("avg_total_tokens", 0):
        return "prefer_routed_codelens"
    if naive and naive.get("avg_total_tokens", 0) <= baseline.get("avg_total_tokens", 0):
        if routed and routed.get("avg_total_tokens", 0) > baseline.get("avg_total_tokens", 0):
            return "prefer_codelens_after_bootstrap"
        return "prefer_naive_codelens"
    return "prefer_native_baseline"


def build_task_summaries(entries):
    grouped = defaultdict(list)
    for entry in entries:
        grouped[(entry.get("repo_id", entry["repo"]), entry["task_kind"])].append(entry)

    summaries = []
    for (repo_id, task_kind), group_entries in sorted(grouped.items()):
        by_mode = defaultdict(list)
        for entry in group_entries:
            by_mode[entry["mode"]].append(entry)
        stats = {mode: mode_stats(records) for mode, records in by_mode.items()}
        policy = choose_policy(task_kind, stats)
        unsupported_modes = [
            mode
            for mode in ("baseline", "naive-on", "routed-on")
            if mode not in stats or stats[mode].get("measured_count", 0) == 0
        ]
        failing_modes = [
            mode
            for mode in ("baseline", "naive-on", "routed-on")
            if mode in stats
            and stats[mode].get("measured_count", 0) > 0
            and stats[mode].get("success_rate", 0.0) == 0.0
        ]
        has_real_quality = any(
            entry.get("source_kind") == "real-session" and entry.get("quality_score") is not None
            for entry in group_entries
        )
        if has_real_quality and not unsupported_modes and not failing_modes:
            confidence = "high"
        elif not unsupported_modes and not failing_modes:
            confidence = "medium"
        else:
            confidence = "low"
        for entry in group_entries:
            entry["recommended_policy"] = policy
        summaries.append(
            {
                "repo_id": repo_id,
                "repo": group_entries[0]["repo"],
                "repo_label": group_entries[0].get("repo_label", repo_id),
                "task_kind": task_kind,
                "recommended_policy": policy,
                "mode_stats": stats,
                "has_real_quality": has_real_quality,
                "unsupported_modes": unsupported_modes,
                "failing_modes": failing_modes,
                "confidence": confidence,
            }
        )
    return summaries


def render_report(report):
    lines = []
    a = lines.append
    baseline = report["baseline_reference"]
    entries = report["entries"]
    task_summaries = report["task_summaries"]
    binary = report.get("binary", "unknown")
    helped = [
        item for item in task_summaries if item["recommended_policy"] in {"prefer_routed_codelens", "prefer_codelens_after_bootstrap", "prefer_naive_codelens"}
    ]
    hurt = [
        item for item in task_summaries if item["recommended_policy"] in {"prefer_native_baseline", "avoid_codelens_for_simple_local_lookup"}
    ]
    needs_more_data = [item for item in task_summaries if item.get("confidence") == "low"]

    a("# CodeLens Harness Evaluation")
    a("")
    a("## Summary")
    a("")
    a("| Metric | Value |")
    a("|---|---|")
    a(f"| Binary | {binary} |")
    a(f"| Synthetic entries | {sum(1 for entry in entries if entry.get('source_kind') == 'synthetic')} |")
    a(f"| Real-session entries | {sum(1 for entry in entries if entry.get('source_kind') == 'real-session')} |")
    a(f"| Representative repos | {len(report['representative_repos'])} |")
    a(f"| Task summaries | {len(task_summaries)} |")
    a(f"| Baseline workflow savings | {baseline.get('workflow_total_savings_pct')}% |")
    a(
        f"| Baseline low-level chain | {baseline.get('low_level_chain_before')} -> {baseline.get('low_level_chain_after')} |"
    )
    a(f"| Baseline avg bootstrap tokens | {baseline.get('codex_like_avg_bootstrap_tokens')} |")
    a(
        f"| Baseline direct-composite overhead | {baseline.get('codex_like_avg_overhead_vs_direct_pct')}% |"
    )
    a(f"| Point lookup regression | {baseline.get('point_lookup_regression')} |")

    a("")
    a("## Task-by-Task Results")
    a("")
    a("| Repo | Task Kind | Mode | Source | Success | Acceptance | Verify | Quality | Total Tokens | Bootstrap | Calls | Low-level Chain | Elapsed(ms) | Policy |")
    a("|---|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---|")
    for entry in sorted(entries, key=lambda item: (item.get("repo_id", item["repo"]), item["task_kind"], item["mode"], item.get("source_kind", ""))):
        success = entry.get("success")
        success_label = "unsupported" if success is None else str(success)
        a(
            f"| {entry.get('repo_label', entry['repo'])} | "
            f"{entry['task_kind']} | "
            f"{entry['mode']} | "
            f"{entry.get('source_kind', 'unknown')} | "
            f"{success_label} | "
            f"{entry.get('acceptance_passed')} | "
            f"{entry.get('verify_passed')} | "
            f"{entry.get('quality_score') if entry.get('quality_score') is not None else '-'} | "
            f"{total_tokens(entry)} | "
            f"{entry.get('bootstrap_tokens') or 0} | "
            f"{entry.get('tool_calls') or 0} | "
            f"{entry.get('low_level_chain_count') or 0} | "
            f"{entry.get('elapsed_ms') if entry.get('elapsed_ms') is not None else '-'} | "
            f"{entry.get('recommended_policy', 'pending')} |"
        )

    a("")
    a("## Where CodeLens Helped")
    a("")
    if helped:
        for item in helped:
            routed = item["mode_stats"].get("routed-on", {})
            naive = item["mode_stats"].get("naive-on", {})
            baseline_stats = item["mode_stats"].get("baseline", {})
            a(
                f"- {item['repo_label']} / {item['task_kind']}: `{item['recommended_policy']}` "
                f"(baseline={baseline_stats.get('avg_total_tokens', 0):.0f}, "
                f"naive={naive.get('avg_total_tokens', 0):.0f}, "
                f"routed={routed.get('avg_total_tokens', 0):.0f}, "
                f"confidence={item.get('confidence', 'unknown')})"
            )
    else:
        a("- no helped segments recorded yet")

    a("")
    a("## Where CodeLens Hurt")
    a("")
    if hurt:
        for item in hurt:
            routed = item["mode_stats"].get("routed-on", {})
            naive = item["mode_stats"].get("naive-on", {})
            baseline_stats = item["mode_stats"].get("baseline", {})
            a(
                f"- {item['repo_label']} / {item['task_kind']}: `{item['recommended_policy']}` "
                f"(baseline={baseline_stats.get('avg_total_tokens', 0):.0f}, "
                f"naive={naive.get('avg_total_tokens', 0):.0f}, "
                f"routed={routed.get('avg_total_tokens', 0):.0f}, "
                f"confidence={item.get('confidence', 'unknown')})"
            )
    else:
        a("- no hurt segments recorded yet")

    a("")
    a("## Needs More Data")
    a("")
    if needs_more_data:
        for item in needs_more_data:
            unsupported = ", ".join(item.get("unsupported_modes") or []) or "-"
            failing = ", ".join(item.get("failing_modes") or []) or "-"
            a(
                f"- {item['repo_label']} / {item['task_kind']}: confidence={item['confidence']}, "
                f"unsupported={unsupported}, failing={failing}"
            )
    else:
        a("- all task summaries have at least medium confidence")

    a("")
    a("## Recommended Routing Rules")
    a("")
    for item in task_summaries:
        task_kind = item["task_kind"]
        policy = item["recommended_policy"]
        if policy == "avoid_codelens_for_simple_local_lookup":
            guidance = "stay native with rg/read/test; do not bootstrap CodeLens for point lookup or already-local edits"
        elif policy == "prefer_routed_codelens":
            guidance = "start with deferred workflow tools, then expand evidence/primitive tiers only as needed"
        elif policy == "prefer_codelens_after_bootstrap":
            guidance = "use CodeLens for multi-file reasoning, but only after the session is already warm or the task spans several steps"
        elif policy == "prefer_naive_codelens":
            guidance = "a direct composite call is already worthwhile; routed session overhead is not required"
        else:
            guidance = "default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy"
        suffix = ""
        if item.get("confidence") != "high":
            suffix = f" (confidence={item.get('confidence')}, unsupported={','.join(item.get('unsupported_modes') or []) or '-'})"
        a(f"- {item['repo_label']} / {task_kind}: `{policy}` — {guidance}{suffix}")

    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default=str(DEFAULT_CONFIG))
    parser.add_argument("--binary", default="")
    parser.add_argument("--base-report", default="")
    parser.add_argument("--repo", action="append", default=[], help="Repo id or absolute path. Can be passed multiple times.")
    parser.add_argument("--skip-synthetic", action="store_true")
    parser.add_argument("--skip-real-sessions", action="store_true")
    parser.add_argument("--session-entry-glob", action="append", default=[])
    parser.add_argument("--no-default-session-glob", action="store_true")
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    parser.add_argument("--label", default="")
    args = parser.parse_args()

    config = load_config(Path(args.config).expanduser())
    selected = []
    requested = set(args.repo)
    for repo_cfg in config["representative_repos"]:
        repo_id = common.normalize_repo_id(repo_cfg)
        if requested and repo_id not in requested and repo_cfg["path"] not in requested:
            continue
        selected.append(repo_cfg)

    if requested and not selected:
        raise SystemExit("no representative repos matched --repo filters")

    binary = resolve_binary(args.binary)
    entries = []
    synthetic_failures = []
    if not args.skip_synthetic:
        for repo_cfg in selected:
            try:
                benchmark = run_token_benchmark(repo_cfg["path"], binary)
                entries.extend(normalize_synthetic_entries(repo_cfg, benchmark))
            except subprocess.CalledProcessError as exc:
                synthetic_failures.append(
                    {
                        "repo": repo_cfg["path"],
                        "error": exc.stderr.strip() or exc.stdout.strip() or str(exc),
                    }
                )
    elif args.base_report:
        base_entries, base_failures = load_synthetic_entries_from_report(
            Path(args.base_report),
            selected,
        )
        entries.extend(base_entries)
        synthetic_failures.extend(base_failures)

    if not args.skip_real_sessions:
        patterns = list(args.session_entry_glob)
        if not args.no_default_session_glob:
            patterns = [DEFAULT_SESSION_GLOB, *patterns] if patterns else [DEFAULT_SESSION_GLOB]
        real_entries, duplicate_real_sessions = common.dedupe_real_session_entries(
            common.load_session_entries(patterns, selected)
        )
        entries.extend(real_entries)
    else:
        duplicate_real_sessions = []

    task_summaries = build_task_summaries(entries)
    report = {
        "schema_version": "codelens-harness-eval-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "binary": str(binary),
        "baseline_reference": config["baseline_reference"],
        "representative_repos": selected,
        "entries": entries,
        "duplicate_real_sessions": duplicate_real_sessions,
        "task_summaries": task_summaries,
        "synthetic_failures": synthetic_failures,
    }

    output_json, output_md = default_report_paths(args.label)
    if args.output_json:
        output_json = Path(args.output_json).expanduser()
    if args.output_md:
        output_md = Path(args.output_md).expanduser()
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_json.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    output_md.write_text(render_report(report))

    print(
        json.dumps(
            {
                "report_json": str(output_json),
                "report_markdown": str(output_md),
                "entry_count": len(entries),
                "duplicate_real_session_count": len(duplicate_real_sessions),
                "task_summary_count": len(task_summaries),
                "synthetic_failures": synthetic_failures,
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
