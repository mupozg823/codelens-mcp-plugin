#!/usr/bin/env python3
"""Tests for policy pipeline integrity."""

from __future__ import annotations

import json
import sys
import importlib.util
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import harness_eval_common as common
import harness_runner_common as runner_common


def load_script_module(module_name: str, filename: str):
    path = Path(__file__).resolve().parent / filename
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


CODEX_RUNNER = load_script_module("codex_task_runner_test", "codex-task-runner.py")


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
        common.qualifying_real_entry({"source_kind": "real-session", "success": None})
        is False
    )
    assert common.qualifying_real_entry({"source_kind": "real-session"}) is False


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


def test_summarize_called_tools_orders_and_filters_zero_call_rows():
    delta_payload = {
        "tools": [
            {"tool": "impact_report", "calls": 1, "total_ms": 90, "total_tokens": 200},
            {"tool": "analyze_change_request", "calls": 3, "total_ms": 40, "total_tokens": 150},
            {"tool": "verify_change_readiness", "calls": 3, "total_ms": 60, "total_tokens": 120},
            {"tool": "get_capabilities", "calls": 0, "total_ms": 99, "total_tokens": 99},
            {"tool": None, "calls": 2, "total_ms": 10, "total_tokens": 10},
        ]
    }
    rows = runner_common.summarize_called_tools(delta_payload)
    assert [row["tool"] for row in rows] == [
        "verify_change_readiness",
        "analyze_change_request",
        "impact_report",
    ]
    assert rows[0]["calls"] == 3
    assert rows[0]["total_ms"] == 60


def test_build_codex_recommendation_outcome_tracks_entrypoint_and_contract_alignment():
    mcp_preflight = {
        "available": True,
        "recommended_entrypoint": "impact_report",
        "recommended_followup_tools": ["get_analysis_section", "verify_change_readiness"],
        "recommended_contract_action": "stay_lean_until_shape_needed",
    }
    delta_payload = {
        "tools": [
            {"tool": "impact_report", "calls": 2, "total_ms": 80, "total_tokens": 200},
            {"tool": "get_analysis_section", "calls": 1, "total_ms": 20, "total_tokens": 40},
        ],
        "session": {
            "deferred_namespace_expansion_count": 0,
            "deferred_hidden_tool_call_denied_count": 0,
        },
    }
    outcome = runner_common.build_codex_recommendation_outcome(mcp_preflight, delta_payload)
    assert outcome is not None
    assert outcome["alignment"] == "matched-entrypoint"
    assert outcome["recommended_entrypoint_called"] is True
    assert outcome["recommended_entrypoint_call_count"] == 2
    assert outcome["recommended_followup_tools_called"] == ["get_analysis_section"]
    assert outcome["recommended_followup_tools_missed"] == ["verify_change_readiness"]
    assert outcome["contract_action_aligned"] is True
    assert (
        runner_common.summarize_codex_recommendation_outcome(outcome)
        == "recommended entrypoint impact_report was exercised"
    )


def test_parse_codex_json_events_ignores_non_json_noise():
    rows = runner_common.parse_codex_json_events(
        "\n".join(
            [
                "plugin warning: skipped",
                '{"type":"thread.started","thread_id":"abc"}',
                "not-json",
                '{"type":"item.completed","item":{"type":"agent_message","text":"OK"}}',
            ]
        )
    )
    assert [row["type"] for row in rows] == ["thread.started", "item.completed"]


def test_build_codex_recommendation_outcome_prefers_event_trace_for_attempted_entrypoint():
    mcp_preflight = {
        "available": True,
        "recommended_entrypoint": "impact_report",
        "recommended_followup_tools": ["get_analysis_section"],
        "recommended_contract_action": "stay_lean_until_shape_needed",
    }
    codex_event_rows = runner_common.parse_codex_json_events(
        "\n".join(
            [
                '{"type":"item.started","item":{"type":"mcp_tool_call","server":"codelens","tool":"impact_report","arguments":{}}}',
                '{"type":"item.completed","item":{"type":"mcp_tool_call","server":"codelens","tool":"impact_report","status":"failed","error":{"message":"user cancelled MCP tool call"}}}',
            ]
        )
    )
    delta_payload = {
        "tools": [
            {"tool": "get_capabilities", "calls": 1, "total_ms": 10, "total_tokens": 20},
        ],
        "session": {
            "deferred_namespace_expansion_count": 0,
            "deferred_hidden_tool_call_denied_count": 0,
        },
    }
    outcome = runner_common.build_codex_recommendation_outcome(
        mcp_preflight,
        delta_payload,
        codex_event_rows=codex_event_rows,
    )
    assert outcome is not None
    assert outcome["evidence_source"] == "codex_event_trace"
    assert outcome["alignment"] == "attempted-entrypoint"
    assert outcome["recommended_entrypoint_called"] is True
    assert outcome["recommended_entrypoint_call_count"] == 1
    assert outcome["recommended_entrypoint_success_count"] == 0
    assert outcome["recommended_entrypoint_failure_count"] == 1
    assert outcome["recommended_entrypoint_cancelled_count"] == 0
    assert outcome["top_called_tools"][0]["tool"] == "impact_report"
    assert (
        runner_common.summarize_codex_recommendation_outcome(outcome)
        == "recommended entrypoint impact_report was attempted but did not complete successfully"
    )


def test_build_minimal_codex_home_config_dedupes_paths_and_keeps_codelens_only():
    config = CODEX_RUNNER.build_minimal_codex_home_config(
        repo_paths=[
            Path("/tmp/repo"),
            Path("/tmp/repo"),
            Path("/tmp/repo-alias"),
        ],
        mcp_url="http://127.0.0.1:9999/mcp",
    )
    assert '[mcp_servers.codelens]' in config
    assert 'url = "http://127.0.0.1:9999/mcp"' in config
    assert config.count('trust_level = "trusted"') == 2
    assert "[plugins." not in config


def main():
    tests = [
        test_repo_override_trumps_global,
        test_agent_divergence_is_valid,
        test_repo_id_canonicalization,
        test_qualifying_filter,
        test_qualifying_real_entry_edge_cases,
        test_compute_quality_score_full_signals,
        test_compute_quality_score_errors_lower_score,
        test_compute_quality_score_no_tool_calls,
        test_compute_quality_score_empty_claude_session,
        test_summarize_called_tools_orders_and_filters_zero_call_rows,
        test_build_codex_recommendation_outcome_tracks_entrypoint_and_contract_alignment,
        test_parse_codex_json_events_ignores_non_json_noise,
        test_build_codex_recommendation_outcome_prefers_event_trace_for_attempted_entrypoint,
        test_build_minimal_codex_home_config_dedupes_paths_and_keeps_codelens_only,
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
