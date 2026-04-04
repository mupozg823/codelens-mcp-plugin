#!/usr/bin/env python3
"""Inspect or execute CodeLens mixed-agent coverage queue items."""

from __future__ import annotations

import argparse
import importlib.util
import json
import subprocess
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
REFRESH_SCRIPT = SCRIPT_DIR / "refresh-routing-policy.py"
DEFAULT_QUEUE_DIR = Path.home() / ".codex" / "harness" / "reports" / "coverage-queues"
DEFAULT_SESSION_GLOB = str(Path.home() / ".codex" / "harness" / "reports" / "session-entries" / "*.json")


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def latest_queue_path():
    candidates = sorted(DEFAULT_QUEUE_DIR.glob("*-coverage-gap-queue.json"))
    if not candidates:
        raise SystemExit("No coverage-gap queue found. Run benchmarks/coverage-gap-queue.py first.")
    return candidates[-1]


def build_queue_id(item: dict) -> str:
    return item.get("queue_id") or f"{item['repo_id']}::{item['task_kind']}::{item['agent']}"


def latest_qualifying_entries(refresh, patterns):
    session_paths = refresh.resolve_session_entry_paths(patterns)
    entries = refresh.load_entries(session_paths)
    config = refresh.load_json(Path(refresh.DEFAULT_CONFIG).expanduser())
    repo_map = {
        refresh.normalize_repo_id(repo_cfg): repo_cfg
        for repo_cfg in config.get("representative_repos", [])
    }
    latest = {}
    for entry in entries:
        if not refresh.qualifying_real_entry(entry):
            continue
        agent = (entry.get("agent") or "unknown").strip().lower()
        repo_id = refresh.resolve_entry_repo_id(entry, repo_map)
        key = (repo_id, entry.get("task_kind"), agent)
        if not key[0] or not key[1]:
            continue
        previous = latest.get(key)
        current_stamp = entry.get("generated_at") or entry.get("captured_at") or ""
        previous_stamp = (previous or {}).get("generated_at") or (previous or {}).get("captured_at") or ""
        if previous is None or current_stamp >= previous_stamp:
            latest[key] = entry
    return latest


def annotate_queue(queue_items, qualifying_lookup):
    annotated = []
    for position, item in enumerate(queue_items, start=1):
        agent = item["agent"].strip().lower()
        key = (item["repo_id"], item["task_kind"], agent)
        matched = qualifying_lookup.get(key)
        annotated.append(
            {
                **item,
                "queue_id": build_queue_id(item),
                "position": position,
                "status": "completed" if matched else "pending",
                "completed_by": matched.get("_source_path") if matched else None,
                "completed_at": (
                    matched.get("generated_at") or matched.get("captured_at") if matched else None
                ),
                "recorded_mode": matched.get("mode") if matched else None,
            }
        )
    return annotated


def filter_items(items, args):
    filtered = items
    if args.pending_only:
        filtered = [item for item in filtered if item["status"] == "pending"]
    if args.agent:
        filtered = [item for item in filtered if item["agent"] == args.agent]
    if args.task_kind:
        filtered = [item for item in filtered if item["task_kind"] == args.task_kind]
    if args.repo_id:
        filtered = [item for item in filtered if item["repo_id"] == args.repo_id]
    if args.queue_id:
        filtered = [item for item in filtered if item["queue_id"] == args.queue_id]
    if args.scenario_id:
        filtered = [item for item in filtered if item["scenario_id"] == args.scenario_id]
    if args.index:
        filtered = [item for item in filtered if item["position"] == args.index]
    return filtered


def group_progress(items):
    by_agent = Counter(item["agent"] for item in items)
    by_status = Counter(item["status"] for item in items)
    by_task = defaultdict(lambda: Counter())
    by_repo = defaultdict(lambda: Counter())
    for item in items:
        by_task[item["task_kind"]][item["status"]] += 1
        by_repo[item["repo_id"]][item["status"]] += 1
    return {
        "by_agent": dict(sorted(by_agent.items())),
        "by_status": dict(sorted(by_status.items())),
        "by_task_kind": {key: dict(sorted(value.items())) for key, value in sorted(by_task.items())},
        "by_repo": {key: dict(sorted(value.items())) for key, value in sorted(by_repo.items())},
    }


