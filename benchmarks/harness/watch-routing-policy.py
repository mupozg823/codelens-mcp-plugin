#!/usr/bin/env python3
"""Summarize recent routing policy stability from refresh and drift artifacts."""

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path

import agent_registry as agents
import harness_eval_common as common


DEFAULT_REFRESH_DIR = Path.home() / ".codex" / "harness" / "reports" / "refresh-status"
DEFAULT_DRIFT_DIR = Path.home() / ".codex" / "harness" / "reports" / "drift"
DEFAULT_WATCH_DIR = Path.home() / ".codex" / "harness" / "reports" / "watch"
DEFAULT_CANONICAL_POLICY = Path(agents.get_agent("codex")["canonical_policy_json"])


def list_json_files(directory: Path):
    if not directory.exists():
        return []
    return sorted(
        [path for path in directory.iterdir() if path.is_file() and path.suffix == ".json"],
        key=lambda path: path.name,
    )


def drift_lookup(paths):
    lookup = {}
    for path in paths:
        data = common.load_json(path)
        lookup[path.name] = data
    return lookup


def refresh_rows(refresh_paths, drift_by_name):
    rows = []
    for path in refresh_paths:
        data = common.load_json(path)
        drift = drift_by_name.get(path.name, {})
        policy_drift = data.get("policy_drift") or drift.get("policy_drift") or {
            "global_rule_changes": [],
            "repo_override_changes": [],
        }
        rows.append(
            {
                "name": path.name,
                "path": str(path),
                "generated_at": data.get("generated_at"),
                "promoted": bool(data.get("promoted")),
                "ready_for_promotion": bool(data.get("ready_for_promotion")),
                "duplicate_real_session_count": int(data.get("duplicate_real_session_count") or 0),
                "unique_qualifying_real_entries": int(data.get("unique_qualifying_real_entries") or 0),
                "missing_coverage_count": len(data.get("missing_coverage") or []),
                "missing_agent_coverage_count": len(data.get("missing_agent_coverage") or []),
                "global_rule_change_count": len(policy_drift.get("global_rule_changes") or []),
                "repo_override_change_count": len(policy_drift.get("repo_override_changes") or []),
                "policy_drift": policy_drift,
            }
        )
    return rows


def stable_refresh(row):
    return (
        row["promoted"]
        and row["ready_for_promotion"]
        and row["duplicate_real_session_count"] == 0
        and row["missing_coverage_count"] == 0
        and row["missing_agent_coverage_count"] == 0
        and row["global_rule_change_count"] == 0
        and row["repo_override_change_count"] == 0
    )


def stable_since(rows):
    suffix = []
    for row in reversed(rows):
        if not stable_refresh(row):
            break
        suffix.append(row)
    if not suffix:
        return None
    return suffix[-1]["generated_at"]


def canonical_policy_summary(path: Path):
    if not path.exists():
        return {
            "path": str(path),
            "exists": False,
            "global_rule_count": 0,
            "repo_override_count": 0,
        }
    data = common.load_json(path)
    return {
        "path": str(path),
        "exists": True,
        "global_rule_count": len(data.get("global_rules") or []),
        "repo_override_count": len(data.get("repo_overrides") or []),
    }


def flapping_summary(rows):
    task_kinds = set()
    repo_overrides = set()
    for row in rows:
        for change in row["policy_drift"].get("global_rule_changes") or []:
            task_kinds.add(change.get("task_kind", ""))
        for change in row["policy_drift"].get("repo_override_changes") or []:
            repo_overrides.add((change.get("repo_id", ""), change.get("task_kind", "")))
    return {
        "global_task_kinds": sorted(item for item in task_kinds if item),
        "repo_overrides": [
            {"repo_id": repo_id, "task_kind": task_kind}
            for repo_id, task_kind in sorted(repo_overrides)
            if repo_id and task_kind
        ],
    }


def warnings(rows):
    if not rows:
        return ["No refresh artifacts found."]
    latest = rows[-1]
    issues = []
    if latest["missing_coverage_count"]:
        issues.append("coverage gap remains in latest refresh")
    if latest["missing_agent_coverage_count"]:
        issues.append("mixed-agent coverage gap remains in latest refresh")
    if latest["duplicate_real_session_count"]:
        issues.append("duplicate real-session entries remain active")
    if latest["global_rule_change_count"] or latest["repo_override_change_count"]:
        issues.append("latest preview still drifts from canonical policy")
    if not latest["promoted"]:
        issues.append("latest refresh did not promote canonical policy")
    return issues


