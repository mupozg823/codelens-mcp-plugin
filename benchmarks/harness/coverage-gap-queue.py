#!/usr/bin/env python3
"""Generate a collection queue for missing mixed-agent real-session coverage."""

from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter
from datetime import datetime
from pathlib import Path

import agent_registry as agents
import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
SESSION_PACK_SCRIPT = SCRIPT_DIR / "session-pack.py"
REFRESH_SCRIPT = SCRIPT_DIR / "refresh-routing-policy.py"
TASK_BOOTSTRAP_SCRIPT = SCRIPT_DIR / "task-bootstrap.py"
DEFAULT_OUTPUT_DIR = Path.home() / ".codex" / "harness" / "reports" / "coverage-queues"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def build_queue_id(repo_id: str, task_kind: str, agent: str) -> str:
    return f"{repo_id}::{task_kind}::{agent}"


def mode_for_policy(policy_name: str) -> str:
    if policy_name in {"prefer_routed_codelens", "prefer_codelens_after_bootstrap"}:
        return "routed-on"
    if policy_name == "prefer_naive_codelens":
        return "naive-on"
    return "baseline"


def build_command(agent: str, pack_json: Path, scenario_id: str):
    base = [str(agents.get_agent(agent)["wrapper_path"])]
    return base + [
        "--scenario-file",
        str(pack_json),
        "--scenario-id",
        scenario_id,
        "--exec",
        "--capture-eval",
    ]


