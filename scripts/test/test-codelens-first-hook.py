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
    env.pop("CODELENS_FIRST_ASSUME_ALIVE", None)
    # Hermetic default: point the health probe at a dead port so strict tests
    # behave identically on dev machines (live daemon) and CI (no daemon).
    # Tests that exercise a strict deny opt in via CODELENS_FIRST_ASSUME_ALIVE=1.
    env["CODELENS_CARD_URL"] = "http://127.0.0.1:9/dead"
    # Isolate HOME so hook metrics (~/.claude/metrics) never pollute the real
    # measurement file with test fixtures. Tests that need a specific HOME
    # (i/j) still override it via env_overrides.
    env["HOME"] = str(tmpdir)
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
        assert 'mcp__codelens__search(mode="symbol", name="handleRequest")' in ctx
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
            env_overrides={"CODELENS_FIRST_ASSUME_ALIVE": "1"},
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        hso = out["hookSpecificOutput"]
        assert hso["permissionDecision"] == "deny", hso
        reason = hso["permissionDecisionReason"]
        assert 'mcp__codelens__search(mode="symbol", name="handleRequest", include_body=true)' in reason
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
            env_overrides={"HOME": str(fake_home), "CODELENS_FIRST_ASSUME_ALIVE": "1"},
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        assert out["hookSpecificOutput"]["permissionDecision"] == "deny", out


def bash_event(cwd: Path, command: str, session_id: str) -> dict:
    return {
        "session_id": session_id,
        "cwd": str(cwd),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
    }


def test_l_bash_grep_symbol_advisory_allows_with_context() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, 'grep -rn "handleRequest" .', "sess-l"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        hso = out["hookSpecificOutput"]
        assert hso["permissionDecision"] == "allow", hso
        assert "handleRequest" in hso["additionalContext"]


def test_m_bash_rg_symbol_strict_denies_whole_command() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, 'rg "computeBudget" src/', "sess-m"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides={"CODELENS_FIRST_ASSUME_ALIVE": "1"},
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        hso = out["hookSpecificOutput"]
        assert hso["permissionDecision"] == "deny", hso
        assert 'mcp__codelens__search(mode="symbol", name="computeBudget", include_body=true)' in hso["permissionDecisionReason"]


def test_n_bash_grep_multi_alternative_passes_silently() -> None:
    # Real-world sample shape: `grep -rn "a\|b\|c" dir` — an OR-of-terms text
    # audit, not a single-symbol lookup. Grep stays the right tool for this.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, r'grep -rn "infer_harness_phase\|BUILD_SIGNAL" .', "sess-n"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"multi-alternative pattern must pass silently: {proc.stdout!r}"


def test_o_bash_grep_single_file_target_passes() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        target = project / "main.rs"
        target.write_text("fn handleRequest() {}\n", encoding="utf-8")
        proc = run_hook(
            bash_event(project, 'grep -n "handleRequest" main.rs', "sess-o"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"single-file bash grep must pass: {proc.stdout!r}"


def test_p_bash_non_grep_command_passes_silently() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, "cargo test -p codelens-mcp suggestions", "sess-p"),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"non-grep bash command must pass: {proc.stdout!r}"


def test_q_bash_compound_command_matches_embedded_grep_segment() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(
                project,
                'cargo build 2>&1 | tail -5; grep -rn "computeBudget" src/',
                "sess-q",
            ),
            tmpdir=Path(raw_tmp),
        )
        assert proc.returncode == 0, proc.stderr
        out = json.loads(proc.stdout)
        assert out["hookSpecificOutput"]["permissionDecision"] == "allow"
        assert "computeBudget" in out["hookSpecificOutput"]["additionalContext"]


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


_ALIVE = {"CODELENS_FIRST_ASSUME_ALIVE": "1"}


def test_r_bash_pipe_filter_grep_never_fires() -> None:
    # `ps aux | grep node` is output filtering — only pipeline HEADS are code
    # searches. Must stay silent even in strict with a live daemon.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, "ps aux | grep node", "sess-r"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides=_ALIVE,
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"pipe-filter grep must pass: {proc.stdout!r}"


def test_s_bash_escape_marker_passes() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        for marker in ("# [cl-text]", "# [cl-fallback]"):
            proc = run_hook(
                bash_event(project, f'rg "computeBudget" src/ {marker}', "sess-s"),
                tmpdir=Path(raw_tmp),
                mode="strict",
                env_overrides=_ALIVE,
            )
            assert proc.returncode == 0, proc.stderr
            assert proc.stdout == "", f"{marker} must pass: {proc.stdout!r}"


