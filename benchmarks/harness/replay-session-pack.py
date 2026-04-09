#!/usr/bin/env python3
"""Replay a harness session pack into isolated real-session evidence artifacts."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime
from pathlib import Path

import agent_registry as agents
import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
HARNESS_EVAL_SCRIPT = SCRIPT_DIR / "harness-eval.py"
RUNNER_SCRIPTS = {
    "codex": SCRIPT_DIR / "codex-task-runner.py",
    "claude": SCRIPT_DIR / "claude-task-runner.py",
}
DEFAULT_OUTPUT_DIR = Path.home() / ".codex" / "harness" / "reports" / "replays"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--session-pack-json", required=True)
    parser.add_argument("--agent", default="codex", choices=sorted(RUNNER_SCRIPTS))
    parser.add_argument("--binary", default="")
    parser.add_argument("--scenario-id", action="append", default=[])
    parser.add_argument("--repo", action="append", default=[])
    parser.add_argument("--task-kind", action="append", default=[])
    parser.add_argument("--mode", action="append", default=[])
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--label", default="session-pack-replay")
    parser.add_argument("--output-dir", default="")
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def slugify(value: str) -> str:
    chars = []
    for char in value.lower():
        chars.append(char if char.isalnum() else "-")
    slug = "".join(chars)
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-") or "scenario"


def load_pack(path: Path) -> dict:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != "codelens-harness-session-pack-v1":
        raise SystemExit(f"unsupported session pack schema: {payload.get('schema_version')}")
    return payload


def filter_scenarios(payload: dict, args) -> list[dict]:
    selected_ids = set(args.scenario_id)
    selected_repos = set(args.repo)
    selected_tasks = set(args.task_kind)
    selected_modes = set(args.mode)
    scenarios = []
    for scenario in payload.get("scenarios", []):
        if selected_ids and scenario.get("scenario_id") not in selected_ids:
            continue
        if selected_repos:
            repo_values = {
                scenario.get("repo_id"),
                scenario.get("repo_label"),
                scenario.get("repo_path"),
            }
            if not any(value in selected_repos for value in repo_values if value):
                continue
        if selected_tasks and scenario.get("task_kind") not in selected_tasks:
            continue
        if selected_modes and scenario.get("mode") not in selected_modes:
            continue
        scenarios.append(scenario)
    scenarios.sort(key=lambda item: item.get("scenario_id") or "")
    if args.limit > 0:
        scenarios = scenarios[: args.limit]
    if not scenarios:
        raise SystemExit("no scenarios matched the requested replay filters")
    return scenarios


def build_output_paths(args) -> tuple[Path, Path, Path]:
    if args.output_dir:
        output_dir = Path(args.output_dir).expanduser().resolve()
    else:
        stamp = datetime.now().strftime("%Y-%m-%d-%H%M%S")
        output_dir = DEFAULT_OUTPUT_DIR / f"{stamp}-{slugify(args.label)}"
    output_dir.mkdir(parents=True, exist_ok=True)
    output_json = (
        Path(args.output_json).expanduser().resolve()
        if args.output_json
        else output_dir / "replay-summary.json"
    )
    output_md = (
        Path(args.output_md).expanduser().resolve()
        if args.output_md
        else output_dir / "replay-summary.md"
    )
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    return output_dir, output_json, output_md


def build_runner_command(
    *,
    scenario_pack: Path,
    scenario: dict,
    agent: str,
    binary: str,
    run_dir: Path,
    session_entry_json: Path,
    session_entry_md: Path,
    last_message_file: Path,
) -> list[str]:
    runner = RUNNER_SCRIPTS[agent]
    cmd = [
        "python3",
        str(runner),
        "--scenario-file",
        str(scenario_pack),
        "--scenario-id",
        str(scenario["scenario_id"]),
        "--run-dir",
        str(run_dir),
        "--session-entry-json",
        str(session_entry_json),
        "--session-entry-md",
        str(session_entry_md),
        "--output-last-message",
        str(last_message_file),
        "--capture-eval",
        "--exec",
    ]
    return cmd


def run_command(cmd: list[str]) -> dict:
    result = subprocess.run(
        cmd,
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            "session replay failed\n"
            f"command: {' '.join(cmd)}\n"
            f"stdout:\n{result.stdout}\n\nstderr:\n{result.stderr}"
        )
    payload = {}
    stdout = result.stdout.strip()
    if stdout:
        try:
            payload = json.loads(stdout)
        except json.JSONDecodeError:
            payload = {"stdout": stdout}
    return payload


def run_harness_eval(session_entry_glob: str, output_json: Path, output_md: Path, label: str) -> dict:
    cmd = [
        "python3",
        str(HARNESS_EVAL_SCRIPT),
        "--skip-synthetic",
        "--no-default-session-glob",
        "--session-entry-glob",
        session_entry_glob,
        "--output-json",
        str(output_json),
        "--output-md",
        str(output_md),
        "--label",
        label,
    ]
    return run_command(cmd)


def render_markdown(summary: dict) -> str:
    lines = [
        "# Harness Session Pack Replay",
        "",
        f"- Agent: `{summary['agent']}`",
        f"- Session pack: `{summary['session_pack_json']}`",
        f"- Scenario count: `{summary['scenario_count']}`",
        f"- Executed count: `{summary['executed_count']}`",
        f"- Dry run: `{summary['dry_run']}`",
        "",
    ]
    if summary.get("harness_report_json"):
        lines.extend(
            [
                "## Outputs",
                "",
                f"- Harness report: `{summary['harness_report_json']}`",
                f"- Session entry glob: `{summary['session_entry_glob']}`",
                "",
            ]
        )
    lines.extend(["## Scenarios", ""])
    for item in summary["scenarios"]:
        lines.extend(
            [
                f"### {item['scenario_id']}",
                "",
                f"- Repo: `{item['repo_path']}`",
                f"- Task kind: `{item['task_kind']}`",
                f"- Mode: `{item['mode']}`",
                "```bash",
                " ".join(item["command"]),
                "```",
                "",
            ]
        )
    return "\n".join(lines) + "\n"


def main():
    args = parse_args()
    scenario_pack = Path(args.session_pack_json).expanduser().resolve()
    payload = load_pack(scenario_pack)
    scenarios = filter_scenarios(payload, args)
    output_dir, output_json, output_md = build_output_paths(args)
    session_entries_dir = output_dir / "session-entries"
    runs_dir = output_dir / "runs"
    session_entries_dir.mkdir(parents=True, exist_ok=True)
    runs_dir.mkdir(parents=True, exist_ok=True)

    scenario_results = []
    executed_count = 0
    for index, scenario in enumerate(scenarios, start=1):
        scenario_slug = slugify(
            f"{index:03d}-{scenario.get('repo_id', '')}-{scenario.get('task_kind', '')}-{scenario.get('mode', '')}"
        )
        run_dir = runs_dir / scenario_slug
        session_entry_json = session_entries_dir / f"{scenario_slug}.json"
        session_entry_md = session_entries_dir / f"{scenario_slug}.md"
        last_message_file = run_dir / "last-message.md"
        command = build_runner_command(
            scenario_pack=scenario_pack,
            scenario=scenario,
            agent=args.agent,
            binary=args.binary,
            run_dir=run_dir,
            session_entry_json=session_entry_json,
            session_entry_md=session_entry_md,
            last_message_file=last_message_file,
        )
        item = {
            "scenario_id": scenario["scenario_id"],
            "repo_id": scenario.get("repo_id"),
            "repo_path": scenario.get("repo_path"),
            "task_kind": scenario.get("task_kind"),
            "mode": scenario.get("mode"),
            "command": command,
            "session_entry_json": str(session_entry_json),
            "session_entry_markdown": str(session_entry_md),
            "run_dir": str(run_dir),
        }
        if not args.dry_run:
            run_dir.mkdir(parents=True, exist_ok=True)
            item["runner_result"] = run_command(command)
            executed_count += 1
        scenario_results.append(item)

    harness_report_json = None
    harness_report_md = None
    harness_eval_result = None
    session_entry_glob = str(session_entries_dir / "*.json")
    if not args.dry_run:
        harness_report_json = output_dir / "harness-eval.json"
        harness_report_md = output_dir / "harness-eval.md"
        harness_eval_result = run_harness_eval(
            session_entry_glob,
            harness_report_json,
            harness_report_md,
            label=slugify(args.label),
        )

    summary = {
        "schema_version": "codelens-harness-session-replay-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "agent": args.agent,
        "agent_label": agents.agent_label(args.agent),
        "binary": args.binary or None,
        "session_pack_json": str(scenario_pack),
        "output_dir": str(output_dir),
        "dry_run": args.dry_run,
        "scenario_count": len(scenarios),
        "executed_count": executed_count,
        "session_entry_glob": session_entry_glob,
        "harness_report_json": str(harness_report_json) if harness_report_json else None,
        "harness_report_markdown": str(harness_report_md) if harness_report_md else None,
        "harness_eval_result": harness_eval_result,
        "scenarios": scenario_results,
    }
    output_json.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    output_md.write_text(render_markdown(summary), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
