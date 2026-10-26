"""Missing provider observations must never become cheap or successful runs."""

import copy
import importlib.util
import sys
import unittest
from pathlib import Path

HARNESS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HARNESS))
spec = importlib.util.spec_from_file_location("analysis_lite_harness_eval", HARNESS / "harness-eval.py")
EVAL = importlib.util.module_from_spec(spec)
spec.loader.exec_module(EVAL)


def real_entry(**changes):
    entry = {
        "repo": "/tmp/project", "repo_id": "project", "repo_label": "project",
        "source_kind": "real-session", "task_kind": "impact/reviewer", "mode": "baseline",
        "agent": "codex", "success": True, "acceptance_passed": True, "verify_passed": True,
        "quality_score": 1.0, "accounting_version": "provider-usage-v1",
        "token_in": 100, "token_out": 20,
        "provider_usage": {"source": "codex.turn.completed", "status": "ok",
            "raw": {"input_tokens": 100, "output_tokens": 20},
            "input_includes_cache": True, "input_tokens": 100, "output_tokens": 20},
    }
    entry.update(changes)
    return entry


class AnalysisLiteAggregationTests(unittest.TestCase):
    def test_missing_or_partial_provider_is_excluded_from_measured_denominator(self):
        rows = [real_entry(), real_entry(token_in=None, token_out=None, provider_usage={"status": "unknown"}),
                real_entry(provider_usage={"status": "partial"}), real_entry(provider_usage={"status": "error"})]
        stats = EVAL.mode_stats(rows)
        self.assertEqual(stats["avg_total_tokens"], 120)
        self.assertEqual(stats["token_measured_count"], 1)
        self.assertEqual(stats["token_unknown_count"], 3)

    def test_empty_and_all_unknown_are_null_instead_of_zero(self):
        for rows in [[], [real_entry(token_in=None, provider_usage=None)]]:
            with self.subTest(rows=rows):
                self.assertIsNone(EVAL.mode_stats(rows)["avg_total_tokens"])

    def test_historical_server_estimates_are_retained_but_not_actual(self):
        old = real_entry()
        del old["accounting_version"]
        del old["provider_usage"]
        old.update(metrics_snapshot={"total_tokens": 120}, bootstrap_tokens=100)
        original = copy.deepcopy(old)
        stats = EVAL.mode_stats([old])
        self.assertIsNone(EVAL.total_tokens(old))
        self.assertEqual(stats["historical_estimates"]["count"], 1)
        self.assertEqual(stats["historical_estimates"]["avg_total_tokens"], 120)
        self.assertIsNone(stats["avg_quality_score"])
        self.assertEqual(old, original)

    def test_legacy_provider_measurement_requires_explicit_provenance(self):
        old = real_entry(token_source="provider", provider_usage_source="host-result.json")
        del old["accounting_version"]
        del old["provider_usage"]
        self.assertEqual(EVAL.total_tokens(old), 120)
        self.assertEqual(EVAL.mode_stats([old])["legacy_provider_measured_count"], 1)

    def test_malformed_or_inconsistent_counts_do_not_enter_average(self):
        for changes in [{"token_in": True}, {"token_in": -1}, {"token_in": 2},
                        {"provider_usage": {"status": "ok"}}]:
            with self.subTest(changes=changes):
                self.assertIsNone(EVAL.total_tokens(real_entry(**changes)))

    def test_real_zero_is_a_measurement(self):
        row = real_entry(token_in=0, token_out=0, provider_usage={"source": "codex.turn.completed",
            "status": "ok", "raw": {"input_tokens": 0, "output_tokens": 0},
            "input_includes_cache": True, "input_tokens": 0, "output_tokens": 0})
        stats = EVAL.mode_stats([row])
        self.assertEqual(stats["avg_total_tokens"], 0)
        self.assertEqual(stats["token_measured_count"], 1)

    def test_unknown_success_and_quality_are_not_replaced_by_process_kpi(self):
        stats = EVAL.mode_stats([real_entry(success=None, quality_score=None, process_kpi_score=1.0)])
        self.assertIsNone(stats["success_rate"])
        self.assertIsNone(stats["avg_quality_score"])
        self.assertEqual(stats["success_unknown_count"], 1)
        self.assertEqual(stats["quality_unknown_count"], 1)

    def test_unknown_or_partial_evidence_never_promotes_a_policy(self):
        stats = EVAL.mode_stats([real_entry(provider_usage={"status": "partial"})])
        for kind in ["impact/reviewer", "simple local lookup/edit"]:
            with self.subTest(kind=kind):
                self.assertEqual(EVAL.choose_policy(kind, {"baseline": stats, "routed-on": stats}),
                                 "prefer_native_baseline")

    def test_failures_and_unknown_runs_remain_visible_in_summaries(self):
        rows = [real_entry(), real_entry(success=False), real_entry(success=None, quality_score=None)]
        summary = EVAL.build_task_summaries(rows)[0]
        self.assertEqual(summary["mode_stats"]["baseline"]["count"], 3)
        self.assertEqual(summary["mode_stats"]["baseline"]["success_rate"], 0.5)
        self.assertEqual(summary["decision_status"], "inconclusive")

    def test_candidate_only_group_cannot_establish_a_comparison(self):
        summary = EVAL.build_task_summaries([real_entry(mode="routed-on")])[0]
        self.assertEqual(summary["mode_stats"]["routed-on"]["token_measured_count"], 1)
        self.assertEqual(summary["decision_status"], "inconclusive")
        self.assertEqual(summary["recommended_policy"], "prefer_native_baseline")

    def test_accounting_sessions_need_measured_acceptance_to_qualify(self):
        self.assertFalse(EVAL.common.qualifying_real_entry(real_entry(acceptance_passed=None)))
        self.assertFalse(EVAL.common.qualifying_real_entry(real_entry(verify_passed=None)))

    def test_distinct_pilot_run_ids_are_not_deduplicated_as_reruns(self):
        rows = [real_entry(run_id="task1-codex-A-1"),
                real_entry(run_id="task1-codex-A-2", success=False),
                real_entry(run_id="task1-claude-A-1", success=None)]
        retained, duplicates = EVAL.common.dedupe_real_session_entries(rows)
        self.assertEqual(len(retained), 3)
        self.assertEqual(duplicates, [])

    def test_markdown_accepts_nulls_and_shows_unknown_measurements(self):
        rows = [real_entry(token_in=None, token_out=None, provider_usage=None, quality_score=None)]
        summaries = EVAL.build_task_summaries(rows)
        report = {"entries": rows, "task_summaries": summaries, "baseline_reference": {},
                  "representative_repos": []}
        text = EVAL.render_report(report)
        self.assertIn("unknown", text)
        self.assertIn("0/1", text)


if __name__ == "__main__":
    unittest.main()
