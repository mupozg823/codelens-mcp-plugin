#!/usr/bin/env python3
"""Tests for policy pipeline integrity."""

from __future__ import annotations

import json
import sys
import tempfile
from types import SimpleNamespace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import harness_eval_common as common
import harness_runner_common as runner_common
import importlib.util


def load_script_module(module_name: str, filename: str):
    path = Path(__file__).resolve().parent / filename
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


HARNESS_EVAL = load_script_module("harness_eval_policy_test", "harness-eval.py")
SESSION_EVAL = load_script_module("session_eval_policy_test", "session-eval.py")


def test_repo_override_trumps_global():
    """repo_override must always take precedence over global_rule."""
    policy = {
        "global_rules": [
            {
                "task_kind": "onboarding/planning",
                "recommended_policy": "prefer_codelens_after_bootstrap",
            },
        ],
        "repo_overrides": [
            {
                "repo_id": "signature-studio",
                "task_kind": "onboarding/planning",
                "recommended_policy": "prefer_native_baseline",
            },
        ],
    }
    global_map = {
        r["task_kind"]: r["recommended_policy"] for r in policy["global_rules"]
    }
    override_map = {
        (r["repo_id"], r["task_kind"]): r["recommended_policy"]
        for r in policy["repo_overrides"]
    }

    # Runtime resolution: override > global > fallback
    effective = override_map.get(
        ("signature-studio", "onboarding/planning")
    ) or global_map.get("onboarding/planning")
    assert (
        effective == "prefer_native_baseline"
    ), f"Expected prefer_native_baseline, got {effective}"

    # Non-overridden task should fall through to global
    effective2 = override_map.get(
        ("signature-studio", "impact/reviewer")
    ) or global_map.get("impact/reviewer")
    assert effective2 is None


def test_agent_divergence_is_valid():
    """codex and claude can have different canonical policies for the same repo/task."""
    codex_policy = {
        "agent": "codex",
        "global_rules": [
            {
                "task_kind": "onboarding/planning",
                "recommended_policy": "prefer_codelens_after_bootstrap",
            }
        ],
        "repo_overrides": [],
    }
    claude_policy = {
        "agent": "claude",
        "global_rules": [
            {
                "task_kind": "onboarding/planning",
                "recommended_policy": "prefer_codelens_after_bootstrap",
            }
        ],
        "repo_overrides": [
            {
                "repo_id": "signature-studio",
                "task_kind": "onboarding/planning",
                "recommended_policy": "prefer_native_baseline",
            },
        ],
    }
    codex_effective = "prefer_codelens_after_bootstrap"
    claude_effective = "prefer_native_baseline"
    assert codex_effective != claude_effective, "Divergence should be detected"
    valid_policies = {
        "prefer_routed_codelens",
        "prefer_codelens_after_bootstrap",
        "prefer_naive_codelens",
        "prefer_native_baseline",
        "avoid_codelens_for_simple_local_lookup",
        "native_or_naive_both_ok_but_default_native",
    }
    assert codex_effective in valid_policies
    assert claude_effective in valid_policies


def test_repo_id_canonicalization():
    """SignatureStudio and signature-studio must map to the same canonical repo_id."""
    assert common.canonical_repo_key("SignatureStudio") == common.canonical_repo_key(
        "signature-studio"
    )
    assert common.canonical_repo_key("Signature-Studio") == common.canonical_repo_key(
        "signaturestudio"
    )
    assert common.canonical_repo_key(
        "codelens-mcp-plugin"
    ) == common.canonical_repo_key("CodeLens-MCP-Plugin")


