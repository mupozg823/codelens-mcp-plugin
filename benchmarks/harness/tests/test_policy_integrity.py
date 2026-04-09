import importlib.util
import json
import sys
import tempfile
import unittest
from types import SimpleNamespace
from pathlib import Path


HARNESS_DIR = Path(__file__).resolve().parents[1]
if str(HARNESS_DIR) not in sys.path:
    sys.path.insert(0, str(HARNESS_DIR))

import harness_eval_common as common  # noqa: E402
import harness_runner_common as runner_common  # noqa: E402


def load_script_module(module_name: str, filename: str):
    path = HARNESS_DIR / filename
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


APPLY = load_script_module("apply_routing_policy_test", "apply-routing-policy.py")
EXPORT = load_script_module("export_routing_policy_test", "export-routing-policy.py")
HARNESS_EVAL = load_script_module("harness_eval_test", "harness-eval.py")
REFRESH = load_script_module("refresh_routing_policy_test", "refresh-routing-policy.py")
TASK_BOOTSTRAP = load_script_module("task_bootstrap_test", "task-bootstrap.py")
SESSION_EVAL = load_script_module("session_eval_test", "session-eval.py")


def make_repo():
    return {
        "id": "signature-studio",
        "label": "Next.js app with AGENTS",
        "path": "/tmp/SignatureStudio",
    }


def make_real_entry(**overrides):
    entry = {
        "source_kind": "real-session",
        "repo": "/tmp/SignatureStudio",
        "repo_id": "signature-studio",
        "repo_label": "Next.js app with AGENTS",
        "task_kind": "onboarding/planning",
        "mode": "baseline",
        "agent": "codex",
        "success": True,
        "acceptance_passed": True,
        "verify_passed": True,
        "quality_score": None,
        "notes": "captured from scenario signature-studio::onboarding/planning::baseline",
        "_source_path": "/tmp/session-a.json",
    }
    entry.update(overrides)
    return entry


def make_policy(*, policy_scope="agent", agent="codex", overrides=None, global_policy="prefer_codelens_after_bootstrap"):
    return {
        "schema_version": "codelens-routing-policy-v2",
        "policy_scope": policy_scope,
        "agent": agent if policy_scope == "agent" else None,
        "generated_at": "2026-04-04T17:45:36",
        "source_report": "2026-04-04T17:45:36",
        "source_report_path": "/tmp/refresh.json",
        "binary": "/tmp/codelens-mcp",
        "source_of_truth": "policy_json",
        "runtime_authority": "agent_canonical_json" if policy_scope == "agent" else "shared_summary_json",
        "global_rules": [
            {
                "task_kind": "onboarding/planning",
                "recommended_policy": global_policy,
                "consensus": "majority",
                "repo_count": 3,
                "vote_count": 2,
                "explanation": EXPORT.POLICY_EXPLANATIONS[global_policy],
            }
        ],
        "repo_overrides": overrides or [],
    }


