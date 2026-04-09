#!/usr/bin/env python3
"""Shared agent registry for harness policy export/apply/bootstrap flows."""

from __future__ import annotations

from pathlib import Path


CODEX_HOME = Path.home() / ".codex"
CLAUDE_HOME = Path.home() / ".claude"
SHARED_POLICY_DIR = CODEX_HOME / "harness" / "policies"
DEFAULT_OVERRIDE_DIR = SHARED_POLICY_DIR / "repo-overrides"

SHARED_POLICY = {
    "canonical_policy_json": SHARED_POLICY_DIR / "codelens-routing-policy.shared.json",
    "canonical_policy_markdown": SHARED_POLICY_DIR / "codelens-routing-policy.shared.md",
}
REPO_CONTRACT_NAMES = (
    "EVAL_CONTRACT.md",
    "docs/platform-setup.md",
)

AGENT_REGISTRY = {
    "codex": {
        "label": "Codex",
        "global_instruction_path": CODEX_HOME / "AGENTS.md",
        "global_instruction_label": "~/.codex/AGENTS.md",
        "repo_instruction_name": "AGENTS.md",
        "repo_contract_names": REPO_CONTRACT_NAMES,
        "policy_output_dir": SHARED_POLICY_DIR,
        "canonical_policy_json": SHARED_POLICY_DIR / "codelens-routing-policy.json",
        "canonical_policy_markdown": SHARED_POLICY_DIR / "codelens-routing-policy.md",
        "bootstrap_output_dir": CODEX_HOME / "harness" / "bootstrap",
        "prompt_dir": CODEX_HOME / "harness" / "bootstrap" / "prompts",
        "run_dir": CODEX_HOME / "harness" / "runs",
        "wrapper_path": CODEX_HOME / "harness" / "bin" / "codex-harness-task",
        "override_suffix": "codex",
    },
    "claude": {
        "label": "Claude",
        "global_instruction_path": CLAUDE_HOME / "CLAUDE.md",
        "global_instruction_label": "~/.claude/CLAUDE.md",
        "repo_instruction_name": "CLAUDE.md",
        "repo_contract_names": REPO_CONTRACT_NAMES,
        "policy_output_dir": CLAUDE_HOME / "harness" / "policies",
        "canonical_policy_json": CLAUDE_HOME / "harness" / "policies" / "codelens-routing-policy.json",
        "canonical_policy_markdown": CLAUDE_HOME / "harness" / "policies" / "codelens-routing-policy.md",
        "bootstrap_output_dir": CLAUDE_HOME / "harness" / "bootstrap",
        "prompt_dir": CLAUDE_HOME / "harness" / "bootstrap" / "prompts",
        "run_dir": CLAUDE_HOME / "harness" / "runs",
        "wrapper_path": CLAUDE_HOME / "harness" / "bin" / "claude-harness-task",
        "override_suffix": "claude",
    },
}


def normalize_agent(agent: str) -> str:
    return agent.strip().lower()


def agent_names() -> tuple[str, ...]:
    return tuple(AGENT_REGISTRY.keys())


def required_agents() -> list[str]:
    return list(agent_names())


def get_agent(agent: str) -> dict:
    normalized = normalize_agent(agent)
    if normalized not in AGENT_REGISTRY:
        raise KeyError(f"Unknown agent `{agent}`")
    return AGENT_REGISTRY[normalized]


def agent_label(agent: str) -> str:
    return str(get_agent(agent)["label"])
