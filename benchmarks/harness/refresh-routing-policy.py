#!/usr/bin/env python3
"""Refresh hybrid routing policy previews from archived real-session entries."""

from __future__ import annotations

import argparse
import json
import subprocess
from collections import defaultdict
from datetime import datetime
from pathlib import Path

import agent_registry as agents
import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
DEFAULT_CONFIG = BENCH_DIR / "harness-eval-config.json"
DEFAULT_BASE_REPORT = (
    Path.home() / ".codex" / "harness" / "reports" / "2026-04-03-codelens-eval-cross-repo-release.json"
)
DEFAULT_SESSION_GLOB = str(Path.home() / ".codex" / "harness" / "reports" / "session-entries" / "*.json")
DEFAULT_REFRESH_REPORT_DIR = Path.home() / ".codex" / "harness" / "reports" / "refreshes"
DEFAULT_REFRESH_STATUS_DIR = Path.home() / ".codex" / "harness" / "reports" / "refresh-status"
DEFAULT_DRIFT_REPORT_DIR = Path.home() / ".codex" / "harness" / "reports" / "drift"
DEFAULT_PREVIEW_POLICY_DIR = Path.home() / ".codex" / "harness" / "policies" / "previews"
DEFAULT_CANONICAL_POLICY = Path(agents.get_agent("codex")["canonical_policy_json"])
DEFAULT_SHARED_CANONICAL_POLICY = Path(agents.SHARED_POLICY["canonical_policy_json"])
DEFAULT_CLAUDE_CANONICAL_POLICY = Path(agents.get_agent("claude")["canonical_policy_json"])
HARNESS_EVAL_SCRIPT = SCRIPT_DIR / "harness-eval.py"
EXPORT_POLICY_SCRIPT = SCRIPT_DIR / "export-routing-policy.py"
APPLY_POLICY_SCRIPT = SCRIPT_DIR / "apply-routing-policy.py"
DEFAULT_REQUIRED_TASK_KINDS = [
    "impact/reviewer",
    "onboarding/planning",
    "refactor preflight",
]
DEFAULT_REQUIRED_AGENTS = agents.required_agents()


def load_entries(paths):
    return common.load_entries(paths)


def resolve_session_entry_paths(patterns):
    return common.resolve_session_entry_paths(patterns)


def resolve_entry_repo_id(entry, repo_map):
    repo_id = entry.get("repo_id")
    if repo_id:
        matched = repo_map.get(repo_id)
        if matched:
            return repo_id
        canonical = common.canonical_repo_key(repo_id)
        for candidate_id, repo_cfg in repo_map.items():
            if canonical in {
                common.canonical_repo_key(candidate_id),
                common.canonical_repo_key(Path(repo_cfg["path"]).name),
            }:
                return candidate_id
    repo_path = entry.get("repo")
    if repo_path:
        for candidate_id, repo_cfg in repo_map.items():
            if Path(repo_cfg["path"]).expanduser().resolve() == Path(repo_path).expanduser().resolve():
                return candidate_id
    return ""


def dedupe_qualifying_real_entries(entries, repo_map):
    total_real_entries = 0
    qualifying_entries = 0
    latest_by_key = {}
    duplicate_buckets = defaultdict(list)
    overall_agent_counts = defaultdict(int)

    for entry in entries:
        if entry.get("source_kind") != "real-session":
            continue
        total_real_entries += 1
        repo_id = resolve_entry_repo_id(entry, repo_map)
        if not repo_id or not qualifying_real_entry(entry):
            continue
        qualifying_entries += 1
        agent = (entry.get("agent") or "unknown").strip().lower()
        overall_agent_counts[agent] += 1
        scenario_id = common.infer_scenario_id(entry)
        key = (
            repo_id,
            entry.get("task_kind", ""),
            agent,
            entry.get("mode", ""),
            scenario_id or "",
        )
        existing = latest_by_key.get(key)
        if existing is None or str(entry.get("_source_path", "")) > str(existing.get("_source_path", "")):
            if existing is not None:
                duplicate_buckets[key].append(existing)
            latest_by_key[key] = entry
        else:
            duplicate_buckets[key].append(entry)

    duplicates = []
    for key, duplicates_list in sorted(duplicate_buckets.items()):
        kept = latest_by_key[key]
        duplicates.append(
            {
                "repo_id": key[0],
                "task_kind": key[1],
                "agent": key[2],
                "mode": key[3],
                "scenario_id": key[4] or None,
                "kept_source_path": kept.get("_source_path"),
                "discarded_source_paths": [item.get("_source_path") for item in duplicates_list],
                "duplicate_count": len(duplicates_list),
            }
        )

    unique_agent_counts = defaultdict(int)
    for entry in latest_by_key.values():
        agent = (entry.get("agent") or "unknown").strip().lower()
        unique_agent_counts[agent] += 1

    return {
        "total_real_entries": total_real_entries,
        "qualifying_real_entries": qualifying_entries,
        "unique_qualifying_real_entries": len(latest_by_key),
        "overall_agent_counts": dict(sorted(overall_agent_counts.items())),
        "unique_overall_agent_counts": dict(sorted(unique_agent_counts.items())),
        "entries": list(latest_by_key.values()),
        "duplicates": duplicates,
    }