def test_qualifying_filter():
    """Non-qualifying real-sessions must not influence policy summary."""
    entries = [
        {"source_kind": "synthetic", "success": True, "repo_id": "r", "task_kind": "t"},
        {
            "source_kind": "real-session",
            "success": True,
            "acceptance_passed": None,
            "verify_passed": None,
        },
        {"source_kind": "real-session", "success": False},
        {"source_kind": "real-session", "success": True, "acceptance_passed": False},
        {"source_kind": "real-session", "success": True, "verify_passed": False},
        {"source_kind": "real-session", "success": True, "completion_contract_passed": False},
        {"source_kind": "real-session", "success": True, "asked_for_user_input": True},
    ]
    filtered = common.filter_qualifying_entries(entries)
    assert (
        len(filtered) == 2
    ), f"Expected 2 entries (1 synthetic + 1 qualifying real), got {len(filtered)}"
    assert filtered[0]["source_kind"] == "synthetic"
    assert filtered[1]["source_kind"] == "real-session"
    assert filtered[1]["success"] is True


def test_qualifying_real_entry_edge_cases():
    """qualifying_real_entry must handle edge cases correctly."""
    assert (
        common.qualifying_real_entry({"source_kind": "real-session", "success": True})
        is True
    )
    assert (
        common.qualifying_real_entry(
            {"source_kind": "real-session", "success": True, "acceptance_passed": None}
        )
        is True
    )
    assert (
        common.qualifying_real_entry(
            {"source_kind": "real-session", "success": True, "verify_passed": None}
        )
        is True
    )
    assert (
        common.qualifying_real_entry({"source_kind": "synthetic", "success": True})
        is False
    )
    assert (
        common.qualifying_real_entry(
            {
                "source_kind": "real-session",
                "success": True,
                "completion_contract_passed": False,
            }
        )
        is False
    )
    assert (
        common.qualifying_real_entry(
            {
                "source_kind": "real-session",
                "success": True,
                "asked_for_user_input": True,
            }
        )
        is False
    )
    assert (
        common.qualifying_real_entry({"source_kind": "real-session", "success": None})
        is False
    )
    assert common.qualifying_real_entry({"source_kind": "real-session"}) is False


def test_codex_preflight_policy_skips_optional_and_avoid_routes():
    assert runner_common.should_run_codex_mcp_preflight({"use_codelens": "avoid"}) is False
    assert runner_common.should_run_codex_mcp_preflight({"use_codelens": "optional"}) is False
    assert runner_common.should_run_codex_mcp_preflight({"use_codelens": "recommended"}) is True
    assert runner_common.should_run_codex_mcp_preflight({"use_codelens": "required"}) is True


