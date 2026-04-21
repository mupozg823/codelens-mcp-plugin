import contextlib
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


HARNESS_DIR = Path(__file__).resolve().parents[1]
HARNESS_PATH = str(HARNESS_DIR)


def load_script_module(module_name: str, path: Path):
    added_path = False
    if HARNESS_PATH not in sys.path:
        sys.path.insert(0, HARNESS_PATH)
        added_path = True
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules[module_name] = module
    try:
        spec.loader.exec_module(module)
    finally:
        if added_path:
            sys.path.remove(HARNESS_PATH)
    return module


RELEASE = load_script_module(
    "release_harness_runner_test",
    HARNESS_DIR / "release-harness-runner.py",
)


def option_value(command: list[str], flag: str) -> str:
    index = command.index(flag)
    return command[index + 1]


def write_stage_files(stage_root: Path, payload: dict) -> None:
    stage_root.mkdir(parents=True, exist_ok=True)
    (stage_root / "run-manifest.json").write_text(
        json.dumps(
            {
                "schema_version": "codelens-harness-run-v1",
                "run_id": stage_root.name,
                "checkpoints": {},
                "artifacts": {},
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    (stage_root / "run-events.jsonl").write_text("", encoding="utf-8")
    (stage_root / "metrics-delta.json").write_text(
        json.dumps(
            {
                "session": {
                    "total_tokens": payload["actual"]["tokens"],
                    "total_ms": payload["actual"]["elapsed_ms"],
                    "total_calls": payload["actual"]["tool_calls"],
                }
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def render_usage_block(kind: str, metrics: dict[str, int]) -> str:
    return (
        f"<usage kind=\"{kind}\">\n"
        f"Total tokens: {metrics['tokens']}\n"
        f"Elapsed: {metrics['elapsed_ms']} ms\n"
        f"Tool calls: {metrics['tool_calls']}\n"
        "</usage>\n"
    )


class ReleaseHarnessRunnerTests(unittest.TestCase):
    def test_parse_usage_blocks_parses_input_output_and_duration(self):
        blocks = RELEASE.parse_usage_blocks(
            """
<usage kind="actual">
Input tokens: 12k
Output tokens: 3k
Elapsed: 00:35
Tool calls: 7
</usage>
<usage kind="self">
Total tokens: 14000
Time: 30s
Tool calls: 6
</usage>
"""
        )

        self.assertEqual(len(blocks), 2)
        self.assertEqual(blocks[0]["kind"], "actual")
        self.assertEqual(blocks[0]["tokens"], 15000)
        self.assertEqual(blocks[0]["elapsed_ms"], 35000)
        self.assertEqual(blocks[0]["tool_calls"], 7)
        self.assertEqual(blocks[1]["kind"], "self")
        self.assertEqual(blocks[1]["tokens"], 14000)

    def test_collect_stage_usage_marks_self_report_without_actual_as_incomplete(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            stage_root = Path(tmpdir)
            (stage_root / "last-message.md").write_text(
                render_usage_block(
                    "self",
                    {"tokens": 33000, "elapsed_ms": 15000, "tool_calls": 1},
                ),
                encoding="utf-8",
            )

            stage_usage = RELEASE.collect_stage_usage(
                {
                    "stage": "orchestrator",
                    "role": "orchestrator",
                    "stage_dir": str(stage_root),
                }
            )
            report = RELEASE.build_usage_drift_report(
                [
                    {
                        "stage": "orchestrator",
                        "role": "orchestrator",
                        "stage_dir": str(stage_root),
                    }
                ]
            )

        self.assertTrue(stage_usage["evidence_incomplete"])
        self.assertTrue(report["release_blocking"])

    def test_main_without_exec_reports_planned_status(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            repo = root / "repo"
            repo.mkdir()
            artifact = repo / "docs" / "release-notes" / "v1.9.53.md"
            artifact.parent.mkdir(parents=True)
            artifact.write_text("# draft\n", encoding="utf-8")

            manifest_path = root / "manifest.json"
            manifest_path.write_text(
                json.dumps(
                    {
                        "task": "Write the v1.9.53 release note.",
                        "artifact_path": str(artifact.relative_to(repo)),
                        "acceptance_criteria": ["Document automated harness."],
                        "roles": {
                            "worker": {"runner": "codex"},
                            "orchestrator": {"runner": "codex"},
                            "evaluator": {"runner": "claude"},
                            "independent_evaluator": {"runner": "claude"},
                        },
                        "repair": {"max_rounds": 0},
                        "runner_defaults": {
                            "repo": str(repo),
                            "task_kind": "release-harness",
                            "mode": "routed-on",
                            "mcp_url": "http://127.0.0.1:7837/mcp",
                        },
                    },
                    ensure_ascii=False,
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            output = io.StringIO()
            run_dir = root / "run"
            with mock.patch.object(sys, "argv", ["release-harness-runner.py", "--manifest", str(manifest_path), "--run-dir", str(run_dir)]):
                with contextlib.redirect_stdout(output):
                    RELEASE.main()

            result = json.loads(output.getvalue())
            manifest = json.loads((run_dir / "run-manifest.json").read_text(encoding="utf-8"))

        self.assertEqual(result["status"], "planned")
        self.assertEqual(manifest["checkpoints"]["worker_scan"]["status"], "planned")
        self.assertEqual(manifest["checkpoints"]["independent_signoff"]["status"], "planned")

    def test_main_exec_records_stage_order_and_reuses_completed_checkpoints(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            repo = root / "repo"
            repo.mkdir()
            artifact = repo / "docs" / "release-notes" / "v1.9.53.md"
            artifact.parent.mkdir(parents=True)
            artifact.write_text("# v1.9.53\n", encoding="utf-8")
            manifest_path = root / "manifest.json"
            run_dir = root / "run"
            manifest_path.write_text(
                json.dumps(
                    {
                        "task": "Write the v1.9.53 release note.",
                        "artifact_path": str(artifact.relative_to(repo)),
                        "acceptance_criteria": [
                            "Document automated harness.",
                            "Mention strict coordination mode.",
                        ],
                        "roles": {
                            "worker": {"runner": "codex"},
                            "orchestrator": {"runner": "codex"},
                            "evaluator": {"runner": "claude"},
                            "independent_evaluator": {"runner": "claude"},
                        },
                        "repair": {"max_rounds": 0},
                        "runner_defaults": {
                            "repo": str(repo),
                            "task_kind": "release-harness",
                            "mode": "routed-on",
                            "mcp_url": "http://127.0.0.1:7837/mcp",
                        },
                    },
                    ensure_ascii=False,
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            stage_payloads = {
                "worker_scan": {
                    "actual": {"tokens": 5000, "elapsed_ms": 1200, "tool_calls": 1},
                    "message": "worker summary\n",
                },
                "orchestrator": {
                    "actual": {"tokens": 33000, "elapsed_ms": 76000, "tool_calls": 1},
                    "self": {"tokens": 3000, "elapsed_ms": 15000, "tool_calls": 1},
                    "message": "orchestrator summary\n",
                },
                "evaluator": {
                    "actual": {"tokens": 32000, "elapsed_ms": 32000, "tool_calls": 0},
                    "message": json.dumps(
                        {
                            "verdict": "PASS",
                            "aggregate_score": 1.0,
                            "summary": "criteria satisfied",
                            "repair_hints": [],
                            "issues": [],
                        }
                    )
                    + "\n",
                },
                "independent_signoff": {
                    "actual": {"tokens": 31000, "elapsed_ms": 26000, "tool_calls": 1},
                    "message": json.dumps(
                        {
                            "verdict": "PASS",
                            "aggregate_score": 1.0,
                            "summary": "independent pass",
                            "repair_hints": [],
                            "issues": [],
                        }
                    )
                    + "\n",
                },
            }
            call_log = []

            def fake_run(command, capture_output=True, text=True):
                stage_root = Path(option_value(command, "--run-dir"))
                stage_name = stage_root.name
                payload = stage_payloads[stage_name]
                call_log.append(stage_name)
                write_stage_files(stage_root, payload)
                last_message_path = Path(option_value(command, "--output-last-message"))
                body = payload["message"] + render_usage_block("actual", payload["actual"])
                if payload.get("self"):
                    body += render_usage_block("self", payload["self"])
                last_message_path.write_text(body, encoding="utf-8")
                return subprocess.CompletedProcess(command, 0, stdout=f"{stage_name}\n", stderr="")

            first_output = io.StringIO()
            argv = [
                "release-harness-runner.py",
                "--manifest",
                str(manifest_path),
                "--run-dir",
                str(run_dir),
                "--exec",
            ]
            with mock.patch.object(RELEASE.subprocess, "run", side_effect=fake_run):
                with mock.patch.object(sys, "argv", argv):
                    with contextlib.redirect_stdout(first_output):
                        RELEASE.main()

            first_result = json.loads(first_output.getvalue())
            parent_manifest = json.loads((run_dir / "run-manifest.json").read_text(encoding="utf-8"))

            self.assertEqual(first_result["status"], "pass")
            self.assertEqual(
                call_log,
                ["worker_scan", "orchestrator", "evaluator", "independent_signoff"],
            )
            self.assertEqual(
                list(parent_manifest["checkpoints"].keys()),
                [
                    "preflight",
                    "worker_scan",
                    "orchestrator",
                    "evaluator",
                    "usage_drift",
                    "independent_signoff",
                    "final_signoff",
                ],
            )
            self.assertIn(
                "child_run_manifest",
                parent_manifest["checkpoints"]["orchestrator"]["artifacts"],
            )
            self.assertTrue((run_dir / "usage-drift.json").exists())
            self.assertTrue((run_dir / "independent-signoff.json").exists())

            second_output = io.StringIO()
            with mock.patch.object(
                RELEASE.subprocess,
                "run",
                side_effect=AssertionError("completed checkpoints should be reused"),
            ):
                with mock.patch.object(sys, "argv", argv):
                    with contextlib.redirect_stdout(second_output):
                        RELEASE.main()

            second_result = json.loads(second_output.getvalue())
            reused_manifest = json.loads((run_dir / "run-manifest.json").read_text(encoding="utf-8"))

        self.assertEqual(second_result["status"], "pass")
        self.assertEqual(
            reused_manifest["checkpoints"]["worker_scan"]["reuse_count"],
            1,
        )
        self.assertEqual(
            reused_manifest["checkpoints"]["independent_signoff"]["reuse_count"],
            1,
        )

    def test_main_exec_fails_on_independent_disagreement(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            repo = root / "repo"
            repo.mkdir()
            artifact = repo / "docs" / "release-notes" / "v1.9.53.md"
            artifact.parent.mkdir(parents=True)
            artifact.write_text("# v1.9.53\n", encoding="utf-8")
            manifest_path = root / "manifest.json"
            run_dir = root / "run"
            manifest_path.write_text(
                json.dumps(
                    {
                        "task": "Write the v1.9.53 release note.",
                        "artifact_path": str(artifact.relative_to(repo)),
                        "acceptance_criteria": ["Document automated harness."],
                        "roles": {
                            "worker": {"runner": "codex"},
                            "orchestrator": {"runner": "codex"},
                            "evaluator": {"runner": "claude"},
                            "independent_evaluator": {"runner": "claude"},
                        },
                        "repair": {"max_rounds": 0},
                        "runner_defaults": {
                            "repo": str(repo),
                            "task_kind": "release-harness",
                            "mode": "routed-on",
                            "mcp_url": "http://127.0.0.1:7837/mcp",
                        },
                    },
                    ensure_ascii=False,
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            stage_payloads = {
                "worker_scan": {
                    "actual": {"tokens": 5000, "elapsed_ms": 1200, "tool_calls": 1},
                    "message": "worker summary\n",
                },
                "orchestrator": {
                    "actual": {"tokens": 33000, "elapsed_ms": 76000, "tool_calls": 1},
                    "message": "orchestrator summary\n",
                },
                "evaluator": {
                    "actual": {"tokens": 32000, "elapsed_ms": 32000, "tool_calls": 0},
                    "message": json.dumps(
                        {
                            "verdict": "PASS",
                            "aggregate_score": 1.0,
                            "summary": "criteria satisfied",
                            "repair_hints": [],
                            "issues": [],
                        }
                    )
                    + "\n",
                },
                "independent_signoff": {
                    "actual": {"tokens": 31000, "elapsed_ms": 26000, "tool_calls": 1},
                    "message": json.dumps(
                        {
                            "verdict": "FAIL",
                            "aggregate_score": 0.5,
                            "summary": "independent disagreement",
                            "repair_hints": ["fix the release note"],
                            "issues": ["missing independent pass"],
                        }
                    )
                    + "\n",
                },
            }

            def fake_run(command, capture_output=True, text=True):
                stage_root = Path(option_value(command, "--run-dir"))
                stage_name = stage_root.name
                payload = stage_payloads[stage_name]
                write_stage_files(stage_root, payload)
                Path(option_value(command, "--output-last-message")).write_text(
                    payload["message"] + render_usage_block("actual", payload["actual"]),
                    encoding="utf-8",
                )
                return subprocess.CompletedProcess(command, 0, stdout=f"{stage_name}\n", stderr="")

            output = io.StringIO()
            with mock.patch.object(RELEASE.subprocess, "run", side_effect=fake_run):
                with mock.patch.object(
                    sys,
                    "argv",
                    [
                        "release-harness-runner.py",
                        "--manifest",
                        str(manifest_path),
                        "--run-dir",
                        str(run_dir),
                        "--exec",
                    ],
                ):
                    with contextlib.redirect_stdout(output):
                        RELEASE.main()

            result = json.loads(output.getvalue())
            signoff = json.loads((run_dir / "independent-signoff.json").read_text(encoding="utf-8"))

        self.assertEqual(result["status"], "fail")
        self.assertTrue(signoff["disagreement"])
        self.assertEqual(signoff["independent_verdict"], "FAIL")


if __name__ == "__main__":
    unittest.main()
