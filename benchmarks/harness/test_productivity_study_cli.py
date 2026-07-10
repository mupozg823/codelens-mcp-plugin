#!/usr/bin/env python3
"""Tests for the deterministic productivity-study command surface."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_cli as cli
from productivity_study_contract import IndexMode
from productivity_study_execution import run_id_for
from productivity_study_runner import load_task_pack


def test_pilot_plan_has_all_conditions_and_no_policy_apply_step() -> None:
    tasks = load_task_pack(Path(__file__).with_name("productivity-study-pilot-v1.json"))

    payload = cli.build_plan_payload("pilot-v1", tasks, IndexMode.WARM)

    assert payload["schema_version"] == "productivity-study-v1"
    assert payload["run_count"] == 48
    assert payload["policy_mutation"] == "forbidden"
    assert {run["condition"] for run in payload["runs"]} == {
        "baseline",
        "naive-on",
        "routed-on",
    }
    assert payload["runs"][0]["sequence_order"] == 0
    assert payload["runs"][0]["run_id"] == run_id_for(
        cli.select_planned_run(tasks, 0), IndexMode.WARM
    )


def test_cold_plan_uses_cold_identity_without_colliding_with_warm() -> None:
    tasks = load_task_pack(Path(__file__).with_name("productivity-study-pilot-v1.json"))

    warm = cli.build_plan_payload("pilot-v1", tasks, IndexMode.WARM)
    cold = cli.build_plan_payload("pilot-v1", tasks, IndexMode.COLD)

    assert all(run["index_mode"] == "cold" for run in cold["runs"])
    assert {run["run_id"] for run in warm["runs"]}.isdisjoint(
        run["run_id"] for run in cold["runs"]
    )


def test_sequence_selector_uses_latin_square_order_without_reordering() -> None:
    tasks = load_task_pack(Path(__file__).with_name("productivity-study-pilot-v1.json"))

    selected = cli.select_planned_run(tasks, 3)

    assert selected.sequence_order == 3
    assert selected.agent.value == "claude"
    assert selected.condition.value == "naive-on"


def test_cli_defaults_to_versioned_productivity_study_policy() -> None:
    expected = Path(__file__).with_name("productivity-study-routing-policy-v1.json")

    assert getattr(cli, "DEFAULT_POLICY_PATH", None) == expected


def main() -> int:
    tests = [
        test_pilot_plan_has_all_conditions_and_no_policy_apply_step,
        test_cold_plan_uses_cold_identity_without_colliding_with_warm,
        test_sequence_selector_uses_latin_square_order_without_reordering,
        test_cli_defaults_to_versioned_productivity_study_policy,
    ]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except Exception as error:
            failures += 1
            print(f"FAIL  {test.__name__}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
