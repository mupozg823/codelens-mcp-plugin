#!/usr/bin/env python3
"""Integration tests for shell-capable study process boundaries."""

from __future__ import annotations

import os
import sys
import tempfile
import time
from contextlib import contextmanager
from pathlib import Path
from typing import Iterator

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_home as home
import productivity_study_agent_process as agent_process
import productivity_study_review as review
import productivity_study_runner as runner
import productivity_study_runtime as runtime
from productivity_study_agents import AgentInvocation
from productivity_study_contract import Agent, Condition


def install_source_home(root: Path) -> tuple[Path, dict[str, str]]:
    source = root / "source-home"
    source.mkdir()
    for profile in (".bash_profile", ".profile", ".zprofile"):
        (source / profile).write_text(
            "export STUDY_PROFILE_SENTINEL=loaded\n"
            "export GIT_DIR=/tmp/profile-git-dir\n",
            encoding="utf-8",
        )
    original = os.environ.copy()
    os.environ["HOME"] = str(source)
    os.environ["STUDY_SOURCE_HOME"] = str(source)
    return source, original


def make_fake_codex(fake_bin: Path) -> None:
    executable = fake_bin / "codex"
    executable.write_text(
        """#!/bin/sh
test "$HOME" != "$STUDY_SOURCE_HOME" || exit 30
for shell in bash sh zsh; do
  "$shell" -lc 'test -z "${STUDY_PROFILE_SENTINEL+x}" && test -z "${GIT_DIR+x}"' || exit 31
done
printf '{"type":"item.completed","item":{"type":"agent_message","text":"%s"}}\n' "$HOME"
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}'
""",
        encoding="utf-8",
    )
    executable.chmod(0o755)


def test_run_agent_prepares_home_before_timing_and_cleans_afterward() -> None:
    # Given: a fake agent that rejects source login-profile state.
    with tempfile.TemporaryDirectory(prefix="codelens-study-boundary-") as raw_tmp:
        root = Path(raw_tmp)
        source, original_environment = install_source_home(root)
        fake_bin = root / "bin"
        fake_bin.mkdir()
        make_fake_codex(fake_bin)
        os.environ["PATH"] = f"{fake_bin}:{original_environment['PATH']}"
        candidate = root / "candidate"
        candidate.mkdir()
        active_homes: list[Path] = []
        events: list[str] = []
        real_boundary = agent_process.isolated_study_environment
        real_monotonic = time.monotonic

        @contextmanager
        def observed_boundary(
            excluded_root: Path,
            overlays: dict[str, str] | None = None,
        ) -> Iterator[dict[str, str]]:
            with home.isolated_study_environment(
                excluded_root, overlays
            ) as environment:
                process_home = Path(environment["HOME"])
                active_homes.append(process_home)
                events.append("setup")
                yield environment
            events.append("cleanup")
            active_homes.pop()

        def observed_monotonic() -> float:
            assert active_homes and active_homes[-1].exists()
            events.append("clock")
            return real_monotonic()

        agent_process.isolated_study_environment = observed_boundary
        agent_process.time.monotonic = observed_monotonic
        try:
            invocation = AgentInvocation(
                Agent.CODEX,
                Condition.BASELINE,
                "Inspect.",
                candidate,
                "fake",
                True,
                "",
                root / "unused-mcp.json",
                "",
            )

            # When: the agent boundary runs.
            result = runtime.run_agent(
                invocation,
                candidate,
                root / "agent.raw",
                timeout_seconds=5,
            )
        finally:
            agent_process.isolated_study_environment = real_boundary
            agent_process.time.monotonic = real_monotonic
            os.environ.clear()
            os.environ.update(original_environment)

        # Then: setup precedes every measured tick and cleanup follows it.
        observed_home = Path(str(result["response"]))
        assert result["agent_exit_code"] == 0
        assert events[0] == "setup"
        assert events[-1] == "cleanup"
        assert "clock" in events[1:-1]
        assert observed_home.is_relative_to(candidate) is False
        assert observed_home.exists() is False
        assert observed_home != source


