#!/usr/bin/env python3
"""Plan controlled productivity-study runs without mutating routing policy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Final, TypedDict

from productivity_study_contract import Agent, IndexMode
from productivity_study_execution import (
    StudyExecutionConfig,
    execute_planned_run,
    run_id_for,
)
from productivity_study_runner import StudyTask, expand_run_plan, load_task_pack


DEFAULT_POLICY_PATH: Final = Path(__file__).with_name(
    "productivity-study-routing-policy-v1.json"
)


class PlanRun(TypedDict):
    run_id: str
    sequence_order: int
    task_id: str
    repo_id: str
    task_kind: str
    agent: str
    condition: str
    index_mode: str


def build_plan_payload(
    study_id: str, tasks: tuple[StudyTask, ...], index_mode: IndexMode
) -> dict[str, object]:
    plan = expand_run_plan(tasks, tuple(agent.value for agent in Agent))
    runs: list[PlanRun] = []
    for planned in plan:
        runs.append(
            {
                "run_id": run_id_for(planned, index_mode),
                "sequence_order": planned.sequence_order,
                "task_id": planned.task.task_id,
                "repo_id": planned.task.repo_id,
                "task_kind": planned.task.task_kind,
                "agent": planned.agent.value,
                "condition": planned.condition.value,
                "index_mode": index_mode.value,
            }
        )
    return {
        "schema_version": "productivity-study-v1",
        "study_id": study_id,
        "policy_mutation": "forbidden",
        "run_count": len(runs),
        "runs": runs,
    }


def select_planned_run(tasks: tuple[StudyTask, ...], sequence_order: int):
    plan = expand_run_plan(tasks, tuple(agent.value for agent in Agent))
    for planned in plan:
        if planned.sequence_order == sequence_order:
            return planned
    raise ValueError(f"no planned run at sequence order {sequence_order}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--task-pack",
        type=Path,
        default=Path(__file__).with_name("productivity-study-pilot-v1.json"),
    )
    parser.add_argument("--study-id", default="productivity-study-v1-pilot")
    parser.add_argument("--execute-sequence", type=int)
    parser.add_argument("--index-mode", choices=[mode.value for mode in IndexMode], default="warm")
    parser.add_argument("--artifact-root", type=Path, default=Path.home() / ".codex" / "productivity-studies")
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY_PATH)
    parser.add_argument("--codelens-repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--codelens-binary", type=Path, default=Path(__file__).resolve().parents[2] / "target" / "release" / "codelens-mcp")
    parser.add_argument("--codex-model", default="")
    parser.add_argument("--claude-model", default="")
    parser.add_argument("--timeout-seconds", type=int, default=900)
    args = parser.parse_args()
    tasks = load_task_pack(args.task_pack)
    if args.execute_sequence is not None:
        if not args.codex_model or not args.claude_model:
            parser.error("--codex-model and --claude-model are required for execution")
        payload = execute_planned_run(
            select_planned_run(tasks, args.execute_sequence),
            StudyExecutionConfig(
                study_id=args.study_id,
                artifact_root=args.artifact_root,
                policy_path=args.policy,
                codelens_repo=args.codelens_repo,
                codelens_binary=args.codelens_binary,
                index_mode=IndexMode(args.index_mode),
                codex_model=args.codex_model,
                claude_model=args.claude_model,
                timeout_seconds=args.timeout_seconds,
            ),
        )
    else:
        payload = build_plan_payload(args.study_id, tasks, IndexMode(args.index_mode))
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
