#!/usr/bin/env python3
# codelens-first PreToolUse hook — nudge symbol lookups from native Grep to CodeLens.
#
# CONTRACT
#   stdin  : PreToolUse JSON — {session_id, cwd, tool_name:"Grep",
#            tool_input:{pattern, path, ...}}
#   stdout : either nothing (no decision → tool proceeds) or one line of
#            {"hookSpecificOutput":{"hookEventName":"PreToolUse",
#             "permissionDecision":"allow"|"deny",
#             "permissionDecisionReason":..., "additionalContext":...}}
#   exit   : always 0 (JSON on stdout carries the decision; fail-open by design).
#
# MODES (env CODELENS_FIRST_MODE)
#   advisory (default) : symbol-like Grep → allow + advice (throttled ≤2/session)
#   strict             : symbol-like Grep → deny + concrete CodeLens call
#   off                : do nothing
#
# GATE
#   Only acts when a PROJECT-local .codelens/ dir exists at/above cwd (≤5 levels).
#   The global ~/.codelens data dir (and a .codelens at a temp root) is excluded —
#   its basename collides with the project marker but it is not a project index.
#   No index → the project does not use CodeLens → pass unconditionally (strict
#   included). A single-file tool_input.path is a targeted text audit → pass.
#   Non-symbol / regex-metachar patterns → pass. Any exception → pass (fail-open).
#
# Output carries no timestamps or absolute paths (prompt-cache hygiene).
# Stdlib only, no external dependencies.

from __future__ import annotations

import json
import os
import re
import sys

SYMBOL_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*$")
MAX_ADVISORIES_PER_SESSION = 2
GATE_MAX_LEVELS = 5


def symbol_form(pattern: object) -> str | None:
    """Return the clean symbol if the Grep pattern is a symbol lookup, else None."""
    if not isinstance(pattern, str):
        return None
    s = pattern
    if s.startswith("\\b"):
        s = s[2:]
    if s.endswith("\\b"):
        s = s[:-2]
    if len(s) < 3:
        return None
    # SYMBOL_RE only admits word chars + `::`, so any regex metachar
    # (.*+?[](){}| etc.) that survived the \b strip fails the match.
    return s if SYMBOL_RE.match(s) else None


# Standard global-temp roots whose `.codelens` (if any) is scratch, not a project
# marker — mirrors codelens-engine's project::root_detect::is_temp_root.
_TEMP_ROOTS = ("/tmp", "/private/tmp", "/var/tmp")


def _canonical(path: str) -> str:
    try:
        return os.path.realpath(path)
    except OSError:
        return os.path.abspath(path)


def _home_dir() -> str | None:
    home = os.environ.get("HOME")
    return _canonical(home) if home else None


def _is_temp_root(path: str) -> bool:
    for candidate in (os.environ.get("TMPDIR"), *_TEMP_ROOTS):
        if candidate and _canonical(candidate) == path:
            return True
    return False


def has_codelens_index(cwd: object) -> bool:
    """True only when a PROJECT-local `.codelens/` marker exists at/above cwd.

    The global CodeLens data dir lives at `~/.codelens`, whose basename collides
    with the project-local index marker. Without excluding it, the upward walk
    matches `~/.codelens` for essentially any project under the home tree and
    defeats the documented "no index -> pass unconditionally" guarantee. So the
    home directory (and temp roots) are not treated as project roots — unless the
    session cwd IS that directory, which mirrors codelens-engine's
    project::root_detect::detect_root_with_bounds.
    """
    if not isinstance(cwd, str) or not cwd:
        return False
    start = _canonical(cwd)
    home = _home_dir()
    p = start
    for _ in range(GATE_MAX_LEVELS):
        # `~/.codelens` is global state; don't infer a project root from it, and
        # don't walk above home. Skipped only when cwd itself is not home.
        if p != start and home is not None and p == home:
            break
        skip_marker = p != start and _is_temp_root(p)
        if not skip_marker and os.path.isdir(os.path.join(p, ".codelens")):
            return True
        parent = os.path.dirname(p)
        if parent == p:
            break
        p = parent
    return False


def path_is_single_file(path: object, cwd: object) -> bool:
    if not isinstance(path, str) or not path:
        return False
    target = path if os.path.isabs(path) else os.path.join(
        cwd if isinstance(cwd, str) else "", path
    )
    return os.path.isfile(target)


def throttle_allows(session_id: object) -> bool:
    """True if an advisory may be emitted (and records it); False if throttled."""
    tmpdir = os.environ.get("TMPDIR") or "/tmp"
    sid = session_id if isinstance(session_id, str) and session_id else "unknown"
    sid = re.sub(r"[^A-Za-z0-9_.-]", "_", sid)
    state = os.path.join(tmpdir, f"codelens-first-{sid}")
    count = 0
    try:
        with open(state, encoding="utf-8") as fh:
            count = int(fh.read().strip() or "0")
    except (OSError, ValueError):
        count = 0
    if count >= MAX_ADVISORIES_PER_SESSION:
        return False
    try:
        with open(state, "w", encoding="utf-8") as fh:
            fh.write(str(count + 1))
    except OSError:
        pass
    return True


def emit(decision: str, *, reason: str | None = None, context: str | None = None) -> None:
    payload = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
        }
    }
    if reason is not None:
        payload["hookSpecificOutput"]["permissionDecisionReason"] = reason
    if context is not None:
        payload["hookSpecificOutput"]["additionalContext"] = context
    sys.stdout.write(json.dumps(payload))


def advisory_context(symbol: str) -> str:
    return (
        f"CodeLens index detected. For symbol lookups like {symbol}, prefer "
        f'mcp__codelens__find_symbol(name="{symbol}") or '
        "mcp__codelens__find_referencing_symbols — bounded ranked results cost "
        "fewer tokens than raw grep. Set CODELENS_FIRST_MODE=off to silence, "
        "=strict to enforce."
    )


def strict_reason(symbol: str) -> str:
    return (
        f'Use mcp__codelens__find_symbol(name="{symbol}") instead — '
        "CODELENS_FIRST_MODE=strict is set. For raw text audit, add a specific "
        "file path or set CODELENS_FIRST_MODE=advisory."
    )


def run() -> None:
    mode = (os.environ.get("CODELENS_FIRST_MODE") or "advisory").strip().lower()
    if mode == "off":
        return

    data = json.loads(sys.stdin.read())
    if not isinstance(data, dict):
        return
    if data.get("tool_name") != "Grep":
        return

    cwd = data.get("cwd")
    if not has_codelens_index(cwd):
        return

    tool_input = data.get("tool_input")
    if not isinstance(tool_input, dict):
        return

    if path_is_single_file(tool_input.get("path"), cwd):
        return

    symbol = symbol_form(tool_input.get("pattern"))
    if symbol is None:
        return

    if mode == "strict":
        emit("deny", reason=strict_reason(symbol))
        return

    # advisory (default, and any unrecognised value)
    if throttle_allows(data.get("session_id")):
        emit("allow", context=advisory_context(symbol))


def main() -> int:
    try:
        run()
    except Exception:
        # Fail-open: never break the user's Grep.
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
