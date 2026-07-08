#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-codelens-first-hook.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-codelens-first-hook.py
# ------------------
#
# Feeds stdin fixtures to hooks/codelens-first.py via subprocess and asserts the
# PreToolUse decision. Each case isolates its own TMPDIR (session throttle state)
# and environment so ordering never leaks between tests.

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
HOOK = REPO_ROOT / "hooks" / "codelens-first.py"


def make_codelens_project(tmp: Path) -> Path:
    """Create a project dir containing a .codelens/ index dir; return the project dir."""
    project = tmp / "project"
    (project / ".codelens").mkdir(parents=True)
    return project


def run_hook(
    stdin_obj: object,
    *,
    tmpdir: Path,
    mode: str | None = None,
    env_overrides: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.pop("CODELENS_FIRST_MODE", None)
    env["TMPDIR"] = str(tmpdir)
    if mode is not None:
        env["CODELENS_FIRST_MODE"] = mode
    if env_overrides:
        env.update(env_overrides)
    payload = stdin_obj if isinstance(stdin_obj, str) else json.dumps(stdin_obj)
    return subprocess.run(
        ["python3", str(HOOK)],
        input=payload,
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
        env=env,
    )


def grep_event(cwd: Path, pattern: str, session_id: str, path: str | None = None) -> dict:
    tool_input: dict[str, object] = {"pattern": pattern}
    if path is not None:
        tool_input["path"] = path
    return {
        "session_id": session_id,
        "cwd": str(cwd),
        "hook_event_name": "PreToolUse",
        "tool_name": "Grep",
        "tool_input": tool_input,
    }


def test_a_symbol_advisory_allows_with_context() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            grep_event(project, "handleRequest", "sess-a"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout, "advisory should emit a decision"
        out = json.loads(proc.stdout)
        hso = out["hookSpecificOutput"]
        assert hso["hookEventName"] == "PreToolUse"
        assert hso["permissionDecision"] == "allow", hso
        ctx = hso["additionalContext"]
        assert "handleRequest" in ctx
        assert "mcp__codelens__find_symbol" in ctx
        assert len(ctx) <= 300, f"advisory context must stay bounded: {len(ctx)}"


def test_b_regex_metachar_passes_silently() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            grep_event(project, "foo.*bar", "sess-b"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"metachar pattern must pass with no output: {proc.stdout!r}"


def test_c_no_codelens_passes_even_in_strict() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        # No .codelens dir anywhere in this tree.
        bare = Path(raw) / "bare"
        bare.mkdir()
        proc = run_hook(
            grep_event(bare, "handleRequest", "sess-c"),
            tmpdir=Path(raw_tmp),
            mode="strict",
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"no index must pass even in strict: {proc.stdout!r}"


def test_d_strict_symbol_denies_with_reason() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            grep_event(project, "handleRequest", "sess-d"),
            tmpdir=Path(raw_tmp),
            mode="strict",
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        hso = out["hookSpecificOutput"]
        assert hso["permissionDecision"] == "deny", hso
        reason = hso["permissionDecisionReason"]
        assert 'mcp__codelens__find_symbol(name="handleRequest")' in reason
        assert "strict" in reason


def test_e_third_advisory_is_throttled() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        tmpdir = Path(raw_tmp)  # shared throttle state across the 3 calls
        first = run_hook(grep_event(project, "handleRequest", "sess-e"), tmpdir=tmpdir)
        second = run_hook(grep_event(project, "handleRequest", "sess-e"), tmpdir=tmpdir)
        third = run_hook(grep_event(project, "handleRequest", "sess-e"), tmpdir=tmpdir)
        assert first.stdout, "1st advisory should emit"
        assert second.stdout, "2nd advisory should emit"
        assert third.stdout == "", f"3rd advisory must be throttled: {third.stdout!r}"
        assert third.returncode == 0


def test_f_broken_stdin_fails_open() -> None:
    with tempfile.TemporaryDirectory() as raw_tmp:
        proc = run_hook("this is not json {", tmpdir=Path(raw_tmp))
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"broken stdin must fail open: {proc.stdout!r}"


def test_g_off_mode_passes_silently() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            grep_event(project, "handleRequest", "sess-g"),
            tmpdir=Path(raw_tmp),
            mode="off",
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"off mode must pass with no output: {proc.stdout!r}"


def test_h_single_file_path_passes() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        target = project / "main.rs"
        target.write_text("fn handleRequest() {}\n", encoding="utf-8")
        proc = run_hook(
            grep_event(project, "handleRequest", "sess-h", path="main.rs"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"single-file audit must pass: {proc.stdout!r}"


def test_i_global_home_codelens_is_not_a_project_index() -> None:
    # Regression (P1): the global ~/.codelens data dir shares the project marker's
    # basename. A project under the home tree WITHOUT its own .codelens must not be
    # gated just because $HOME/.codelens exists — otherwise strict would deny Grep
    # across the whole home tree (reproduced at cwd=~/Downloads).
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        fake_home = Path(raw) / "home"
        (fake_home / ".codelens").mkdir(parents=True)  # global data dir
        project = fake_home / "sub" / "project"  # NO .codelens of its own
        project.mkdir(parents=True)
        proc = run_hook(
            grep_event(project, "handleRequest", "sess-i"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides={"HOME": str(fake_home)},
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", (
            f"global ~/.codelens must not gate a non-CodeLens project: {proc.stdout!r}"
        )


def test_j_project_local_codelens_under_home_still_gates() -> None:
    # Positive control for the fix: a real project-local .codelens under the home
    # tree (even with a colliding global $HOME/.codelens present) must still gate.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        fake_home = Path(raw) / "home"
        (fake_home / ".codelens").mkdir(parents=True)  # global data dir
        project = fake_home / "proj"
        (project / ".codelens").mkdir(parents=True)  # project-local index
        nested = project / "src"
        nested.mkdir()
        proc = run_hook(
            grep_event(nested, "handleRequest", "sess-j"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides={"HOME": str(fake_home)},
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        assert out["hookSpecificOutput"]["permissionDecision"] == "deny", out


def test_k_temp_root_codelens_is_not_a_project_index() -> None:
    # A .codelens sitting at a temp root is global scratch, not a project marker
    # (mirrors codelens-engine is_temp_root). A subdir project without its own
    # .codelens must not be gated by it.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        temp_root = Path(raw) / "tmproot"
        (temp_root / ".codelens").mkdir(parents=True)  # scratch at the temp root
        project = temp_root / "project"  # NO .codelens of its own
        project.mkdir()
        proc = run_hook(
            grep_event(project, "handleRequest", "sess-k"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides={"TMPDIR": str(temp_root)},
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", (
            f".codelens at a temp root must not gate a subdir project: {proc.stdout!r}"
        )


def main() -> int:
    tests = [
        test_a_symbol_advisory_allows_with_context,
        test_b_regex_metachar_passes_silently,
        test_c_no_codelens_passes_even_in_strict,
        test_d_strict_symbol_denies_with_reason,
        test_e_third_advisory_is_throttled,
        test_f_broken_stdin_fails_open,
        test_g_off_mode_passes_silently,
        test_h_single_file_path_passes,
        test_i_global_home_codelens_is_not_a_project_index,
        test_j_project_local_codelens_under_home_still_gates,
        test_k_temp_root_codelens_is_not_a_project_index,
    ]
    failures: list[str] = []
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as error:
            print(f"FAIL  {test.__name__}: {error}")
            failures.append(test.__name__)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