def render_markdown(summary: dict):
    lines = [
        "# CodeLens Coverage Queue Status",
        "",
        f"- Queue: `{summary['queue_path']}`",
        f"- Generated at: `{datetime.now().isoformat(timespec='seconds')}`",
        f"- Pending: `{summary['pending_count']}`",
        f"- Completed: `{summary['completed_count']}`",
        "",
        "## Progress",
        "",
    ]
    for key, value in summary["progress"]["by_status"].items():
        lines.append(f"- {key}: `{value}`")
    lines.extend(["", "## Next Items", ""])
    preview = summary["selected_items"] or summary["pending_preview"]
    if not preview:
        lines.append("- No matching pending items.")
    else:
        for item in preview:
            lines.extend(
                [
                    f"### {item['position']}. {item['repo_id']} / {item['task_kind']} / {item['agent']}",
                    "",
                    f"- Queue ID: `{item['queue_id']}`",
                    f"- Policy: `{item['recommended_policy']}`",
                    f"- Mode: `{item['mode']}`",
                    f"- Scenario: `{item['scenario_id']}`",
                    f"- Status: `{item['status']}`",
                    "",
                    "Command:",
                    "```bash",
                    " ".join(item["command"]),
                    "```",
                    "",
                ]
            )
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--queue-json", default="")
    parser.add_argument("--session-entry-glob", action="append", default=[])
    parser.add_argument("--queue-id", default="")
    parser.add_argument("--scenario-id", default="")
    parser.add_argument("--index", type=int, default=0)
    parser.add_argument("--agent", default="")
    parser.add_argument("--task-kind", default="")
    parser.add_argument("--repo-id", default="")
    parser.add_argument("--pending-only", action="store_true")
    parser.add_argument("--next", action="store_true")
    parser.add_argument("--limit", type=int, default=5)
    parser.add_argument("--exec", action="store_true")
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    args = parser.parse_args()

    refresh = load_module(REFRESH_SCRIPT, "refresh_module")
    queue_path = Path(args.queue_json).expanduser() if args.queue_json else latest_queue_path()
    queue_payload = common.load_json(queue_path)
    session_patterns = args.session_entry_glob or [DEFAULT_SESSION_GLOB]
    qualifying_lookup = latest_qualifying_entries(refresh, session_patterns)
    items = annotate_queue(queue_payload["queue"], qualifying_lookup)
    filtered = filter_items(items, args)

    selected_items = filtered
    if args.next:
        pending = [item for item in filtered if item["status"] == "pending"]
        selected_items = pending[:1]
    elif not any([args.queue_id, args.scenario_id, args.index, args.agent, args.task_kind, args.repo_id]):
        selected_items = filtered[: args.limit]

    summary = {
        "schema_version": "codelens-coverage-gap-runner-v1",
        "queue_path": str(queue_path),
        "scenario_pack_json": queue_payload.get("scenario_pack_json"),
        "total_count": len(items),
        "pending_count": sum(1 for item in items if item["status"] == "pending"),
        "completed_count": sum(1 for item in items if item["status"] == "completed"),
        "progress": group_progress(items),
        "selected_items": selected_items,
        "pending_preview": [item for item in items if item["status"] == "pending"][: args.limit],
    }

    if args.exec:
        if len(selected_items) != 1:
            raise SystemExit("Execution requires exactly one selected queue item.")
        target = selected_items[0]
        proc = subprocess.run(target["command"], cwd=Path(target["repo_path"]), check=False)
        summary["executed"] = {
            "queue_id": target["queue_id"],
            "command": target["command"],
            "returncode": proc.returncode,
        }
        if proc.returncode != 0:
            raise SystemExit(proc.returncode)

    if args.output_json:
        output_json = Path(args.output_json).expanduser()
        output_json.parent.mkdir(parents=True, exist_ok=True)
        output_json.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    if args.output_md:
        output_md = Path(args.output_md).expanduser()
        output_md.parent.mkdir(parents=True, exist_ok=True)
        output_md.write_text(render_markdown(summary))

    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