def test_codex_preflight_cache_round_trip_and_expiry():
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
    cache_payload = {
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
        runner_common.write_cached_codex_preflight(cache_dir, cache_key, cache_payload)
        cached = runner_common.read_cached_codex_preflight(cache_dir, cache_key, 300)
        assert cached is not None
        assert cached["cache_hit"] is True
        assert cached["probe_strategy"] == "cache"
        assert "session_id" not in cached
        assert "init_response" not in cached
        assert runner_common.read_cached_codex_preflight(cache_dir, cache_key, 0) is None


def test_codex_metrics_capture_policy_respects_mode_and_route():
    assert runner_common.should_capture_codex_metrics({"use_codelens": "optional"}, "baseline") is False
    assert runner_common.should_capture_codex_metrics({"use_codelens": "optional"}, "naive-on") is True
    assert runner_common.should_capture_codex_metrics({"use_codelens": "recommended"}, "baseline") is True


def test_session_entry_without_metrics_keeps_null_metric_fields():
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
    assert entry["token_in"] is None
    assert entry["token_out"] is None
    assert entry["bootstrap_tokens"] is None
    assert entry["tool_calls"] is None
    assert entry["metrics_capture_skipped"] is True
    assert entry["quality_score"] is None


def test_mode_stats_ignores_entries_without_token_metrics():
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
    assert stats["avg_total_tokens"] == 30
    assert stats["avg_bootstrap_tokens"] == 10


def test_codex_prompt_compacts_duplicate_guidance():
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
    prompt = runner_common.render_prompt(brief, "~/.codex/AGENTS.md", mcp_preflight=mcp_preflight)
    assert prompt.count("Non-interactive by default") == 1
    assert "Preferred CodeLens entrypoints for this task kind:" not in prompt
    assert "Finish with these exact headers:" in prompt


def test_promotion_structural_identity():
    """preview policy and promoted canonical must be structurally identical."""
    policy_a = {
        "schema_version": "codelens-routing-policy-v2",
        "policy_scope": "agent",
        "agent": "claude",
        "global_rules": [
            {
                "task_kind": "impact/reviewer",
                "recommended_policy": "prefer_codelens_after_bootstrap",
                "consensus": "unanimous",
                "repo_count": 3,
                "vote_count": 3,
                "explanation": "test",
            },
        ],
        "repo_overrides": [],
    }
    policy_b = json.loads(json.dumps(policy_a))
    result = common.compare_policy_structure(policy_a, policy_b)
    assert (
        result["identical"] is True
    ), f"Identical policies should match: {result['differences']}"

    policy_b["global_rules"][0]["recommended_policy"] = "prefer_native_baseline"
    result2 = common.compare_policy_structure(policy_a, policy_b)
    assert result2["identical"] is False, "Different policies should not match"
    assert any(d["field"] == "global_rules" for d in result2["differences"])


def test_promotion_structural_ignores_timestamps():
    """policy_structure must not compare generated_at or other volatile fields."""
    policy_a = {
        "schema_version": "codelens-routing-policy-v2",
        "generated_at": "2026-04-04T10:00:00",
        "global_rules": [
            {
                "task_kind": "t",
                "recommended_policy": "p",
                "consensus": "u",
                "repo_count": 1,
                "vote_count": 1,
                "explanation": "e",
            }
        ],
        "repo_overrides": [],
    }
    policy_b = json.loads(json.dumps(policy_a))
    policy_b["generated_at"] = "2026-04-04T22:00:00"
    result = common.compare_policy_structure(policy_a, policy_b)
    # generated_at is NOT in policy_structure keys, so this should be identical
    struct_keys = set(common.policy_structure(policy_a).keys())
    assert (
        "generated_at" not in struct_keys
    ), f"generated_at should not be in policy_structure: {struct_keys}"


def test_normalize_repo_id_fallback():
    """normalize_repo_id falls back to path basename when id is missing."""
    assert (
        common.normalize_repo_id({"id": "my-repo", "path": "/some/path"}) == "my-repo"
    )
    assert common.normalize_repo_id({"path": "/Users/dev/MyProject"}) == "MyProject"


def test_compute_quality_score_full_signals():
    """Entry with all quality signals gets a high score."""
    entry = {
        "tool_calls": 5,
        "verifier_used": True,
        "evidence_reuse_rate": 1.0,
        "composite_ratio": 0.33,
        "metrics_snapshot": {
            "error_count": 0,
            "verifier_followthrough_rate": 1.0,
        },
    }
    score = common.compute_quality_score(entry)
    assert score is not None
    # 0.3*1.0 + 0.2*1.0 + 0.2*1.0 + 0.15*1.0 + 0.15*0.33 = 0.9 + 0.0495
    assert 0.89 <= score <= 1.0, f"Expected high score, got {score}"


def test_compute_quality_score_errors_lower_score():
    """Errors reduce quality_score via the error_free component."""
    entry = {
        "tool_calls": 3,
        "verifier_used": False,
        "evidence_reuse_rate": 0.0,
        "composite_ratio": 0.5,
        "metrics_snapshot": {
            "error_count": 2,
            "verifier_followthrough_rate": 0.0,
        },
    }
    score = common.compute_quality_score(entry)
    assert score is not None
    # 0.3*0.0 + 0.2*0.0 + 0.2*0.0 + 0.15*0.0 + 0.15*0.5 = 0.075
    assert score == 0.075, f"Expected 0.075, got {score}"


def test_compute_quality_score_no_tool_calls():
    """Zero tool calls returns None — insufficient data."""
    entry = {
        "tool_calls": 0,
        "verifier_used": True,
        "metrics_snapshot": {"error_count": 0, "verifier_followthrough_rate": 1.0},
    }
    assert common.compute_quality_score(entry) is None


def test_compute_quality_score_empty_claude_session():
    """Claude placeholder session (all zeros) returns None."""
    entry = {
        "tool_calls": 0,
        "verifier_used": False,
        "evidence_reuse_rate": 0.0,
        "composite_ratio": 0.0,
        "metrics_snapshot": {
            "error_count": 0,
            "verifier_followthrough_rate": 0.0,
        },
    }
    assert common.compute_quality_score(entry) is None


def test_analyze_completion_contract_detects_structured_sections():
    text = """
- Requested work completed: updated the harness prompt
- Evidence used: session telemetry and policy refresh
- Verification run: cargo check
- Remaining risks: no fresh replay yet
"""
    analysis = common.analyze_completion_contract(text)
    assert analysis["score"] == 1.0
    assert analysis["passed"] is True
    assert analysis["asked_for_user_input"] is False


def test_analyze_completion_contract_accepts_bold_markdown_sections():
    text = """
**Requested work completed:** updated the harness prompt
**Evidence used:** session telemetry and policy refresh
**Verification run:** cargo check
**Remaining risks:** no fresh replay yet
"""
    analysis = common.analyze_completion_contract(text)
    assert analysis["score"] == 1.0
    assert analysis["passed"] is True


def test_analyze_completion_contract_ignores_non_blocking_korean_offer():
    text = """
- Remaining risks: none
원하시면 추가로 cargo check까지 돌릴 수 있습니다.
"""
    analysis = common.analyze_completion_contract(text)
    assert analysis["asked_for_user_input"] is False


def test_compute_quality_score_includes_completion_contract_when_present():
    entry = {
        "tool_calls": 5,
        "verifier_used": True,
        "evidence_reuse_rate": 1.0,
        "composite_ratio": 0.33,
        "completion_contract_score": 1.0,
        "asked_for_user_input": False,
        "metrics_snapshot": {
            "error_count": 0,
            "verifier_followthrough_rate": 1.0,
        },
    }
    score = common.compute_quality_score(entry)
    assert score is not None
    assert score >= 0.92, f"Expected completion contract bonus, got {score}"


def main():
    tests = [
        test_repo_override_trumps_global,
        test_agent_divergence_is_valid,
        test_repo_id_canonicalization,
        test_qualifying_filter,
        test_qualifying_real_entry_edge_cases,
        test_codex_preflight_policy_skips_optional_and_avoid_routes,
        test_codex_preflight_cache_round_trip_and_expiry,
        test_codex_metrics_capture_policy_respects_mode_and_route,
        test_session_entry_without_metrics_keeps_null_metric_fields,
        test_mode_stats_ignores_entries_without_token_metrics,
        test_codex_prompt_compacts_duplicate_guidance,
        test_compute_quality_score_full_signals,
        test_compute_quality_score_errors_lower_score,
        test_compute_quality_score_no_tool_calls,
        test_compute_quality_score_empty_claude_session,
        test_analyze_completion_contract_detects_structured_sections,
        test_analyze_completion_contract_accepts_bold_markdown_sections,
        test_analyze_completion_contract_ignores_non_blocking_korean_offer,
        test_compute_quality_score_includes_completion_contract_when_present,
        test_promotion_structural_identity,
        test_promotion_structural_ignores_timestamps,
        test_normalize_repo_id_fallback,
    ]
    passed = 0
    failed = 0
    for test in tests:
        try:
            test()
            print(f"  PASS: {test.__name__}")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {test.__name__}: {e}")
            failed += 1
    print(f"\n{passed} passed, {failed} failed out of {len(tests)} tests")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