def policy_map_by_key(policy):
    global_rules = {
        row["task_kind"]: {
            "recommended_policy": row.get("recommended_policy"),
            "consensus": row.get("consensus"),
            "explanation": row.get("explanation"),
        }
        for row in policy.get("global_rules", [])
    }
    repo_overrides = {
        (row["repo_id"], row["task_kind"]): {
            "recommended_policy": row.get("recommended_policy"),
            "confidence": row.get("confidence"),
            "explanation": row.get("explanation"),
        }
        for row in policy.get("repo_overrides", [])
    }
    return global_rules, repo_overrides


def policy_drift_summary(canonical_policy_path: Path, preview_policy_path: Path):
    if not canonical_policy_path.exists() or not preview_policy_path.exists():
        return {"global_rule_changes": [], "repo_override_changes": []}

    canonical = common.load_json(canonical_policy_path)
    preview = common.load_json(preview_policy_path)
    canonical_globals, canonical_overrides = policy_map_by_key(canonical)
    preview_globals, preview_overrides = policy_map_by_key(preview)

    global_changes = []
    for key in sorted(set(canonical_globals) | set(preview_globals)):
        before = canonical_globals.get(key)
        after = preview_globals.get(key)
        if before != after:
            global_changes.append({"task_kind": key, "before": before, "after": after})

    override_changes = []
    for key in sorted(set(canonical_overrides) | set(preview_overrides)):
        before = canonical_overrides.get(key)
        after = preview_overrides.get(key)
        if before != after:
            override_changes.append({"repo_id": key[0], "task_kind": key[1], "before": before, "after": after})

    return {
        "global_rule_changes": global_changes,
        "repo_override_changes": override_changes,
    }


def aggregate_policy_drift(platform_drifts):
    global_changes = []
    repo_override_changes = []
    for platform, drift in sorted(platform_drifts.items()):
        for row in drift.get("global_rule_changes") or []:
            global_changes.append({"platform": platform, **row})
        for row in drift.get("repo_override_changes") or []:
            repo_override_changes.append({"platform": platform, **row})
    return {
        "global_rule_changes": global_changes,
        "repo_override_changes": repo_override_changes,
    }


def render_drift_markdown(drift, canonical_policy_path: Path, preview_policy_path: Path):
    lines = [
        "# Routing Policy Drift",
        "",
        f"- Canonical: `{canonical_policy_path}`",
        f"- Preview: `{preview_policy_path}`",
        f"- Global rule changes: `{len(drift['global_rule_changes'])}`",
        f"- Repo override changes: `{len(drift['repo_override_changes'])}`",
        "",
    ]
    if not drift["global_rule_changes"] and not drift["repo_override_changes"]:
        lines.append("No policy drift detected.")
        return "\n".join(lines) + "\n"

    if drift["global_rule_changes"]:
        lines.extend(["## Global Rules", ""])
        for row in drift["global_rule_changes"]:
            platform = row.get("platform")
            prefix = f"[{platform}] " if platform else ""
            lines.append(f"- {prefix}`{row['task_kind']}`")
            lines.append(f"  - before: `{row['before']}`")
            lines.append(f"  - after: `{row['after']}`")
        lines.append("")

    if drift["repo_override_changes"]:
        lines.extend(["## Repo Overrides", ""])
        for row in drift["repo_override_changes"]:
            platform = row.get("platform")
            prefix = f"[{platform}] " if platform else ""
            lines.append(f"- {prefix}`{row['repo_id']} / {row['task_kind']}`")
            lines.append(f"  - before: `{row['before']}`")
            lines.append(f"  - after: `{row['after']}`")
        lines.append("")

    return "\n".join(lines)