def render_markdown(queue_items, pack_json: Path):
    lines = [
        "# CodeLens Coverage Gap Queue",
        "",
        f"- Scenario pack: `{pack_json}`",
        f"- Generated at: `{datetime.now().isoformat(timespec='seconds')}`",
        "",
    ]
    by_agent = Counter(item["agent"] for item in queue_items)
    agent_summary = [
        f"- {agents.agent_label(agent)} items: `{by_agent.get(agent, 0)}`"
        for agent in agents.agent_names()
    ]
    lines.extend(
        [
            "## Summary",
            "",
            f"- Total queue items: `{len(queue_items)}`",
            *agent_summary,
            "",
            "## Queue",
            "",
        ]
    )
    for item in queue_items:
        lines.extend(
            [
                f"### {item['repo_label']} / {item['task_kind']} / {item['agent']}",
                "",
                f"- Queue ID: `{item['queue_id']}`",
                f"- Policy: `{item['recommended_policy']}`",
                f"- Scenario: `{item['scenario_id']}`",
                f"- Missing agents for combo: `{', '.join(item['missing_agents'])}`",
                f"- Suggested mode: `{item['mode']}`",
                "",
                "Command:",
                "```bash",
                " ".join(item["command"]),
                "```",
                "",
                "Notes:",
                "- Add `--acceptance-passed`, `--verify-passed`, and `--quality-score` when you have reviewer/evaluator judgment.",
                "- This queue item exists because the current mixed-agent promotion gate is missing this repo/task/agent evidence.",
                "",
            ]
        )
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", default=str(agents.SHARED_POLICY["canonical_policy_json"]))
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    parser.add_argument("--label", default="coverage-gap-queue")
    args = parser.parse_args()

    refresh = load_module(REFRESH_SCRIPT, "refresh_module")
    session_pack = load_module(SESSION_PACK_SCRIPT, "session_pack_module")
    task_bootstrap = load_module(TASK_BOOTSTRAP_SCRIPT, "task_bootstrap_module")

    config = common.load_json(Path(refresh.DEFAULT_CONFIG).expanduser())
    agent_policies = {
        agent: task_bootstrap.load_json(Path(agents.get_agent(agent)["canonical_policy_json"]).expanduser())
        for agent in refresh.DEFAULT_REQUIRED_AGENTS
    }
    session_paths = refresh.resolve_session_entry_paths([refresh.DEFAULT_SESSION_GLOB])
    entries = refresh.load_entries(session_paths)
    coverage = refresh.coverage_summary(
        config,
        entries,
        list(refresh.DEFAULT_REQUIRED_TASK_KINDS),
        1,
        list(refresh.DEFAULT_REQUIRED_AGENTS),
    )

    scenario_requests = []
    for row in coverage["missing_agent_coverage"]:
        for agent in row["missing_agents"]:
            resolved_rule = task_bootstrap.choose_rule(agent_policies[agent], row["repo_id"], row["task_kind"])
            scenario_requests.append(
                {
                    "repo_id": row["repo_id"],
                    "repo_label": row["repo_label"],
                    "task_kind": row["task_kind"],
                    "agent": agent,
                    "recommended_policy": resolved_rule["recommended_policy"],
                    "mode": mode_for_policy(resolved_rule["recommended_policy"]),
                }
            )

    scenarios = session_pack.build_scenarios(
        config["representative_repos"],
        common.load_json(Path(session_pack.DEFAULT_CATALOG).expanduser()),
        selected_repos=sorted({item["repo_id"] for item in scenario_requests}),
        selected_tasks=sorted({item["task_kind"] for item in scenario_requests}),
        selected_modes=sorted({item["mode"] for item in scenario_requests}),
    )
    scenario_map = {
        (scenario["repo_id"], scenario["task_kind"], scenario["mode"]): scenario
        for scenario in scenarios
    }

    DEFAULT_OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y-%m-%d-%H%M%S")
    base_name = f"{stamp}-{common.slugify(args.label)}"
    pack_json = DEFAULT_OUTPUT_DIR / f"{base_name}-pack.json"
    pack_md = DEFAULT_OUTPUT_DIR / f"{base_name}-pack.md"
    queue_json = Path(args.output_json).expanduser() if args.output_json else DEFAULT_OUTPUT_DIR / f"{base_name}.json"
    queue_md = Path(args.output_md).expanduser() if args.output_md else DEFAULT_OUTPUT_DIR / f"{base_name}.md"

    pack_payload = {
        "schema_version": "codelens-harness-session-pack-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "scenarios": scenarios,
    }
    pack_json.write_text(json.dumps(pack_payload, ensure_ascii=False, indent=2) + "\n")
    pack_md.write_text(session_pack.render_markdown(scenarios))

    queue_items = []
    for request in scenario_requests:
        scenario = scenario_map[(request["repo_id"], request["task_kind"], request["mode"])]
        queue_items.append(
            {
                "queue_id": build_queue_id(request["repo_id"], request["task_kind"], request["agent"]),
                "repo_id": request["repo_id"],
                "repo_label": request["repo_label"],
                "repo_path": scenario["repo_path"],
                "task_kind": request["task_kind"],
                "agent": request["agent"],
                "mode": request["mode"],
                "recommended_policy": request["recommended_policy"],
                "missing_agents": [request["agent"]],
                "scenario_id": scenario["scenario_id"],
                "scenario_pack_json": str(pack_json),
                "command": build_command(request["agent"], pack_json, scenario["scenario_id"]),
            }
        )

    queue_payload = {
        "schema_version": "codelens-coverage-gap-queue-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "policy_path": str(Path(args.policy).expanduser()),
        "agent_policy_paths": {
            agent: str(agents.get_agent(agent)["canonical_policy_json"])
            for agent in refresh.DEFAULT_REQUIRED_AGENTS
        },
        "session_entry_glob": refresh.DEFAULT_SESSION_GLOB,
        "required_task_kinds": list(refresh.DEFAULT_REQUIRED_TASK_KINDS),
        "required_agents": list(refresh.DEFAULT_REQUIRED_AGENTS),
        "coverage_snapshot": {
            "overall_agent_counts": coverage["overall_agent_counts"],
            "missing_agent_coverage_count": len(coverage["missing_agent_coverage"]),
        },
        "scenario_pack_json": str(pack_json),
        "scenario_pack_markdown": str(pack_md),
        "queue": queue_items,
    }
    queue_json.write_text(json.dumps(queue_payload, ensure_ascii=False, indent=2) + "\n")
    queue_md.write_text(render_markdown(queue_items, pack_json))

    print(
        json.dumps(
            {
                "queue_json": str(queue_json),
                "queue_markdown": str(queue_md),
                "scenario_pack_json": str(pack_json),
                "scenario_pack_markdown": str(pack_md),
                "queue_count": len(queue_items),
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
