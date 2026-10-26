"""Regression contracts for provider accounting and normal host inheritance."""

import argparse
import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import tomllib

HARNESS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HARNESS))


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), HARNESS / name)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_script("codex-task-runner.py")
SESSION = load_script("session-eval.py")


def entry_args(**overrides):
    values = {"repo": "/tmp/project", "task_kind": "lookup", "mode": "baseline", "agent": "codex",
              "success": None, "acceptance_passed": None, "verify_passed": None,
              "quality_score": None, "recommended_policy": "", "notes": ""}
    values.update(overrides)
    return argparse.Namespace(**values)


class AnalysisLiteMetricsTests(unittest.TestCase):
    def test_absent_model_and_effort_are_not_replaced(self):
        config = tomllib.loads(RUNNER.build_minimal_codex_home_config(
            repo_paths=[], mcp_url="http://localhost/mcp"))
        self.assertNotIn("model", config)
        self.assertNotIn("model_reasoning_effort", config)

    def test_explicit_model_and_effort_are_preserved(self):
        config = tomllib.loads(RUNNER.build_minimal_codex_home_config(
            repo_paths=[], mcp_url="http://localhost/mcp", model="gpt-6.1-sol",
            reasoning_effort="high"))
        self.assertEqual(config["model"], "gpt-6.1-sol")
        self.assertEqual(config["model_reasoning_effort"], "high")

    def test_legacy_home_copy_is_denied_before_filesystem_access(self):
        with mock.patch.object(Path, "exists", side_effect=AssertionError("credential probe")):
            result = RUNNER.prepare_isolated_codex_home(
                source_home=Path("/never-read"), repo_paths=[], mcp_url="http://localhost/mcp")
        self.assertEqual(result, (None, None, "legacy auth copying disabled"))

    def test_missing_provider_usage_keeps_actual_tokens_unknown(self):
        entry = SESSION.build_entry(entry_args(), {"session": {"tools_list_tokens": 25,
                                                             "total_tokens": 90}})
        self.assertIsNone(entry["token_in"])
        self.assertIsNone(entry["token_out"])
        self.assertEqual(entry["provider_usage"]["status"], "unknown")
        self.assertEqual(entry["server_estimated_tools_list_tokens"], 25)
        self.assertEqual(entry["server_estimated_total_tokens"], 90)
        self.assertEqual(entry["bootstrap_tokens"], 25)  # legacy server estimate alias

    def test_no_server_metrics_does_not_invent_counts_or_success(self):
        entry = SESSION.build_entry(entry_args(), {})
        self.assertIsNone(entry["server_estimated_total_tokens"])
        self.assertIsNone(entry["bootstrap_tokens"])
        self.assertIsNone(entry["success"])

    def test_process_metrics_do_not_grade_task_quality(self):
        entry = SESSION.build_entry(entry_args(), {
            "session": {"total_calls": 2, "error_count": 0},
            "derived_kpis": {"verifier_contract_present_rate": 1, "composite_ratio": 1}})
        self.assertIsNone(entry["quality_score"])
        self.assertIsNone(entry["success"])
        self.assertGreater(entry["process_kpi_score"], 0)

    def test_explicit_quality_and_success_survive(self):
        entry = SESSION.build_entry(entry_args(quality_score=0.75, success=False), {})
        self.assertEqual(entry["quality_score"], 0.75)
        self.assertIs(entry["success"], False)

    def test_inclusive_cache_is_not_added_twice_and_raw_is_preserved(self):
        raw = {"input_tokens": 100, "cached_input_tokens": 70, "output_tokens": 20}
        entry = SESSION.build_entry(entry_args(provider_usage=raw,
            provider_usage_source="codex.turn.completed", provider_input_includes_cache=True),
            {"session": {"tools_list_tokens": 9000, "total_tokens": 20000}})
        self.assertEqual((entry["token_in"], entry["token_out"]), (100, 20))
        self.assertEqual(entry["provider_usage"]["uncached_input_tokens"], 30)
        self.assertEqual(entry["provider_usage"]["total_tokens"], 120)
        self.assertEqual(entry["server_estimated_total_tokens"], 20000)
        self.assertEqual(entry["provider_usage"]["source"], "codex.turn.completed")
        self.assertEqual(entry["provider_usage"]["raw"], raw)
        self.assertEqual(raw, {"input_tokens": 100, "cached_input_tokens": 70, "output_tokens": 20})

    def test_exclusive_cache_components_are_added_once(self):
        raw = {"input_tokens": 10, "cache_read_input_tokens": 70,
               "cache_creation_input_tokens": 20, "output_tokens": 5}
        entry = SESSION.build_entry(entry_args(provider_usage=raw,
            provider_usage_source="claude.result", provider_input_includes_cache=False), {})
        self.assertEqual(entry["token_in"], 100)
        self.assertEqual(entry["provider_usage"]["total_tokens"], 105)

    def test_undeclared_cache_semantics_stays_unknown(self):
        entry = SESSION.build_entry(entry_args(provider_usage={"input_tokens": 100,
            "output_tokens": 20}, provider_usage_source="unverified"), {})
        self.assertIsNone(entry["token_in"])
        self.assertEqual(entry["token_out"], 20)

    def test_zero_usage_is_measured_and_missing_cache_is_unknown(self):
        for raw, inclusive, expected_input in [
            ({"input_tokens": 0, "output_tokens": 0}, True, 0),
            ({"input_tokens": 10, "output_tokens": 2}, False, None),
        ]:
            with self.subTest(raw=raw):
                entry = SESSION.build_entry(entry_args(provider_usage=raw,
                    provider_usage_source="host.raw", provider_input_includes_cache=inclusive), {})
                self.assertEqual(entry["token_in"], expected_input)
                self.assertEqual(entry["provider_usage"]["raw"], raw)

    def test_invalid_counts_and_cache_overlap_are_reported(self):
        for raw in [{"input_tokens": -1}, {"input_tokens": True},
                    {"input_tokens": 10, "cached_input_tokens": 11}]:
            with self.subTest(raw=raw):
                entry = SESSION.build_entry(entry_args(provider_usage=raw,
                    provider_usage_source="codex", provider_input_includes_cache=True), {})
                self.assertEqual(entry["provider_usage"]["status"], "error")
                self.assertIsNone(entry["token_in"])

    def test_default_runner_uses_host_auth_and_no_model_override(self):
        # Only the external model process is replaced; policy, prompt, manifest and runner are real.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "policy.json").write_text("{}")
            (root / "repos.json").write_text("{}")
            argv = ["runner", "--repo", tmp, "--task-kind", "lookup", "--task", "read",
                    "--policy", str(root / "policy.json"), "--repo-config", str(root / "repos.json"),
                    "--run-dir", str(root / "run"), "--bootstrap-json", str(root / "brief.json"),
                    "--bootstrap-md", str(root / "brief.md"), "--prompt-file", str(root / "prompt.md"),
                    "--skip-mcp-preflight", "--exec"]
            output = io.StringIO()
            process = mock.Mock(returncode=0)
            with mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(output), \
                 mock.patch.object(RUNNER.subprocess, "run", return_value=process) as launch, \
                 mock.patch.object(RUNNER, "prepare_isolated_codex_home",
                                   side_effect=AssertionError("auth copy forbidden")):
                RUNNER.main()
            result = json.loads(output.getvalue())
            self.assertEqual(result["codex_home_mode"], "current-host")
            self.assertIsNone(launch.call_args.kwargs["env"])
            self.assertNotIn("--model", result["codex_command"])
            self.assertEqual(result["host_settings"]["model"]["source"], "host-inherited")


if __name__ == "__main__":
    unittest.main()
