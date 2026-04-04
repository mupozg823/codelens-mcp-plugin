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


def dedupe_qualifying_real_entries(entries, representative_repos):
    repo_map = {
        common.normalize_repo_id(repo_cfg): repo_cfg
        for repo_cfg in representative_repos
    }
    total_real_entries = 0
    qualifying_entries = 0
    overall_agent_counts = defaultdict(int)
    qualifying_candidates = []

    for entry in entries:
        if entry.get("source_kind") != "real-session":
            continue
        total_real_entries += 1
        repo_id = entry.get("repo_id", "")
        if repo_id not in repo_map or not common.qualifying_real_entry(entry):
            continue
        qualifying_entries += 1
        normalized = dict(entry)
        normalized["repo_id"] = repo_id
        normalized["repo_label"] = repo_map[repo_id].get("label", repo_id)
        agent = (entry.get("agent") or "unknown").strip().lower()
        overall_agent_counts[agent] += 1
        qualifying_candidates.append(normalized)

    deduped_entries, duplicates = common.dedupe_real_session_entries(qualifying_candidates)

    unique_agent_counts = defaultdict(int)
    for entry in deduped_entries:
        agent = (entry.get("agent") or "unknown").strip().lower()
        unique_agent_counts[agent] += 1

    return {
        "total_real_entries": total_real_entries,
        "qualifying_real_entries": qualifying_entries,
        "unique_qualifying_real_entries": len(deduped_entries),
        "overall_agent_counts": dict(sorted(overall_agent_counts.items())),
        "unique_overall_agent_counts": dict(sorted(unique_agent_counts.items())),
        "entries": deduped_entries,
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


def agent_policy_divergence(shared_policy_path: Path, agent_policy_paths: dict[str, str | Path]):
    if not shared_policy_path.exists():
        return {"changed": False, "global_rule_changes": [], "repo_override_changes": []}

    shared_policy = common.load_json(shared_policy_path)
    shared_globals, shared_overrides = policy_map_by_key(shared_policy)
    global_changes = []
    repo_override_changes = []

    for agent, path_value in sorted(agent_policy_paths.items()):
        policy_path = Path(path_value).expanduser()
        if not policy_path.exists():
            continue
        agent_policy = common.load_json(policy_path)
        agent_globals, agent_overrides = policy_map_by_key(agent_policy)
        for key in sorted(set(shared_globals) | set(agent_globals)):
            shared_row = shared_globals.get(key)
            agent_row = agent_globals.get(key)
            if shared_row != agent_row:
                global_changes.append(
                    {
                        "agent": agent,
                        "task_kind": key,
                        "shared": shared_row,
                        "agent_policy": agent_row,
                    }
                )
        for key in sorted(set(shared_overrides) | set(agent_overrides)):
            shared_row = shared_overrides.get(key)
            agent_row = agent_overrides.get(key)
            if shared_row != agent_row:
                repo_override_changes.append(
                    {
                        "agent": agent,
                        "repo_id": key[0],
                        "task_kind": key[1],
                        "shared": shared_row,
                        "agent_policy": agent_row,
                    }
                )

    return {
        "changed": bool(global_changes or repo_override_changes),
        "global_rule_changes": global_changes,
        "repo_override_changes": repo_override_changes,
    }


def promotion_integrity_summary(preview_policy_paths: dict[str, str | Path], canonical_policy_paths: dict[str, str | Path]):
    comparisons = {}
    mismatches = []
    for label, preview_path_value in sorted(preview_policy_paths.items()):
        preview_path = Path(preview_path_value).expanduser()
        canonical_path_value = canonical_policy_paths.get(label)
        canonical_path = Path(canonical_path_value).expanduser() if canonical_path_value else None
        preview_exists = preview_path.exists()
        canonical_exists = canonical_path.exists() if canonical_path else False
        matches = False
        if preview_exists and canonical_exists:
            preview_policy = common.load_json(preview_path)
            canonical_policy = common.load_json(canonical_path)
            matches = common.policy_structure(preview_policy) == common.policy_structure(canonical_policy)
        comparisons[label] = {
            "preview_path": str(preview_path),
            "canonical_path": str(canonical_path) if canonical_path else None,
            "preview_exists": preview_exists,
            "canonical_exists": canonical_exists,
            "matches": matches,
        }
        if not matches:
            mismatches.append(label)
    return {
        "checked": True,
        "ok": not mismatches,
        "mismatched_targets": mismatches,
        "comparisons": comparisons,
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
    promotion_integrity = payload.get("promotion_integrity") or {}
    agent_divergence = payload.get("agent_policy_divergence") or {}
    lines = [
        "# Routing Policy Refresh",
        "",
        f"- Generated: `{payload['generated_at']}`",
        f"- Ready for promotion: `{payload['ready_for_promotion']}`",
        f"- Promotion attempted: `{payload.get('promotion_attempted')}`",
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
    if agent_divergence.get("changed"):
        lines.extend(["## Agent Divergence", ""])
        for row in agent_divergence.get("global_rule_changes") or []:
            lines.append(f"- global: `{row['agent']} / {row['task_kind']}`")
        for row in agent_divergence.get("repo_override_changes") or []:
            lines.append(f"- override: `{row['agent']} / {row['repo_id']} / {row['task_kind']}`")
        lines.append("")
    else:
        lines.extend(["## Agent Divergence", "", "No agent-specific divergence from shared preview policy.", ""])
    lines.extend(["## Promotion Integrity", ""])
    if not promotion_integrity.get("checked"):
        lines.append("- skipped")
        lines.append("")
        return "\n".join(lines)
    lines.append(f"- ok: `{promotion_integrity.get('ok')}`")
    for label, comparison in sorted((promotion_integrity.get("comparisons") or {}).items()):
        lines.append(
            f"- `{label}`: preview_exists=`{comparison['preview_exists']}` canonical_exists=`{comparison['canonical_exists']}` matches=`{comparison['matches']}`"
        )
    lines.append("")
    return "\n".join(lines)


def coverage_summary(config, entries, required_task_kinds, min_per_combo, required_agents):
    repo_map = {
        common.normalize_repo_id(repo_cfg): repo_cfg
        for repo_cfg in config.get("representative_repos", [])
    }
    deduped = dedupe_qualifying_real_entries(entries, config.get("representative_repos", []))
    counts = defaultdict(int)
    agent_counts = defaultdict(lambda: defaultdict(int))
    for entry in deduped["entries"]:
        repo_id = entry.get("repo_id", "")
        if repo_id not in repo_map:
            continue
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

    config = common.load_json(config_path)
    required_task_kinds = args.required_task_kind or list(DEFAULT_REQUIRED_TASK_KINDS)
    required_agents = [agent.strip().lower() for agent in (args.required_agent or list(DEFAULT_REQUIRED_AGENTS)) if agent.strip()]
    session_patterns = args.session_entry_glob or [DEFAULT_SESSION_GLOB]
    session_paths = resolve_session_entry_paths(session_patterns)
    real_entries = common.canonicalize_entry_repo_ids(
        load_entries(session_paths),
        config.get("representative_repos", []),
    )
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
    policy_generated_at = datetime.now().isoformat(timespec="seconds")

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
        "--generated-at",
        policy_generated_at,
    ]
    preview_policy_result = run_json_command(export_preview_cmd)
    preview_agent_policy_jsons = {
        agent: preview_policy_result.get(f"{agent}_policy_json")
        for agent in agents.agent_names()
    }
    agent_policy_divergence_summary = agent_policy_divergence(
        Path(preview_policy_result["shared_policy_json"]).expanduser(),
        preview_agent_policy_jsons,
    )
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
    promotion_attempted = promote
    promoted_policy_result = None
    apply_result = None
    promotion_integrity = {"checked": False, "ok": None, "mismatched_targets": [], "comparisons": {}}
    if promotion_attempted:
        promoted_policy_result = run_json_command(
            [
                "python3",
                str(EXPORT_POLICY_SCRIPT),
                "--input",
                str(preview_report_json),
                "--label",
                args.label,
                "--generated-at",
                policy_generated_at,
            ]
        )
        promotion_integrity = promotion_integrity_summary(
            {
                "shared": preview_policy_result["shared_policy_json"],
                **preview_agent_policy_jsons,
            },
            {
                "shared": promoted_policy_result.get("shared_canonical_json"),
                **{
                    agent: promoted_policy_result.get(f"{agent}_canonical_json")
                    for agent in agents.agent_names()
                },
            },
        )
        if promotion_integrity["ok"]:
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
        else:
            promote = False

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
        "agent_policy_divergence": agent_policy_divergence_summary,
        "promotion_integrity": promotion_integrity,
        "drift_report_json": str(drift_report_json),
        "drift_report_markdown": str(drift_report_md),
        "ready_for_promotion": coverage["ready"],
        "promotion_attempted": promotion_attempted,
        "promoted": promote,
        "preview_report_json": str(preview_report_json),
        "preview_report_markdown": str(preview_report_md),
        "preview_policy_json": str(preview_policy_json),
        "preview_policy_markdown": str(preview_policy_md),
        "preview_shared_policy_json": preview_policy_result.get("shared_policy_json"),
        "preview_shared_policy_markdown": preview_policy_result.get("shared_policy_markdown"),
        "preview_agent_policy_jsons": preview_agent_policy_jsons,
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
