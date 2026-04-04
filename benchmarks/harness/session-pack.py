#!/usr/bin/env python3
"""Generate real-session capture packs for harness evaluation."""

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path

import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
ROOT = BENCH_DIR.parent
DEFAULT_EVAL_CONFIG = BENCH_DIR / "harness-eval-config.json"
DEFAULT_CATALOG = BENCH_DIR / "harness-session-catalog.json"
DEFAULT_OUTPUT_DIR = Path.home() / ".codex" / "harness" / "reports" / "session-packs"


def find_task(catalog, task_id):
    for task in catalog["tasks"]:
        if task["id"] == task_id:
            return task
    raise KeyError(f"unknown task id: {task_id}")


def build_scenarios(repos, catalog, selected_repos=None, selected_tasks=None, selected_modes=None):
    scenarios = []
    selected_repos = set(selected_repos or [])
    selected_tasks = set(selected_tasks or [])
    selected_modes = set(selected_modes or [])
    for repo in repos:
        repo_id = repo["id"]
        if selected_repos and repo_id not in selected_repos and repo["path"] not in selected_repos:
            continue
        repo_notes = catalog.get("repo_overrides", {}).get(repo_id, {}).get("notes", [])
        for task in catalog["tasks"]:
            task_id = task["id"]
            if selected_tasks and task_id not in selected_tasks:
                continue
            for mode, prompt in task["mode_prompts"].items():
                if selected_modes and mode not in selected_modes:
                    continue
                scenario_id = f"{repo_id}::{task_id}::{mode}"
                scenarios.append(
                    {
                        "scenario_id": scenario_id,
                        "repo_id": repo_id,
                        "repo_label": repo.get("label", repo_id),
                        "repo_path": repo["path"],
                        "stack": repo.get("stack", "unknown"),
                        "task_kind": task_id,
                        "mode": mode,
                        "task_mode": task.get("task_mode", "read-only-eval"),
                        "verification_mode": task.get("verification_mode", "smallest-relevant"),
                        "workflow_budget": task.get("workflow_budget", {}),
                        "result_budget": task.get("result_budget", {}),
                        "primary_entrypoint": task.get("primary_entrypoint"),
                        "secondary_entrypoints": task.get("secondary_entrypoints", []),
                        "stop_rule": task.get("stop_rule", ""),
                        "agent_hints": task.get("agent_hints", {}),
                        "goal": task["goal"],
                        "acceptance_criteria": task.get("acceptance_criteria", []),
                        "execution_quality_checks": task.get("execution_quality_checks", []),
                        "verify_commands": repo.get("verify_commands", []),
                        "repo_notes": repo_notes,
                        "prompt": prompt,
                    }
                )
    return scenarios


def render_markdown(scenarios):
    lines = ["# CodeLens Harness Real-Session Pack", ""]
    grouped = {}
    for scenario in scenarios:
        grouped.setdefault((scenario["repo_label"], scenario["repo_path"]), []).append(scenario)
    for (repo_label, repo_path), items in grouped.items():
        lines.extend([f"## {repo_label}", "", f"- Repo: `{repo_path}`", ""])
        for scenario in items:
            lines.extend(
                [
                    f"### {scenario['task_kind']} / {scenario['mode']}",
                    "",
                    f"- Scenario ID: `{scenario['scenario_id']}`",
                    f"- Goal: {scenario['goal']}",
                    f"- Task mode: `{scenario.get('task_mode', 'read-only-eval')}`",
                    f"- Verification mode: `{scenario.get('verification_mode', 'smallest-relevant')}`",
                    "",
                    "Acceptance:",
                ]
            )
            if scenario.get("workflow_budget"):
                lines.extend(["", "Workflow budget:"])
                for key, value in scenario["workflow_budget"].items():
                    lines.append(f"- {key}: `{value}`")
            if scenario.get("result_budget"):
                lines.extend(["", "Result budget:"])
                for key, value in scenario["result_budget"].items():
                    lines.append(f"- {key}: `{value}`")
            if scenario.get("primary_entrypoint") or scenario.get("secondary_entrypoints"):
                lines.extend(["", "Entrypoints:"])
                if scenario.get("primary_entrypoint"):
                    lines.append(f"- primary: `{scenario['primary_entrypoint']}`")
                for item in scenario.get("secondary_entrypoints", []):
                    lines.append(f"- secondary: `{item}`")
            if scenario.get("stop_rule"):
                lines.extend(["", "Stop rule:", f"- {scenario['stop_rule']}"])
            lines.extend([f"- {item}" for item in scenario["acceptance_criteria"]])
            lines.extend(["", "Execution quality:"])
            lines.extend([f"- {item}" for item in scenario["execution_quality_checks"]])
            if scenario["repo_notes"]:
                lines.extend(["", "Repo notes:"])
                lines.extend([f"- {item}" for item in scenario["repo_notes"]])
            lines.extend(["", "Prompt:"])
            lines.append(scenario["prompt"])
            lines.extend(["", "Verify commands:"])
            lines.extend([f"- `{cmd}`" for cmd in scenario["verify_commands"]])
            lines.extend(
                [
                    "",
                    "Capture:",
                    "- Run the task in Codex or Claude.",
                    "- At session end, call `get_tool_metrics` and save the JSON payload.",
                    "- Normalize with `benchmarks/session-eval.py --scenario-file <pack.json> --scenario-id <scenario-id> ...`.",
                    "",
                ]
            )
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--eval-config", default=str(DEFAULT_EVAL_CONFIG))
    parser.add_argument("--catalog", default=str(DEFAULT_CATALOG))
    parser.add_argument("--repo", action="append", default=[])
    parser.add_argument("--task-kind", action="append", default=[])
    parser.add_argument("--mode", action="append", default=[])
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    parser.add_argument("--label", default="real-session-pack")
    args = parser.parse_args()

    eval_config = common.load_json(Path(args.eval_config).expanduser())
    catalog = common.load_json(Path(args.catalog).expanduser())
    scenarios = build_scenarios(
        eval_config["representative_repos"],
        catalog,
        selected_repos=args.repo,
        selected_tasks=args.task_kind,
        selected_modes=args.mode,
    )

    pack = {
        "schema_version": "codelens-harness-session-pack-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "scenarios": scenarios,
    }

    DEFAULT_OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y-%m-%d")
    base_name = f"{stamp}-{common.slugify(args.label)}"
    output_json = Path(args.output_json).expanduser() if args.output_json else DEFAULT_OUTPUT_DIR / f"{base_name}.json"
    output_md = Path(args.output_md).expanduser() if args.output_md else DEFAULT_OUTPUT_DIR / f"{base_name}.md"
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_json.write_text(json.dumps(pack, ensure_ascii=False, indent=2) + "\n")
    output_md.write_text(render_markdown(scenarios))

    print(
        json.dumps(
            {
                "pack_json": str(output_json),
                "pack_markdown": str(output_md),
                "scenario_count": len(scenarios),
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