class PolicyIntegrityTests(unittest.TestCase):
    def test_choose_rule_prefers_repo_override(self):
        policy = {
            "global_rules": [
                {
                    "task_kind": "onboarding/planning",
                    "recommended_policy": "prefer_codelens_after_bootstrap",
                    "consensus": "unanimous",
                    "explanation": "",
                }
            ],
            "repo_overrides": [
                {
                    "repo_id": "signature-studio",
                    "task_kind": "onboarding/planning",
                    "recommended_policy": "prefer_native_baseline",
                    "confidence": "medium",
                    "explanation": "",
                }
            ],
        }

        result = TASK_BOOTSTRAP.choose_rule(policy, "signature-studio", "onboarding/planning")

        self.assertEqual(result["source"], "repo_override")
        self.assertEqual(result["recommended_policy"], "prefer_native_baseline")

    def test_repo_normalization_and_qualifying_dedupe_share_one_path(self):
        entries = [
            make_real_entry(repo_id="SignatureStudio", agent="Codex", _source_path="/tmp/session-a.json"),
            make_real_entry(repo_id="signature-studio", agent="codex", _source_path="/tmp/session-b.json"),
            make_real_entry(
                repo_id="SignatureStudio",
                success=False,
                _source_path="/tmp/session-c.json",
            ),
        ]

        common.canonicalize_entry_repo_ids(entries, [make_repo()])
        deduped, duplicates = common.dedupe_real_session_entries(
            entries,
            include_entry=common.qualifying_real_entry,
        )

        self.assertEqual([entry["repo_id"] for entry in entries], ["signature-studio"] * 3)
        self.assertEqual([entry["repo_label"] for entry in entries], ["Next.js app with AGENTS"] * 3)
        self.assertEqual(len(deduped), 1)
        self.assertEqual(deduped[0]["_source_path"], "/tmp/session-b.json")
        self.assertEqual(len(duplicates), 1)

    def test_qualifying_real_entry_rejects_completion_contract_failures(self):
        self.assertFalse(
            common.qualifying_real_entry(
                make_real_entry(completion_contract_passed=False)
            )
        )
        self.assertFalse(
            common.qualifying_real_entry(
                make_real_entry(asked_for_user_input=True)
            )
        )

    def test_codex_preflight_policy_skips_optional_and_avoid_routes(self):
        self.assertFalse(runner_common.should_run_codex_mcp_preflight({"use_codelens": "avoid"}))
        self.assertFalse(runner_common.should_run_codex_mcp_preflight({"use_codelens": "optional"}))
        self.assertTrue(runner_common.should_run_codex_mcp_preflight({"use_codelens": "recommended"}))
        self.assertTrue(runner_common.should_run_codex_mcp_preflight({"use_codelens": "required"}))

    def test_codex_preflight_cache_round_trip_and_expiry(self):
        brief = {
            "task_kind": "impact/reviewer",
            "recommended_policy": "prefer_codelens_after_bootstrap",
            "route_mode": "native_then_deferred_workflow",
            "use_codelens": "recommended",
            "native_first": True,
            "deferred_loading": True,
            "preferred_entrypoints": ["impact_report"],
            "evaluation_mode": "read-only-eval",
        }
        payload = {
            "available": True,
            "preferred_entrypoints": ["impact_report"],
            "fallback_to_native": False,
            "session_id": "ephemeral",
            "init_response": {"jsonrpc": "2.0"},
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            cache_dir = Path(tmpdir)
            cache_key = runner_common.build_codex_preflight_cache_key(
                "http://127.0.0.1:7837/mcp",
                Path("/tmp/repo"),
                brief,
            )
            runner_common.write_cached_codex_preflight(cache_dir, cache_key, payload)
            cached = runner_common.read_cached_codex_preflight(cache_dir, cache_key, 300)
            self.assertIsNotNone(cached)
            self.assertTrue(cached["cache_hit"])
            self.assertEqual(cached["probe_strategy"], "cache")
            self.assertNotIn("session_id", cached)
            self.assertNotIn("init_response", cached)
            self.assertIsNone(runner_common.read_cached_codex_preflight(cache_dir, cache_key, 0))

    def test_codex_metrics_capture_policy_respects_mode_and_route(self):
        self.assertFalse(runner_common.should_capture_codex_metrics({"use_codelens": "optional"}, "baseline"))
        self.assertTrue(runner_common.should_capture_codex_metrics({"use_codelens": "optional"}, "naive-on"))
        self.assertTrue(runner_common.should_capture_codex_metrics({"use_codelens": "recommended"}, "baseline"))

    def test_session_entry_without_metrics_keeps_null_metric_fields(self):
        args = SimpleNamespace(
            repo="/tmp/repo",
            repo_id="repo",
            repo_label="Repo",
            task_kind="simple local lookup/edit",
            mode="baseline",
            agent="codex",
            success=True,
            acceptance_passed=None,
            verify_passed=None,
            quality_score=None,
            recommended_policy="native_or_naive_both_ok_but_default_native",
            notes="",
            last_message_file="",
        )
        entry = SESSION_EVAL.build_entry(
            args,
            runner_common.empty_metrics_delta_payload("native policy / baseline"),
        )
        self.assertIsNone(entry["token_in"])
        self.assertIsNone(entry["token_out"])
        self.assertIsNone(entry["bootstrap_tokens"])
        self.assertIsNone(entry["tool_calls"])
        self.assertTrue(entry["metrics_capture_skipped"])
        self.assertIsNone(entry["quality_score"])

    def test_mode_stats_ignores_entries_without_token_metrics(self):
        stats = HARNESS_EVAL.mode_stats(
            [
                {
                    "success": True,
                    "bootstrap_tokens": None,
                    "token_in": None,
                    "token_out": None,
                    "quality_score": None,
                    "tool_calls": None,
                },
                {
                    "success": True,
                    "bootstrap_tokens": 10,
                    "token_in": 10,
                    "token_out": 20,
                    "quality_score": None,
                    "tool_calls": 1,
                },
            ]
        )
        self.assertEqual(stats["avg_total_tokens"], 30)
        self.assertEqual(stats["avg_bootstrap_tokens"], 10)

    def test_codex_prompt_compacts_duplicate_guidance(self):
        brief = {
            "platform": "codex",
            "task_kind": "impact/reviewer",
            "recommended_policy": "prefer_codelens_after_bootstrap",
            "policy_source": "global_rule",
            "confidence": "high",
            "route_mode": "native_then_deferred_workflow",
            "explanation": "native then workflow",
            "first_actions": [
                "Start with native rg/read/test.",
                "CodeLens entrypoints: `impact_report`, `analyze_change_request`.",
            ],
            "workflow_budget": {},
            "result_budget": {},
            "stop_rule": "",
            "preferred_entrypoints": ["impact_report", "analyze_change_request"],
            "use_codelens": "recommended",
            "task": "review dispatch changes",
            "verify_commands": ["cargo check"],
        }
        mcp_preflight = {
            "available": True,
            "auto_surface": "planner-readonly",
            "auto_budget": 6000,
            "tools_list_contract_mode": "lean",
            "recommended_entrypoint": "impact_report",
            "recommended_contract_action": "stay_lean_until_shape_needed",
            "recommended_followup_tools": ["analyze_change_request"],
            "fallback_to_native": True,
        }
        prompt = runner_common.render_prompt(
            brief,
            "~/.codex/AGENTS.md",
            mcp_preflight=mcp_preflight,
        )
        self.assertEqual(prompt.count("Non-interactive by default"), 1)
        self.assertNotIn("Preferred CodeLens entrypoints for this task kind:", prompt)
        self.assertIn("Finish with these exact headers:", prompt)

    def test_policy_entries_exclude_non_qualifying_real_sessions(self):
        entries = [
            {
                "source_kind": "synthetic",
                "repo": "/tmp/SignatureStudio",
                "repo_id": "signature-studio",
                "repo_label": "Next.js app with AGENTS",
                "task_kind": "onboarding/planning",
                "mode": "naive-on",
            },
            make_real_entry(_source_path="/tmp/good.json"),
            make_real_entry(success=False, _source_path="/tmp/bad.json"),
        ]

        policy_entries = HARNESS_EVAL.build_policy_entries(entries)

        self.assertEqual(len(policy_entries), 2)
        self.assertEqual(
            sum(1 for entry in policy_entries if entry.get("source_kind") == "real-session"),
            1,
        )

    def test_coverage_summary_counts_only_qualifying_sessions_but_keeps_diagnostics(self):
        config = {"representative_repos": [make_repo()]}
        entries = [
            make_real_entry(_source_path="/tmp/good.json"),
            make_real_entry(success=False, _source_path="/tmp/bad.json"),
        ]
        common.canonicalize_entry_repo_ids(entries, config["representative_repos"])

        coverage = REFRESH.coverage_summary(
            config,
            entries,
            ["onboarding/planning"],
            1,
            ["codex"],
        )

        self.assertEqual(coverage["total_real_entries"], 2)
        self.assertEqual(coverage["qualifying_real_entries"], 1)
        self.assertEqual(coverage["unique_qualifying_real_entries"], 1)
        self.assertEqual(coverage["coverage"][0]["count"], 1)

    def test_agent_divergence_is_reported_separately_from_shared_policy(self):
        shared_policy = make_policy(policy_scope="shared", agent=None)
        codex_policy = make_policy(agent="codex")
        claude_policy = make_policy(
            agent="claude",
            overrides=[
                {
                    "repo_id": "signature-studio",
                    "repo_label": "Next.js app with AGENTS",
                    "task_kind": "onboarding/planning",
                    "recommended_policy": "prefer_native_baseline",
                    "confidence": "medium",
                    "explanation": EXPORT.POLICY_EXPLANATIONS["prefer_native_baseline"],
                }
            ],
        )

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            shared_path = tmp / "shared.json"
            codex_path = tmp / "codex.json"
            claude_path = tmp / "claude.json"
            shared_path.write_text(json.dumps(shared_policy))
            codex_path.write_text(json.dumps(codex_policy))
            claude_path.write_text(json.dumps(claude_policy))

            divergence = REFRESH.agent_policy_divergence(
                shared_path,
                {"codex": codex_path, "claude": claude_path},
            )

        self.assertTrue(divergence["changed"])
        self.assertEqual(len(divergence["global_rule_changes"]), 0)
        self.assertEqual(len(divergence["repo_override_changes"]), 1)
        self.assertEqual(divergence["repo_override_changes"][0]["agent"], "claude")

    def test_agent_divergence_ignores_consensus_only_differences(self):
        shared_policy = make_policy(policy_scope="shared", agent=None)
        claude_policy = make_policy(agent="claude")
        claude_policy["global_rules"][0]["consensus"] = "majority"

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            shared_path = tmp / "shared.json"
            claude_path = tmp / "claude.json"
            shared_path.write_text(json.dumps(shared_policy))
            claude_path.write_text(json.dumps(claude_policy))

            divergence = REFRESH.agent_policy_divergence(
                shared_path,
                {"claude": claude_path},
            )

        self.assertFalse(divergence["changed"])
        self.assertEqual(divergence["global_rule_changes"], [])
        self.assertEqual(divergence["repo_override_changes"], [])

    def test_promotion_integrity_ignores_generated_at_but_catches_policy_changes(self):
        preview_policy = make_policy(agent="codex")
        canonical_policy = make_policy(agent="codex")
        canonical_policy["generated_at"] = "2026-04-04T17:46:00"

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            preview_path = tmp / "preview.json"
            canonical_path = tmp / "canonical.json"
            preview_path.write_text(json.dumps(preview_policy))
            canonical_path.write_text(json.dumps(canonical_policy))

            integrity = REFRESH.promotion_integrity_summary(
                {"codex": preview_path},
                {"codex": canonical_path},
            )

            self.assertTrue(integrity["ok"])

            canonical_policy["global_rules"][0]["recommended_policy"] = "prefer_native_baseline"
            canonical_path.write_text(json.dumps(canonical_policy))
            broken_integrity = REFRESH.promotion_integrity_summary(
                {"codex": preview_path},
                {"codex": canonical_path},
            )

        self.assertFalse(broken_integrity["ok"])
        self.assertEqual(broken_integrity["mismatched_targets"], ["codex"])

    def test_generated_sections_are_marked_non_authoritative_and_agent_scoped(self):
        policy = make_policy(agent="codex")

        policy_section = APPLY.render_policy_section(policy)
        claude_override = APPLY.render_override_snippet(
            "Next.js app with AGENTS",
            "signature-studio",
            [],
            "claude",
        )

        self.assertIn("non-authoritative", policy_section)
        self.assertIn("authoritative policy JSON", policy_section)
        self.assertIn("Policy target: `claude`", claude_override)
        self.assertIn("reference only", claude_override)

    def test_completion_contract_parser_detects_expected_sections(self):
        analysis = common.analyze_completion_contract(
            """
- Requested work completed: updated routing bootstrap
- Evidence used: policy refresh and session metrics
- Verification run: cargo check
- Remaining risks: no fresh replay yet
"""
        )
        self.assertEqual(analysis["score"], 1.0)
        self.assertTrue(analysis["passed"])
        self.assertFalse(analysis["asked_for_user_input"])

    def test_completion_contract_parser_accepts_bold_markdown_headers(self):
        analysis = common.analyze_completion_contract(
            """
**Requested work completed:** updated routing bootstrap
**Evidence used:** policy refresh and session metrics
**Verification run:** cargo check
**Remaining risks:** no fresh replay yet
"""
        )
        self.assertEqual(analysis["score"], 1.0)
        self.assertTrue(analysis["passed"])

    def test_completion_contract_parser_ignores_non_blocking_korean_offer(self):
        analysis = common.analyze_completion_contract(
            """
- Remaining risks: none
원하시면 추가로 cargo check까지 돌릴 수 있습니다.
"""
        )
        self.assertFalse(analysis["asked_for_user_input"])


if __name__ == "__main__":
    unittest.main()
