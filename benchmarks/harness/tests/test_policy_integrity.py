import importlib.util
import json
import sys
import tempfile
import unittest
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

    def test_recommendation_outcome_detects_followup_only_match(self):
        outcome = runner_common.build_codex_recommendation_outcome(
            {
                "available": True,
                "recommended_entrypoint": "impact_report",
                "recommended_followup_tools": ["get_analysis_section", "verify_change_readiness"],
                "recommended_contract_action": "use_prefetched_workflow_contract",
                "richer_contract_prefetched": True,
            },
            {
                "tools": [
                    {
                        "tool": "get_analysis_section",
                        "calls": 2,
                        "total_ms": 25,
                        "total_tokens": 40,
                    }
                ],
                "session": {
                    "deferred_namespace_expansion_count": 1,
                    "deferred_hidden_tool_call_denied_count": 0,
                },
            },
        )

        self.assertIsNotNone(outcome)
        self.assertEqual(outcome["alignment"], "matched-followup")
        self.assertEqual(outcome["recommended_followup_tools_called"], ["get_analysis_section"])
        self.assertEqual(outcome["recommended_followup_tools_missed"], ["verify_change_readiness"])
        self.assertTrue(outcome["contract_action_aligned"])
        self.assertEqual(
            runner_common.summarize_codex_recommendation_outcome(outcome),
            "recommended follow-up tools exercised: get_analysis_section",
        )


if __name__ == "__main__":
    unittest.main()
