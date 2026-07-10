"""Thin CLI command adapters for controlled Codex and Claude study runs."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from productivity_study_contract import Agent, Condition


@dataclass(frozen=True, slots=True)
class AgentInvocation:
    agent: Agent
    condition: Condition
    prompt: str
    worktree: Path
    model: str
    read_only: bool
    codelens_url: str
    claude_mcp_config: Path
    routed_policy: str


def build_agent_command(invocation: AgentInvocation) -> tuple[str, ...]:
    prompt = build_study_prompt(invocation)
    match invocation.agent:
        case Agent.CODEX:
            return build_codex_command(invocation, prompt)
        case Agent.CLAUDE:
            return build_claude_command(invocation, prompt)


def build_codex_command(invocation: AgentInvocation, prompt: str) -> tuple[str, ...]:
    command = [
        "codex",
        "exec",
        "--ephemeral",
        "--json",
        "--ignore-user-config",
        "--model",
        invocation.model,
        "--sandbox",
        "read-only" if invocation.read_only else "workspace-write",
        "--cd",
        str(invocation.worktree),
    ]
    if invocation.condition is not Condition.BASELINE:
        command.extend(
            [
                "--config",
                f'mcp_servers.codelens.url="{invocation.codelens_url}"',
            ]
        )
    command.append(prompt)
    return tuple(command)


def build_claude_command(invocation: AgentInvocation, prompt: str) -> tuple[str, ...]:
    command = [
        "claude",
        "--print",
        "--output-format",
        "stream-json",
        "--no-session-persistence",
        "--model",
        invocation.model,
        "--permission-mode",
        "plan" if invocation.read_only else "acceptEdits",
    ]
    if invocation.condition is Condition.BASELINE:
        command.append("--safe-mode")
    else:
        command.extend(
            [
                "--strict-mcp-config",
                "--mcp-config",
                str(invocation.claude_mcp_config),
            ]
        )
    command.append(prompt)
    return tuple(command)


def build_study_prompt(invocation: AgentInvocation) -> str:
    condition_text = condition_instruction(invocation.condition, invocation.routed_policy)
    edit_text = "Do not edit files." if invocation.read_only else "Edit only as needed for the task."
    return "\n".join(
        (
            "Controlled productivity study. Complete the task using only the checked-out worktree.",
            condition_text,
            edit_text,
            "Do not refresh, apply, rewrite, or otherwise mutate routing policy during this run.",
            "Record actual verification in the final response; do not claim unrun checks passed.",
            "",
            "Task:",
            invocation.prompt,
        )
    )


def condition_instruction(condition: Condition, routed_policy: str) -> str:
    match condition:
        case Condition.BASELINE:
            return "CodeLens is unavailable in this run; use native repository tools only."
        case Condition.NAIVE:
            return "CodeLens is configured, but this run provides no routing recommendation."
        case Condition.ROUTED:
            return f"Use this fixed routing policy without changing it: {routed_policy}"
