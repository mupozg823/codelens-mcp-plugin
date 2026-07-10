#!/usr/bin/env python3
"""Tests for process-wide productivity-study Git environment isolation."""

from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_candidate as candidate
import productivity_study_runtime as runtime
import productivity_study_runner as runner
from productivity_study_agents import AgentInvocation
from productivity_study_contract import Agent, Condition


SAFE_GIT_ENVIRONMENT = {
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_CONFIG_COUNT": "1",
    "GIT_CONFIG_KEY_0": "core.hooksPath",
    "GIT_CONFIG_VALUE_0": os.devnull,
}


def install_malicious_environment(root: Path, fake_bin: Path) -> dict[str, str]:
    original = os.environ.copy()
    malicious_global = root / "malicious.gitconfig"
    malicious_global.write_text(
        "[core]\n\thooksPath = /tmp/global-hooks\n", encoding="utf-8"
    )
    os.environ.update(
        {
            "PATH": f"{fake_bin}:{original['PATH']}",
            "STUDY_AUTH_TOKEN": "preserved",
            "GIT_DIR": "/tmp/poison.git",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES": "/tmp/poison-objects",
            "GIT_CONFIG_GLOBAL": str(malicious_global),
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "core.hooksPath",
            "GIT_CONFIG_VALUE_0": "/tmp/injected-hooks",
            "GIT_SSH_COMMAND": "exit 99",
        }
    )
    return original


def assert_safe_environment(environment: dict[str, str]) -> None:
    git_values = {
        key: value for key, value in environment.items() if key.startswith("GIT_")
    }
    assert git_values == SAFE_GIT_ENVIRONMENT
    assert environment["STUDY_AUTH_TOKEN"] == "preserved"
    assert environment["HOME"] == os.environ["HOME"]
    assert environment["PATH"] == os.environ["PATH"]


def test_generated_daemon_agent_and_grading_environments_remove_ambient_git_state() -> (
    None
):
    with tempfile.TemporaryDirectory(prefix="codelens-study-environment-") as raw_tmp:
        root = Path(raw_tmp)
        fake_bin = root / "bin"
        fake_bin.mkdir()
        fake_codex = fake_bin / "codex"
        fake_codex.write_text(
            """#!/bin/sh
test -z "${GIT_DIR+x}" || exit 31
test -z "${GIT_ALTERNATE_OBJECT_DIRECTORIES+x}" || exit 32
test -z "${GIT_SSH_COMMAND+x}" || exit 33
test "$GIT_CONFIG_NOSYSTEM" = "1" || exit 34
test "$GIT_CONFIG_GLOBAL" = "/dev/null" || exit 35
test "$GIT_TERMINAL_PROMPT" = "0" || exit 36
test "$GIT_CONFIG_KEY_0" = "core.hooksPath" || exit 37
test "$GIT_CONFIG_VALUE_0" = "/dev/null" || exit 38
test "$STUDY_AUTH_TOKEN" = "preserved" || exit 39
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"environment clean"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}'
""",
            encoding="utf-8",
        )
        fake_codex.chmod(0o755)
        original = install_malicious_environment(root, fake_bin)
        try:
            generated = candidate.study_process_environment()
            assert_safe_environment(generated)

            telemetry_path = root / "telemetry.jsonl"
            daemon_environment = runtime.daemon_environment(telemetry_path)
            assert_safe_environment(daemon_environment)
            assert daemon_environment["CODELENS_TELEMETRY_PATH"] == str(telemetry_path)

            worktree = root / "worktree"
            worktree.mkdir()
            raw_path = root / "agent.raw"
            invocation = AgentInvocation(
                agent=Agent.CODEX,
                condition=Condition.BASELINE,
                prompt="Inspect the environment.",
                worktree=worktree,
                model="fake-model",
                read_only=True,
                codelens_url="",
                claude_mcp_config=root / "unused-mcp.json",
                routed_policy="",
            )
            agent_result = runtime.run_agent(
                invocation, worktree, raw_path, timeout_seconds=5
            )
            assert agent_result["agent_exit_code"] == 0
            assert agent_result["response"] == "environment clean"

            verification = worktree / "verify_environment.py"
            verification.write_text(
                """import os
import subprocess

safe = {
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_CONFIG_COUNT": "1",
    "GIT_CONFIG_KEY_0": "core.hooksPath",
    "GIT_CONFIG_VALUE_0": "/dev/null",
}
observed = {key: value for key, value in os.environ.items() if key.startswith("GIT_")}
assert observed == safe, observed
assert os.environ["STUDY_AUTH_TOKEN"] == "preserved"
hook = subprocess.run(
    ["git", "config", "--get", "core.hooksPath"],
    check=True,
    capture_output=True,
    text=True,
)
assert hook.stdout.strip() == "/dev/null"
""",
                encoding="utf-8",
            )
            passed, failure = runner.run_verification_commands(
                worktree, ("python3 verify_environment.py",)
            )
            assert passed is True, failure
        finally:
            os.environ.clear()
            os.environ.update(original)


def main() -> int:
    test_generated_daemon_agent_and_grading_environments_remove_ambient_git_state()
    print(
        "PASS  test_generated_daemon_agent_and_grading_environments_remove_ambient_git_state"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