def test_t_bash_text_audit_flag_passes() -> None:
    # -i / -F / -v signal a text audit, not a symbol lookup.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        for flags in ("-i", "-F", "-v"):
            proc = run_hook(
                bash_event(project, f'rg {flags} "computeBudget" src/', "sess-t"),
                tmpdir=Path(raw_tmp),
                mode="strict",
                env_overrides=_ALIVE,
            )
            assert proc.returncode == 0, proc.stderr
            assert proc.stdout == "", f"rg {flags} must pass: {proc.stdout!r}"


def test_u_worktree_cwd_passes() -> None:
    # Builder worktrees edit files faster than the index refreshes — never gate.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = Path(raw) / "worktrees" / "wt-1"
        (project / ".codelens").mkdir(parents=True)
        proc = run_hook(
            bash_event(project, 'rg "computeBudget" src/', "sess-u"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides=_ALIVE,
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"worktree cwd must pass: {proc.stdout!r}"


def test_v_strict_deny_capped_per_session() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        tmpdir = Path(raw_tmp)  # shared deny-counter state across calls
        decisions = []
        for _ in range(5):
            proc = run_hook(
                bash_event(project, 'rg "computeBudget" src/', "sess-v"),
                tmpdir=tmpdir,
                mode="strict",
                env_overrides=_ALIVE,
            )
            if proc.stdout:
                out = json.loads(proc.stdout)
                decisions.append(out["hookSpecificOutput"]["permissionDecision"])
            else:
                decisions.append("")
        assert decisions == ["deny", "deny", "deny", "", ""], decisions


def test_w_strict_daemon_down_fails_open() -> None:
    # No ASSUME_ALIVE and the (hermetic, dead) probe endpoint → strict must
    # pass silently instead of denying toward a dead tool.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, 'rg "computeBudget" src/', "sess-w"),
            tmpdir=Path(raw_tmp),
            mode="strict",
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"daemon-down strict must fail open: {proc.stdout!r}"


def test_x_strict_repeat_deny_is_terse() -> None:
    # Deny #1 carries the full procedure; deny #2+ must be a short one-liner so
    # repeated redirects stay cheap in injected tokens.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        tmpdir = Path(raw_tmp)
        first = run_hook(
            bash_event(project, 'rg "computeBudget" src/', "sess-x"),
            tmpdir=tmpdir, mode="strict", env_overrides=_ALIVE,
        )
        second = run_hook(
            bash_event(project, 'rg "computeBudget" src/', "sess-x"),
            tmpdir=tmpdir, mode="strict", env_overrides=_ALIVE,
        )
        r1 = json.loads(first.stdout)["hookSpecificOutput"]["permissionDecisionReason"]
        r2 = json.loads(second.stdout)["hookSpecificOutput"]["permissionDecisionReason"]
        assert len(r1) > 300, f"first deny should carry the full procedure: {len(r1)}"
        assert len(r2) < 220, f"repeat deny must be terse: {len(r2)}"
        assert 'mcp__codelens__search(mode="symbol", name="computeBudget")' in r2


def test_y_strict_lowercase_downgrades_to_single_advisory() -> None:
    # Plain lowercase words are ambiguous — strict never denies them, and the
    # advisory is capped at 1/session in strict (advice has no repeat value).
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        tmpdir = Path(raw_tmp)
        first = run_hook(
            bash_event(project, 'rg "session" src/', "sess-y"),
            tmpdir=tmpdir, mode="strict", env_overrides=_ALIVE,
        )
        second = run_hook(
            bash_event(project, 'rg "session" src/', "sess-y"),
            tmpdir=tmpdir, mode="strict", env_overrides=_ALIVE,
        )
        out = json.loads(first.stdout)["hookSpecificOutput"]
        assert out["permissionDecision"] == "allow", out
        assert "session" in out["additionalContext"]
        assert second.stdout == "", f"strict advisory is capped at 1: {second.stdout!r}"


def test_z_bash_doc_target_passes() -> None:
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        proc = run_hook(
            bash_event(project, 'rg "computeBudget" README.md docs/notes.md', "sess-z"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides=_ALIVE,
        )
        assert proc.returncode == 0, proc.stderr
        assert proc.stdout == "", f"doc targets must pass: {proc.stdout!r}"


def test_aa_bash_project_root_absolute_path_is_in_project() -> None:
    # Regression: the project root itself (as an absolute grep target) must
    # NOT be misclassified as "outside the project" — that used to make the
    # single most common pattern (`grep -rn X <repo-root>`) bypass the hook
    # entirely, with no advisory and no strict deny.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        for target in (str(project), str(project) + "/"):
            proc = run_hook(
                bash_event(project, f'grep -rn "handleRequest" {target}', "sess-aa"),
                tmpdir=Path(raw_tmp),
            )
            assert proc.returncode == 0, proc.stderr
            out = json.loads(proc.stdout)
            assert out["hookSpecificOutput"]["permissionDecision"] == "allow", (target, proc.stdout)


def test_ab_bash_combined_short_flags_detect_text_audit() -> None:
    # Regression: shlex does not split `-rni` into `-r -n -i`, so an
    # exact-token check against `-i` alone missed it — a legitimate
    # case-insensitive text audit used to get wrongly denied in strict mode.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        for flags in ("-rni", "-in", "-vn"):
            proc = run_hook(
                bash_event(project, f'grep {flags} "computeBudget" src/', "sess-ab"),
                tmpdir=Path(raw_tmp),
                mode="strict",
                env_overrides=_ALIVE,
            )
            assert proc.returncode == 0, proc.stderr
            assert proc.stdout == "", f"grep {flags} must pass: {proc.stdout!r}"
        # Control: -rn alone (no i/F/v) carries no text-audit signal — must
        # still deny, so the fix isn't just silencing everything.
        control = run_hook(
            bash_event(project, 'grep -rn "computeBudget" src/', "sess-ab"),
            tmpdir=Path(raw_tmp),
            mode="strict",
            env_overrides=_ALIVE,
        )
        out = json.loads(control.stdout)["hookSpecificOutput"]
        assert out["permissionDecision"] == "deny", control.stdout


def test_ac_metric_file_rotates_past_size_cap() -> None:
    # Regression: metric() used to append forever with no cap — a sibling
    # PreToolUse hook's own metrics file reached 6.6MB this way. Once the file
    # crosses CODELENS_FIRST_METRIC_MAX_BYTES it must keep only the tail.
    with tempfile.TemporaryDirectory() as raw, tempfile.TemporaryDirectory() as raw_tmp:
        project = make_codelens_project(Path(raw))
        home = Path(raw_tmp)
        metrics_dir = home / ".claude" / "metrics"
        metrics_dir.mkdir(parents=True)
        metric_file = metrics_dir / "codelens-first.jsonl"
        old_lines = [json.dumps({"s": "old", "d": "pass", "i": i}) + "\n" for i in range(500)]
        metric_file.write_text("".join(old_lines), encoding="utf-8")
        assert metric_file.stat().st_size > 200, "fixture must exceed the tiny test cap below"

        proc = run_hook(
            grep_event(project, "handleRequest", "sess-ac"),
            tmpdir=home,
            env_overrides={"CODELENS_FIRST_METRIC_MAX_BYTES": "200"},
        )
        assert proc.returncode == 0, proc.stderr

        new_lines = metric_file.read_text(encoding="utf-8").splitlines()
        assert len(new_lines) < len(old_lines) + 1, (
            f"rotation must have trimmed old lines, got {len(new_lines)}"
        )
        assert json.loads(new_lines[-1])["d"] == "advise", "the new record must still land"


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
        test_l_bash_grep_symbol_advisory_allows_with_context,
        test_m_bash_rg_symbol_strict_denies_whole_command,
        test_n_bash_grep_multi_alternative_passes_silently,
        test_o_bash_grep_single_file_target_passes,
        test_p_bash_non_grep_command_passes_silently,
        test_q_bash_compound_command_matches_embedded_grep_segment,
        test_r_bash_pipe_filter_grep_never_fires,
        test_s_bash_escape_marker_passes,
        test_t_bash_text_audit_flag_passes,
        test_u_worktree_cwd_passes,
        test_v_strict_deny_capped_per_session,
        test_w_strict_daemon_down_fails_open,
        test_x_strict_repeat_deny_is_terse,
        test_y_strict_lowercase_downgrades_to_single_advisory,
        test_z_bash_doc_target_passes,
        test_aa_bash_project_root_absolute_path_is_in_project,
        test_ab_bash_combined_short_flags_detect_text_audit,
        test_ac_metric_file_rotates_past_size_cap,
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