def render_refresh_status_markdown(payload):
    lines = [
        "# Routing Policy Refresh",
        "",
        f"- Generated: `{payload['generated_at']}`",
        f"- Ready for promotion: `{payload['ready_for_promotion']}`",
        f"- Promoted: `{payload['promoted']}`",
        f"- Unique qualifying real entries: `{payload['unique_qualifying_real_entries']}`",
        f"- Duplicate real session count: `{payload['duplicate_real_session_count']}`",
        "",
    ]
    if payload["missing_coverage"] or payload["missing_agent_coverage"]:
        lines.extend(["## Gaps", ""])
        for row in payload["missing_coverage"]:
            lines.append(f"- coverage: `{row['repo_id']} / {row['task_kind']}`")
        for row in payload["missing_agent_coverage"]:
            lines.append(f"- agent coverage: `{row['repo_id']} / {row['task_kind']}` missing `{row['missing_agents']}`")
        lines.append("")
    else:
        lines.append("No active coverage gaps.")
        lines.append("")
    return "\n".join(lines)


def qualifying_real_entry(entry):
    if entry.get("source_kind") != "real-session":
        return False
    if entry.get("success") is not True:
        return False
    if entry.get("acceptance_passed") is False:
        return False
    if entry.get("verify_passed") is False:
        return False
    return True


def coverage_summary(config, entries, required_task_kinds, min_per_combo, required_agents):
    repo_map = {
        common.normalize_repo_id(repo_cfg): repo_cfg
        for repo_cfg in config.get("representative_repos", [])
    }
    deduped = dedupe_qualifying_real_entries(entries, repo_map)
    counts = defaultdict(int)
    agent_counts = defaultdict(lambda: defaultdict(int))
    for entry in deduped["entries"]:
        repo_id = resolve_entry_repo_id(entry, repo_map)
        task_kind = entry.get("task_kind", "")
        counts[(repo_id, task_kind)] += 1
        agent = (entry.get("agent") or "unknown").strip().lower()
        agent_counts[(repo_id, task_kind)][agent] += 1

    coverage = []
    missing = []
    missing_agent_coverage = []
    for repo_id, repo_cfg in sorted(repo_map.items()):
        for task_kind in required_task_kinds:
            count = counts[(repo_id, task_kind)]
            present_agents = sorted(agent_counts[(repo_id, task_kind)].keys())
            missing_agents = [agent for agent in required_agents if agent not in agent_counts[(repo_id, task_kind)]]
            ready = count >= min_per_combo and not missing_agents
            row = {
                "repo_id": repo_id,
                "repo_label": repo_cfg.get("label", repo_id),
                "task_kind": task_kind,
                "count": count,
                "required": min_per_combo,
                "required_agents": required_agents,
                "agents_present": present_agents,
                "missing_agents": missing_agents,
                "agent_counts": dict(sorted(agent_counts[(repo_id, task_kind)].items())),
                "ready": ready,
            }
            coverage.append(row)
            if count < min_per_combo:
                missing.append(row)
            if missing_agents:
                missing_agent_coverage.append(row)
    return {
        "total_real_entries": deduped["total_real_entries"],
        "qualifying_real_entries": deduped["qualifying_real_entries"],
        "unique_qualifying_real_entries": deduped["unique_qualifying_real_entries"],
        "overall_agent_counts": deduped["overall_agent_counts"],
        "unique_overall_agent_counts": deduped["unique_overall_agent_counts"],
        "duplicate_real_sessions": deduped["duplicates"],
        "coverage": coverage,
        "missing": missing,
        "missing_agent_coverage": missing_agent_coverage,
        "ready": not missing and not missing_agent_coverage,
    }


