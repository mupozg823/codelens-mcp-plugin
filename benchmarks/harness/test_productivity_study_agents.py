#!/usr/bin/env python3
"""Unit tests for the thin Codex and Claude study adapters."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from productivity_study_agents import AgentInvocation, build_agent_command
from productivity_study_contract import Agent, Condition


def invocation(agent: Agent, condition: Condition, read_only: bool) -> AgentInvocation:
    return AgentInvocation(
        agent=agent,
        condition=condition,
        prompt="Inspect the repository.",
        worktree=Path("/tmp/study-worktree"),
        model="pinned-model",
        read_only=read_only,
        codelens_url="http://127.0.0.1:7837/mcp",
        claude_mcp_config=Path("/tmp/codelens-mcp.json"),
        routed_policy="Use native search for simple lookup; use CodeLens for multi-file impact.",
    )


def test_codex_baseline_has_no_mcp_configuration_and_is_read_only() -> None:
    command = build_agent_command(invocation(Agent.CODEX, Condition.BASELINE, True))

    assert command[:3] == ("codex", "exec", "--ephemeral")
    assert "--json" in command
    assert "--ignore-user-config" in command
    assert "read-only" in command
    assert not any("mcp_servers.codelens" in part for part in command)


def test_codex_routed_adds_mcp_and_fixed_policy() -> None:
    command = build_agent_command(invocation(Agent.CODEX, Condition.ROUTED, False))

    assert "workspace-write" in command
    assert any("mcp_servers.codelens.url" in part for part in command)
    assert command[-1].startswith("Controlled productivity study")
    assert "fixed routing policy" in command[-1]


def test_claude_uses_stream_json_and_strict_mcp_only_for_treatments() -> None:
    baseline = build_agent_command(invocation(Agent.CLAUDE, Condition.BASELINE, True))
    routed = build_agent_command(invocation(Agent.CLAUDE, Condition.ROUTED, False))

    assert baseline[:4] == ("claude", "--print", "--output-format", "stream-json")
    assert "--safe-mode" in baseline
    assert "--strict-mcp-config" not in baseline
    assert "plan" in baseline
    assert "--strict-mcp-config" in routed
    assert "--mcp-config" in routed
    assert "acceptEdits" in routed
    assert "fixed routing policy" in routed[-1]


def main() -> int:
    tests = [
        test_codex_baseline_has_no_mcp_configuration_and_is_read_only,
        test_codex_routed_adds_mcp_and_fixed_policy,
        test_claude_uses_stream_json_and_strict_mcp_only_for_treatments,
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