def render_markdown(payload):
    lines = [
        "# Routing Policy Watch",
        "",
        f"- Generated: `{payload['generated_at']}`",
        f"- Refreshes analyzed: `{payload['recent_refresh_count']}`",
        f"- Stable now: `{payload['stable_now']}`",
        f"- Stable since: `{payload['stable_since']}`",
        f"- Drift events in window: `{payload['drift_event_count']}`",
        f"- Max duplicate sessions in window: `{payload['max_duplicate_real_session_count']}`",
        "",
    ]
    if payload["warnings"]:
        lines.append("## Warnings")
        lines.append("")
        for warning in payload["warnings"]:
            lines.append(f"- {warning}")
        lines.append("")
    else:
        lines.append("No active warnings.")
        lines.append("")

    lines.extend(
        [
            "## Canonical Policy",
            "",
            f"- Path: `{payload['canonical_policy']['path']}`",
            f"- Exists: `{payload['canonical_policy']['exists']}`",
            f"- Global rules: `{payload['canonical_policy']['global_rule_count']}`",
            f"- Repo overrides: `{payload['canonical_policy']['repo_override_count']}`",
            "",
        ]
    )
    if payload.get("platform_canonical_policies"):
        lines.extend(["## Platform Canonical Policies", ""])
        for platform, row in sorted(payload["platform_canonical_policies"].items()):
            lines.append(
                f"- `{platform}` path=`{row['path']}` exists=`{row['exists']}` "
                f"global_rules=`{row['global_rule_count']}` repo_overrides=`{row['repo_override_count']}`"
            )
        lines.append("")

    if payload["flapping"]["global_task_kinds"] or payload["flapping"]["repo_overrides"]:
        lines.extend(["## Drifted Rules In Window", ""])
        for task_kind in payload["flapping"]["global_task_kinds"]:
            lines.append(f"- global `{task_kind}`")
        for row in payload["flapping"]["repo_overrides"]:
            lines.append(f"- repo `{row['repo_id']} / {row['task_kind']}`")
        lines.append("")

    lines.extend(["## Recent Refreshes", ""])
    for row in payload["history"]:
        lines.append(
            f"- `{row['generated_at']}` promoted=`{row['promoted']}` ready=`{row['ready_for_promotion']}` "
            f"duplicates=`{row['duplicate_real_session_count']}` drift=`{row['global_rule_change_count'] + row['repo_override_change_count']}`"
        )
    lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--refresh-dir", default=str(DEFAULT_REFRESH_DIR))
    parser.add_argument("--drift-dir", default=str(DEFAULT_DRIFT_DIR))
    parser.add_argument("--canonical-policy", default=str(DEFAULT_CANONICAL_POLICY))
    parser.add_argument("--limit", type=int, default=10)
    parser.add_argument("--output-json")
    parser.add_argument("--output-md")
    parser.add_argument("--write-defaults", action="store_true")
    args = parser.parse_args()

    refresh_dir = Path(args.refresh_dir).expanduser()
    drift_dir = Path(args.drift_dir).expanduser()
    refresh_paths = [path for path in list_json_files(refresh_dir) if path.name.endswith("routing-policy-refresh.json")]
    if args.limit > 0:
        refresh_paths = refresh_paths[-args.limit :]
    drift_by_name = drift_lookup(list_json_files(drift_dir))
    rows = refresh_rows(refresh_paths, drift_by_name)
    drift_events = [
        row
        for row in rows
        if row["global_rule_change_count"] or row["repo_override_change_count"]
    ]
    payload = {
        "schema_version": "codelens-routing-watch-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "refresh_dir": str(refresh_dir),
        "drift_dir": str(drift_dir),
        "recent_refresh_count": len(rows),
        "stable_now": stable_refresh(rows[-1]) if rows else False,
        "stable_since": stable_since(rows),
        "drift_event_count": len(drift_events),
        "max_duplicate_real_session_count": max(
            (row["duplicate_real_session_count"] for row in rows),
            default=0,
        ),
        "canonical_policy": canonical_policy_summary(Path(args.canonical_policy).expanduser()),
        "platform_canonical_policies": {
            "shared": canonical_policy_summary(Path(agents.SHARED_POLICY["canonical_policy_json"])),
            **{
                agent: canonical_policy_summary(Path(agents.get_agent(agent)["canonical_policy_json"]))
                for agent in agents.agent_names()
            },
        },
        "flapping": flapping_summary(rows),
        "warnings": warnings(rows),
        "history": rows,
    }

    output_json = Path(args.output_json).expanduser() if args.output_json else None
    output_md = Path(args.output_md).expanduser() if args.output_md else None
    if args.write_defaults:
        watch_dir = DEFAULT_WATCH_DIR.expanduser()
        watch_dir.mkdir(parents=True, exist_ok=True)
        stamp = datetime.now().strftime("%Y-%m-%d-%H%M%S")
        output_json = output_json or (watch_dir / f"{stamp}-routing-policy-watch.json")
        output_md = output_md or (watch_dir / f"{stamp}-routing-policy-watch.md")
    if output_json:
        output_json.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    if output_md:
        output_md.write_text(render_markdown(payload))

    print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