def run_json_command(cmd):
    proc = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return json.loads(proc.stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default=str(DEFAULT_CONFIG))
    parser.add_argument("--base-report", default=str(DEFAULT_BASE_REPORT))
    parser.add_argument("--session-entry-glob", action="append", default=[])
    parser.add_argument("--min-real-sessions-per-combo", type=int, default=1)
    parser.add_argument("--required-task-kind", action="append", default=[])
    parser.add_argument("--required-agent", action="append", default=[])
    parser.add_argument("--refresh-report-dir", default=str(DEFAULT_REFRESH_REPORT_DIR))
    parser.add_argument("--refresh-status-dir", default=str(DEFAULT_REFRESH_STATUS_DIR))
    parser.add_argument("--drift-report-dir", default=str(DEFAULT_DRIFT_REPORT_DIR))
    parser.add_argument("--preview-policy-dir", default=str(DEFAULT_PREVIEW_POLICY_DIR))
    parser.add_argument("--label", default="routing-policy-refresh")
    parser.add_argument("--skip-promote", action="store_true")
    args = parser.parse_args()

    config_path = Path(args.config).expanduser()
    base_report_path = Path(args.base_report).expanduser()
    refresh_report_dir = Path(args.refresh_report_dir).expanduser()
    refresh_status_dir = Path(args.refresh_status_dir).expanduser()
    drift_report_dir = Path(args.drift_report_dir).expanduser()
    preview_policy_dir = Path(args.preview_policy_dir).expanduser()
    refresh_report_dir.mkdir(parents=True, exist_ok=True)
    refresh_status_dir.mkdir(parents=True, exist_ok=True)
    drift_report_dir.mkdir(parents=True, exist_ok=True)
    preview_policy_dir.mkdir(parents=True, exist_ok=True)

    required_task_kinds = args.required_task_kind or list(DEFAULT_REQUIRED_TASK_KINDS)
    required_agents = [agent.strip().lower() for agent in (args.required_agent or list(DEFAULT_REQUIRED_AGENTS)) if agent.strip()]
    session_patterns = args.session_entry_glob or [DEFAULT_SESSION_GLOB]
    session_paths = resolve_session_entry_paths(session_patterns)
    real_entries = load_entries(session_paths)
    config = common.load_json(config_path)
    coverage = coverage_summary(
        config,
        real_entries,
        required_task_kinds,
        args.min_real_sessions_per_combo,
        required_agents,
    )

    stamp = datetime.now().strftime("%Y-%m-%d-%H%M%S")
    base_name = f"{stamp}-{common.slugify(args.label)}"
    preview_report_json = refresh_report_dir / f"{base_name}.json"
    preview_report_md = refresh_report_dir / f"{base_name}.md"
    refresh_status_json = refresh_status_dir / f"{base_name}.json"
    refresh_status_md = refresh_status_dir / f"{base_name}.md"
    preview_policy_json = preview_policy_dir / f"{base_name}.json"
    preview_policy_md = preview_policy_dir / f"{base_name}.md"

    harness_cmd = [
        "python3",
        str(HARNESS_EVAL_SCRIPT),
        "--config",
        str(config_path),
        "--skip-synthetic",
        "--base-report",
        str(base_report_path),
        "--no-default-session-glob",
        "--output-json",
        str(preview_report_json),
        "--output-md",
        str(preview_report_md),
        "--label",
        f"{args.label}-preview",
    ]
    for pattern in session_patterns:
        harness_cmd.extend(["--session-entry-glob", pattern])
    harness_result = run_json_command(harness_cmd)

    export_preview_cmd = [
        "python3",
        str(EXPORT_POLICY_SCRIPT),
        "--input",
        str(preview_report_json),
        "--output-json",
        str(preview_policy_json),
        "--output-md",
        str(preview_policy_md),
        "--label",
        f"{args.label}-preview",
        "--skip-canonical-write",
    ]
    preview_policy_result = run_json_command(export_preview_cmd)
    platform_policy_drift = {
        "shared": policy_drift_summary(
            DEFAULT_SHARED_CANONICAL_POLICY,
            Path(preview_policy_result["shared_policy_json"]).expanduser(),
        )
    }
    for agent in agents.agent_names():
        platform_policy_drift[agent] = policy_drift_summary(
            Path(agents.get_agent(agent)["canonical_policy_json"]),
            Path(preview_policy_result[f"{agent}_policy_json"]).expanduser(),
        )
    policy_drift = aggregate_policy_drift(platform_policy_drift)
    drift_report_json = drift_report_dir / f"{base_name}.json"
    drift_report_md = drift_report_dir / f"{base_name}.md"
    drift_payload = {
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "canonical_policy_json": str(DEFAULT_CANONICAL_POLICY),
        "preview_policy_json": str(preview_policy_json),
        "canonical_shared_policy_json": str(DEFAULT_SHARED_CANONICAL_POLICY),
        "canonical_claude_policy_json": str(DEFAULT_CLAUDE_CANONICAL_POLICY),
        "canonical_agent_policy_jsons": {
            agent: str(agents.get_agent(agent)["canonical_policy_json"])
            for agent in agents.agent_names()
        },
        "global_rule_change_count": len(policy_drift["global_rule_changes"]),
        "repo_override_change_count": len(policy_drift["repo_override_changes"]),
        "changed": bool(policy_drift["global_rule_changes"] or policy_drift["repo_override_changes"]),
        "policy_drift": policy_drift,
        "platform_policy_drift": platform_policy_drift,
    }
    drift_report_json.write_text(json.dumps(drift_payload, ensure_ascii=False, indent=2) + "\n")
    drift_report_md.write_text(
        render_drift_markdown(
            policy_drift,
            DEFAULT_SHARED_CANONICAL_POLICY,
            Path(preview_policy_result["shared_policy_json"]).expanduser(),
        )
    )

    promote = coverage["ready"] and not args.skip_promote
    promoted_policy_result = None
    apply_result = None
    if promote:
        promoted_policy_result = run_json_command(
            [
                "python3",
                str(EXPORT_POLICY_SCRIPT),
                "--input",
                str(preview_report_json),
                "--label",
                args.label,
            ]
        )
        apply_result = run_json_command(
            [
                "python3",
                str(APPLY_POLICY_SCRIPT),
                "--policy",
                str(DEFAULT_CANONICAL_POLICY),
                "--claude-policy",
                str(DEFAULT_CLAUDE_CANONICAL_POLICY),
                "--bootstrap-missing-agents",
                "--bootstrap-missing-claude",
            ]
        )

    payload = {
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "base_report": str(base_report_path),
        "session_entry_globs": session_patterns,
        "resolved_session_entries": [str(path) for path in session_paths],
        "required_task_kinds": required_task_kinds,
        "required_agents": required_agents,
        "min_real_sessions_per_combo": args.min_real_sessions_per_combo,
        "coverage": coverage["coverage"],
        "missing_coverage": coverage["missing"],
        "missing_agent_coverage": coverage["missing_agent_coverage"],
        "total_real_entries": coverage["total_real_entries"],
        "qualifying_real_entries": coverage["qualifying_real_entries"],
        "unique_qualifying_real_entries": coverage["unique_qualifying_real_entries"],
        "overall_agent_counts": coverage["overall_agent_counts"],
        "unique_overall_agent_counts": coverage["unique_overall_agent_counts"],
        "duplicate_real_session_count": len(coverage["duplicate_real_sessions"]),
        "duplicate_real_sessions": coverage["duplicate_real_sessions"],
        "policy_drift": policy_drift,
        "platform_policy_drift": platform_policy_drift,
        "drift_report_json": str(drift_report_json),
        "drift_report_markdown": str(drift_report_md),
        "ready_for_promotion": coverage["ready"],
        "promoted": promote,
        "preview_report_json": str(preview_report_json),
        "preview_report_markdown": str(preview_report_md),
        "preview_policy_json": str(preview_policy_json),
        "preview_policy_markdown": str(preview_policy_md),
        "preview_shared_policy_json": preview_policy_result.get("shared_policy_json"),
        "preview_shared_policy_markdown": preview_policy_result.get("shared_policy_markdown"),
        "preview_agent_policy_jsons": {
            agent: preview_policy_result.get(f"{agent}_policy_json")
            for agent in agents.agent_names()
        },
        "preview_agent_policy_markdowns": {
            agent: preview_policy_result.get(f"{agent}_policy_markdown")
            for agent in agents.agent_names()
        },
        "refresh_status_json": str(refresh_status_json),
        "refresh_status_markdown": str(refresh_status_md),
        "harness_eval_result": harness_result,
        "preview_policy_result": preview_policy_result,
        "promoted_policy_result": promoted_policy_result,
        "apply_result": apply_result,
        "canonical_policy_json": (
            promoted_policy_result.get("canonical_json") if promoted_policy_result else None
        ),
        "canonical_policy_markdown": (
            promoted_policy_result.get("canonical_markdown") if promoted_policy_result else None
        ),
        "shared_canonical_policy_json": (
            promoted_policy_result.get("shared_canonical_json") if promoted_policy_result else None
        ),
        "agent_canonical_policy_jsons": {
            agent: promoted_policy_result.get(f"{agent}_canonical_json") if promoted_policy_result else None
            for agent in agents.agent_names()
        },
    }
    if "codex" in agents.agent_names():
        payload["preview_codex_policy_json"] = payload["preview_agent_policy_jsons"].get("codex")
        payload["preview_codex_policy_markdown"] = payload["preview_agent_policy_markdowns"].get("codex")
        payload["codex_canonical_policy_json"] = payload["agent_canonical_policy_jsons"].get("codex")
    if "claude" in agents.agent_names():
        payload["preview_claude_policy_json"] = payload["preview_agent_policy_jsons"].get("claude")
        payload["preview_claude_policy_markdown"] = payload["preview_agent_policy_markdowns"].get("claude")
        payload["claude_canonical_policy_json"] = payload["agent_canonical_policy_jsons"].get("claude")
    refresh_status_json.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    refresh_status_md.write_text(render_refresh_status_markdown(payload))
    print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