def test_grader_and_blind_reviewer_use_ephemeral_homes() -> None:
    # Given: source profiles that would poison login descendants.
    with tempfile.TemporaryDirectory(prefix="codelens-study-boundary-") as raw_tmp:
        root = Path(raw_tmp)
        source, original_environment = install_source_home(root)
        candidate = root / "candidate"
        candidate.mkdir()
        review_workdir = root / "review-workdir"
        review_workdir.mkdir()
        grader_home_path = candidate / "grader-home.txt"
        try:
            # When: grading and the default blind executor create login shells.
            passed, failure = runner.run_verification_commands(
                candidate,
                (
                    'bash -lc \'test -z "${STUDY_PROFILE_SENTINEL+x}" && test -z "${GIT_DIR+x}" && printf %s "$HOME" > grader-home.txt\' '
                    '&& sh -lc \'test -z "${STUDY_PROFILE_SENTINEL+x}" && test -z "${GIT_DIR+x}"\' '
                    '&& zsh -lc \'test -z "${STUDY_PROFILE_SENTINEL+x}" && test -z "${GIT_DIR+x}"\'',
                ),
            )
            blind_output = review.default_executor(
                Agent.CODEX,
                (
                    "bash",
                    "-lc",
                    'printf \'%s|%s|%s\' "${STUDY_PROFILE_SENTINEL-unset}" "${GIT_DIR-unset}" "$HOME"',
                ),
                review_workdir,
            )
        finally:
            os.environ.clear()
            os.environ.update(original_environment)

        # Then: neither process sees the source profiles, and both homes are gone.
        grader_home = Path(grader_home_path.read_text(encoding="utf-8"))
        sentinel, git_dir, blind_home_raw = blind_output.strip().split("|")
        blind_home = Path(blind_home_raw)
        assert passed is True, failure
        assert sentinel == "unset"
        assert git_dir == "unset"
        assert grader_home != source
        assert blind_home != source
        assert grader_home.exists() is False
        assert blind_home.exists() is False


def test_dedicated_daemon_receives_an_ephemeral_home() -> None:
    # Given: a daemon process boundary and a source home with login profiles.
    with tempfile.TemporaryDirectory(prefix="codelens-study-boundary-") as raw_tmp:
        root = Path(raw_tmp)
        source, original_environment = install_source_home(root)
        candidate = root / "candidate"
        candidate.mkdir()
        captured: dict[str, str] = {}
        real_snapshot = runtime.bound_binary_snapshot
        real_environment = runtime.isolated_study_environment
        real_popen = runtime.subprocess.Popen
        real_port = runtime.unused_local_port
        real_session = runtime.open_mcp_session

        @contextmanager
        def fake_snapshot(binary: Path, _digest: str) -> Iterator[Path]:
            yield binary

        @contextmanager
        def observed_environment(
            excluded_root: Path,
            overlays: dict[str, str] | None = None,
        ) -> Iterator[dict[str, str]]:
            with home.isolated_study_environment(
                excluded_root, overlays
            ) as environment:
                captured.update(environment)
                yield environment

        class FakeProcess:
            pid = 1234

            def terminate(self) -> None:
                return None

            def wait(self, timeout: int) -> int:
                assert timeout == 5
                return 0

        def fake_popen(
            _command: tuple[str, ...],
            *,
            stdout: int,
            stderr: int,
            env: dict[str, str],
        ) -> FakeProcess:
            assert stdout == runtime.subprocess.DEVNULL
            assert stderr == runtime.subprocess.DEVNULL
            assert env["HOME"] == captured["HOME"]
            return FakeProcess()

        runtime.bound_binary_snapshot = fake_snapshot
        runtime.isolated_study_environment = observed_environment
        runtime.subprocess.Popen = fake_popen
        runtime.unused_local_port = lambda: 17839
        runtime.open_mcp_session = lambda _url: "health-session"
        try:
            # When: the daemon context starts and stops.
            with runtime.dedicated_daemon(
                root / "codelens-mcp",
                candidate,
                root / "telemetry.raw",
                expected_sha256="pinned",
            ) as daemon:
                assert daemon.health_session_id == "health-session"
        finally:
            runtime.bound_binary_snapshot = real_snapshot
            runtime.isolated_study_environment = real_environment
            runtime.subprocess.Popen = real_popen
            runtime.unused_local_port = real_port
            runtime.open_mcp_session = real_session
            os.environ.clear()
            os.environ.update(original_environment)

        # Then: the daemon receives the same private contract and it is removed.
        daemon_home = Path(captured["HOME"])
        assert daemon_home != source
        assert daemon_home.is_relative_to(candidate) is False
        assert captured["CODELENS_TELEMETRY_PATH"] == str(root / "telemetry.raw")
        assert daemon_home.exists() is False


def main() -> int:
    tests = (
        test_run_agent_prepares_home_before_timing_and_cleans_afterward,
        test_grader_and_blind_reviewer_use_ephemeral_homes,
        test_dedicated_daemon_receives_an_ephemeral_home,
    )
    for test in tests:
        test()
        print(f"PASS  {test.__name__}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
