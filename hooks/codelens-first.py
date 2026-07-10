#!/usr/bin/env python3
# codelens-first PreToolUse hook — route symbol lookups from native Grep AND
# shell-invoked grep/rg (via Bash) to CodeLens.
#
# CONTRACT
#   stdin  : PreToolUse JSON — {session_id, cwd, tool_name:"Grep"|"Bash",
#            tool_input:{pattern, path, ...} | {command, ...}}
#   stdout : either nothing (no decision → tool proceeds) or one line of
#            {"hookSpecificOutput":{"hookEventName":"PreToolUse",
#             "permissionDecision":"allow"|"deny",
#             "permissionDecisionReason":..., "additionalContext":...}}
#   exit   : always 0 (JSON on stdout carries the decision; fail-open by design).
#
# MODES (env CODELENS_FIRST_MODE)
#   advisory (default) : symbol-like Grep/Bash-grep → allow + advice (throttled ≤2/session)
#   strict             : HIGH-CONFIDENCE symbol lookup (snake_case/camelCase/`::`)
#                        → deny + concrete CodeLens procedure, capped at ≤3
#                        denies/session (repeat denials use a terse one-liner to
#                        keep the injected-token cost flat). Ambiguous
#                        identifiers (plain lowercase words) downgrade to a
#                        single advisory. strict only ever denies when every
#                        safeguard passes (fail-open otherwise):
#                          · daemon health probe (down → pass; 5-min negative cache;
#                            env CODELENS_FIRST_ASSUME_ALIVE=1 skips the probe — CI/tests)
#                          · worktree cwd (post-edit index staleness) → pass
#                          · escape markers  [cl-text] (text audit) / [cl-fallback]
#                            (CodeLens failed or returned nothing) → pass
#                          · text-audit flags -i / -F / -v, incl. combined
#                            short clusters like -rni / -in → pass
#                          · doc/log/artifact targets (md/json/log/docs/dist…) → pass
#                          · absolute targets outside the project root → pass
#   off                : do nothing
#
# GATE
#   Only acts when a PROJECT-local .codelens/ dir exists at/above cwd (≤5 levels).
#   The global ~/.codelens data dir (and a .codelens at a temp root) is excluded —
#   its basename collides with the project marker but it is not a project index.
#   No index → the project does not use CodeLens → pass unconditionally (strict
#   included). A single-file target (Grep tool_input.path, or the sole positional
#   path in a Bash grep/rg segment) is a targeted text audit → pass. Non-symbol /
#   regex-metachar / multi-alternative (`a\|b`) patterns → pass (those are exactly
#   the recall/text-audit shape grep stays better at). Any exception → pass
#   (fail-open).
#
#   Bash detection is pipeline-aware: the command is split into pipelines on
#   `;`/`&&`/`||`/newline, and only the FIRST command of each pipeline is
#   classified — `ps aux | grep node` is output filtering, not a code search,
#   and never fires. Within a grep/rg head, tokens are walked with shlex.
#   In `strict` mode a matching segment denies the *whole* Bash call — coarser
#   than the single-tool-call Grep path, an accepted tradeoff given the escape
#   markers and the per-session deny cap.
#
# METRICS: when ~/.claude/metrics/ already exists (never created by this hook),
#   every decision appends one JSONL line to codelens-first.jsonl there, so
#   redirect→conversion rates and realized token savings stay measurable.
#
# Output carries no timestamps or absolute paths (prompt-cache hygiene).
# Stdlib only, no external dependencies.

from __future__ import annotations

import json
import os
import re
import shlex
import sys
import time

SYMBOL_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*$")
MAX_ADVISORIES_PER_SESSION = 2
MAX_ADVISORIES_STRICT = 1
MAX_DENIES_PER_SESSION = 3
GATE_MAX_LEVELS = 5
CARD_URL = os.environ.get(
    "CODELENS_CARD_URL", "http://127.0.0.1:7839/.well-known/mcp.json"
)
HEALTH_NEG_CACHE_SECS = 300
METRIC_DIR = os.path.expanduser("~/.claude/metrics")
METRIC_FILE = os.path.join(METRIC_DIR, "codelens-first.jsonl")
# A sibling PreToolUse hook's metrics file grows append-forever and reached
# 6.6MB — bound this one the same way an unattended global default would.
METRIC_MAX_BYTES = int(os.environ.get("CODELENS_FIRST_METRIC_MAX_BYTES", 5 * 1024 * 1024))

# Doc/log/artifact targets are text audits → pass.
TEXT_TARGET = re.compile(
    r"\.(md|json|log|txt|ya?ml|lock|csv|html?)$"
    r"|(^|/)(logs|docs|node_modules|dist|build|out|coverage|target)(/|$)"
)


def _rotate_metric_if_large() -> None:
    """Once the file crosses the byte cap, keep only as much of the tail as
    fits in half the cap — a line-count-based keep (e.g. "last 2000 lines")
    doesn't actually guarantee landing under a BYTE cap when lines are large
    or numerous, so this walks from the end summing encoded byte length
    directly. Leaving headroom (half, not the full cap) avoids rotating
    again on the very next call."""
    try:
        if os.path.getsize(METRIC_FILE) <= METRIC_MAX_BYTES:
            return
        with open(METRIC_FILE, encoding="utf-8") as fh:
            lines = fh.readlines()
        keep_budget = METRIC_MAX_BYTES // 2
        kept: list[str] = []
        size = 0
        for line in reversed(lines):
            size += len(line.encode("utf-8"))
            if size > keep_budget:
                break
            kept.append(line)
        kept.reverse()
        with open(METRIC_FILE, "w", encoding="utf-8") as fh:
            fh.writelines(kept)
    except OSError:
        pass


def metric(rec: dict) -> None:
    """Append a decision record — only when the metrics dir already exists."""
    try:
        if not os.path.isdir(METRIC_DIR):
            return
        if os.path.isfile(METRIC_FILE):
            _rotate_metric_if_large()
        rec["ts"] = int(time.time())
        with open(METRIC_FILE, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(rec, ensure_ascii=False) + "\n")
    except Exception:
        pass


def symbol_form(pattern: object) -> str | None:
    """Return the clean symbol if the Grep pattern is a symbol lookup, else None."""
    if not isinstance(pattern, str):
        return None
    s = pattern
    if s.startswith("\\b"):
        s = s[2:]
    if s.endswith("\\b"):
        s = s[:-2]
    s = s.strip("^$")
    s = re.sub(r"\\?\($", "", s)  # function-call search `foo\(` → foo
    if len(s) < 3:
        return None
    # SYMBOL_RE only admits word chars + `::`, so any regex metachar
    # (.*+?[](){}| etc.) that survived the \b strip fails the match.
    return s if SYMBOL_RE.match(s) else None


def high_confidence_symbol(symbol: str) -> bool:
    """snake_case / camelCase / PascalCase / `::` shapes are confidently code
    symbols. Plain lowercase words (error, session, node …) may be natural
    language, so strict never denies them — they downgrade to advisory."""
    return bool(
        "_" in symbol
        or re.search(r"[a-z][A-Z]", symbol)
        or re.search(r"[A-Z][a-z].*[A-Z]", symbol)
        or "::" in symbol
    )


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


def codelens_project_root(cwd: object) -> str | None:
    """Return the project root whose `.codelens/` marker covers cwd, else None.

    The global CodeLens data dir lives at `~/.codelens`, whose basename collides
    with the project-local index marker. Without excluding it, the upward walk
    matches `~/.codelens` for essentially any project under the home tree and
    defeats the documented "no index -> pass unconditionally" guarantee. So the
    home directory (and temp roots) are not treated as project roots — unless the
    session cwd IS that directory, which mirrors codelens-engine's
    project::root_detect::detect_root_with_bounds.
    """
    if not isinstance(cwd, str) or not cwd:
        return None
    start = _canonical(cwd)
    home = _home_dir()
    p = start
    for _ in range(GATE_MAX_LEVELS):
        if p != start and home is not None and p == home:
            break
        skip_marker = p != start and _is_temp_root(p)
        if not skip_marker and os.path.isdir(os.path.join(p, ".codelens")):
            return p
        parent = os.path.dirname(p)
        if parent == p:
            break
        p = parent
    return None


def path_is_single_file(path: object, cwd: object) -> bool:
    if not isinstance(path, str) or not path:
        return False
    target = path if os.path.isabs(path) else os.path.join(
        cwd if isinstance(cwd, str) else "", path
    )
    return os.path.isfile(target)


PIPELINE_SPLIT = re.compile(r"[\n;]|&&|\|\|")
BASH_GREP_HEAD_RE = re.compile(r"^\s*(?:command\s+)?(?:rg|grep|egrep)\b")
# Flags that consume the following token as a value (not a pattern/path).
_GREP_VALUE_FLAGS = frozenset(
    {
        "-e", "--regexp", "-A", "-B", "-C", "-m", "--max-count",
        "--include", "--exclude", "--exclude-dir", "-g", "--glob",
        "-f", "--file", "-t", "--type", "--type-add", "-M",
        "--max-columns", "-j", "--threads",
    }
)
# Text-audit signals: case folding / fixed strings / inverted match.
_TEXT_AUDIT_FLAGS = frozenset({"-i", "--ignore-case", "-F", "--fixed-strings", "-v"})
# Same signals, as single-char short-option letters (for combined clusters like
# `-rni`/`-in` — shlex does not split these into `-r -n -i`, so an exact-token
# check against _TEXT_AUDIT_FLAGS alone misses them).
_TEXT_AUDIT_SHORT_CHARS = frozenset({"i", "F", "v"})


def _short_cluster_has_text_audit_flag(tok: str) -> bool:
    """True if a combined short-option cluster (`-rni`, not `--...`) contains
    an -i/-F/-v letter. Stops scanning at the first digit, since a trailing
    digit run is a value glued to a count flag (`-A3`, `-m5`), not more letters."""
    if not tok.startswith("-") or tok.startswith("--"):
        return False
    for ch in tok[1:]:
        if ch.isdigit():
            break
        if ch in _TEXT_AUDIT_SHORT_CHARS:
            return True
    return False


def _grep_segment_pattern(seg: str) -> tuple[str | None, list[str], bool]:
    """Return (pattern, positional_paths, text_audit_flag) for a grep/rg-headed
    shell segment.

    Best-effort argv walk over shlex tokens: first non-flag positional is the
    pattern (or the value of -e/--regexp if given), remaining positionals are
    treated as path targets. Returns (None, [], False) if the segment doesn't
    parse or doesn't start with grep/rg.
    """
    try:
        tokens = shlex.split(seg)
    except ValueError:
        return None, [], False
    idx = 1 if tokens[:1] == ["command"] else 0
    if idx >= len(tokens) or tokens[idx] not in ("grep", "rg", "egrep"):
        return None, [], False
    idx += 1
    pattern: str | None = None
    positionals: list[str] = []
    text_audit = False
    while idx < len(tokens):
        tok = tokens[idx]
        if tok.startswith("-") and tok != "-":
            if tok in _TEXT_AUDIT_FLAGS or _short_cluster_has_text_audit_flag(tok):
                text_audit = True
            base = tok.split("=", 1)[0]
            if base in _GREP_VALUE_FLAGS:
                if "=" in tok:
                    if base in ("-e", "--regexp") and pattern is None:
                        pattern = tok.split("=", 1)[1]
                elif idx + 1 < len(tokens):
                    if base in ("-e", "--regexp") and pattern is None:
                        pattern = tokens[idx + 1]
                    idx += 2
                    continue
            idx += 1
            continue
        if pattern is None:
            pattern = tok
        else:
            positionals.append(tok)
        idx += 1
    return pattern, positionals, text_audit


def bash_symbol_candidate(command: object, cwd: object, root: str) -> str | None:
    """Scan a Bash `command` string for a pipeline-head grep/rg that is a bare
    single-identifier, multi-file/directory code search inside the project.
    Returns the symbol name, or None if no pipeline qualifies (fail-open —
    includes parse failures, pipe-filtering greps, text-audit flags/targets,
    out-of-project absolute targets, and single-file-target segments)."""
    if not isinstance(command, str) or not command:
        return None
    for pipeline in PIPELINE_SPLIT.split(command):
        head = pipeline.split("|", 1)[0].strip()
        if not head or not BASH_GREP_HEAD_RE.match(head):
            continue
        pattern, positionals, text_audit = _grep_segment_pattern(head)
        if text_audit:
            continue
        symbol = symbol_form(pattern)
        if symbol is None:
            continue
        if len(positionals) == 1 and path_is_single_file(positionals[0], cwd):
            continue
        if positionals and all(TEXT_TARGET.search(p) for p in positionals):
            continue
        if positionals and all(
            os.path.isabs(p)
            and _canonical(p) != root
            and not _canonical(p).startswith(root + os.sep)
            for p in positionals
        ):
            continue  # only absolute targets outside the project → not CodeLens turf
        return symbol
    return None


def _counter(state_name: str, session_id: object, limit: int) -> bool:
    """True if under limit (and increments); False if throttled."""
    tmpdir = os.environ.get("TMPDIR") or "/tmp"
    sid = session_id if isinstance(session_id, str) and session_id else "unknown"
    sid = re.sub(r"[^A-Za-z0-9_.-]", "_", sid)
    state = os.path.join(tmpdir, f"{state_name}-{sid}")
    count = 0
    try:
        with open(state, encoding="utf-8") as fh:
            count = int(fh.read().strip() or "0")
    except (OSError, ValueError):
        count = 0
    if count >= limit:
        return False
    try:
        with open(state, "w", encoding="utf-8") as fh:
            fh.write(str(count + 1))
    except OSError:
        pass
    return True


def throttle_allows(session_id: object, mode: str) -> bool:
    limit = MAX_ADVISORIES_STRICT if mode == "strict" else MAX_ADVISORIES_PER_SESSION
    return _counter("codelens-first", session_id, limit)


def deny_allows(session_id: object) -> bool:
    return _counter("codelens-first-deny", session_id, MAX_DENIES_PER_SESSION)


def _deny_count(session_id: object) -> int:
    tmpdir = os.environ.get("TMPDIR") or "/tmp"
    sid = session_id if isinstance(session_id, str) and session_id else "unknown"
    sid = re.sub(r"[^A-Za-z0-9_.-]", "_", sid)
    try:
        with open(os.path.join(tmpdir, f"codelens-first-deny-{sid}"), encoding="utf-8") as fh:
            return int(fh.read().strip() or "1")
    except (OSError, ValueError):
        return 1


def daemon_alive() -> bool:
    """CodeLens HTTP daemon liveness. Down → 5-min negative cache (avoids paying
    the probe timeout on every symbol grep). CODELENS_FIRST_ASSUME_ALIVE=1 skips
    the probe entirely — for CI/tests and stdio-only setups."""
    assume = (os.environ.get("CODELENS_FIRST_ASSUME_ALIVE") or "").strip().lower()
    if assume in ("1", "true", "yes"):
        return True
    tmpdir = os.environ.get("TMPDIR") or "/tmp"
    neg = os.path.join(tmpdir, "codelens-first-daemon-down")
    try:
        if time.time() - os.path.getmtime(neg) < HEALTH_NEG_CACHE_SECS:
            return False
    except OSError:
        pass
    try:
        import urllib.request

        with urllib.request.urlopen(CARD_URL, timeout=0.5) as resp:
            if resp.status == 200:
                try:
                    os.unlink(neg)
                except OSError:
                    pass
                return True
    except Exception:
        pass
    try:
        with open(neg, "w", encoding="utf-8") as fh:
            fh.write("1")
    except OSError:
        pass
    return False


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
    sys.stdout.write(json.dumps(payload, ensure_ascii=False))


def advisory_context(symbol: str) -> str:
    return (
        f"CodeLens index detected. For symbol lookups like {symbol}, prefer "
        f'mcp__codelens__search(mode="symbol", name="{symbol}") or '
        'mcp__codelens__search(mode="refs", symbol_name="…") — ranked '
        "results cost fewer tokens than raw grep. Set CODELENS_FIRST_MODE=off "
        "to silence, =strict to enforce."
    )


def strict_reason(symbol: str, root: str, deny_no: int) -> str:
    if deny_no >= 2:
        # Terse repeat: the model already saw the full procedure this session.
        return (
            f"[codelens-first strict {deny_no}/{MAX_DENIES_PER_SESSION}] "
            f"'{symbol}' → mcp__codelens__search(mode=\"symbol\", name=\"{symbol}\") / "
            "search(mode=\"refs\", symbol_name=…). Escapes: `# [cl-text]` · `# [cl-fallback]`."
        )
    return (
        f"[codelens-first strict {deny_no}/{MAX_DENIES_PER_SESSION}] "
        f"'{symbol}' is a symbol lookup — one CodeLens call replaces the "
        "grep→Read chain. Steps: ① if mcp__codelens__search is not in your tool "
        "list: ToolSearch \"select:mcp__codelens__search,mcp__codelens__graph\" "
        f"② if CodeLens is not bound yet this session: prepare_harness_session(project=\"{root}\") "
        f"③ mcp__codelens__search(mode=\"symbol\", name=\"{symbol}\", include_body=true); "
        "refs via search(mode=\"refs\", symbol_name=…); callers/impact via "
        "graph(mode=\"callers\"|\"impact\"), else "
        f"search(mode=\"ranked\", query=\"{symbol}\"). Escapes: append "
        "`# [cl-text]` for a plain text audit, `# [cl-fallback]` if CodeLens "
        "failed or returned nothing."
    )


def run() -> None:
    mode = (os.environ.get("CODELENS_FIRST_MODE") or "advisory").strip().lower()
    if mode == "off":
        return

    data = json.loads(sys.stdin.read())
    if not isinstance(data, dict):
        return

    tool_name = data.get("tool_name")
    if tool_name not in ("Grep", "Bash"):
        return

    cwd = data.get("cwd")
    root = codelens_project_root(cwd)
    if root is None:
        return

    tool_input = data.get("tool_input")
    if not isinstance(tool_input, dict):
        return

    session = data.get("session_id")
    # Short session prefix in metric records — lets the offline reporter join
    # decisions to the session transcript for redirect→conversion measurement.
    sid = (session if isinstance(session, str) else "")[:8]

    if tool_name == "Grep":
        if path_is_single_file(tool_input.get("path"), cwd):
            return
        gpath = tool_input.get("path")
        if isinstance(gpath, str) and TEXT_TARGET.search(gpath):
            return
        symbol = symbol_form(tool_input.get("pattern"))
    else:
        command = tool_input.get("command")
        if isinstance(command, str) and (
            "[cl-text]" in command or "[cl-fallback]" in command
        ):
            metric({"s": sid, "d": "pass", "why": "marker", "tool": tool_name})
            return
        symbol = bash_symbol_candidate(command, cwd, root)

    if symbol is None:
        return

    if isinstance(cwd, str) and "/worktrees/" in cwd:
        metric({"s": sid, "d": "pass", "why": "worktree", "sym": symbol})
        return

    if mode == "strict" and high_confidence_symbol(symbol):
        if not daemon_alive():
            metric({"s": sid, "d": "pass", "why": "daemon_down", "sym": symbol})
            return
        if not deny_allows(session):
            metric({"s": sid, "d": "pass", "why": "deny_capped", "sym": symbol})
            return
        deny_no = _deny_count(session)
        metric({"s": sid, "d": "deny", "sym": symbol, "n": deny_no, "tool": tool_name})
        emit("deny", reason=strict_reason(symbol, root, deny_no))
        return

    # advisory (default, ambiguous-symbol strict downgrade, unrecognised values)
    if throttle_allows(session, mode):
        metric({"s": sid, "d": "advise", "sym": symbol, "tool": tool_name})
        emit("allow", context=advisory_context(symbol))
    else:
        metric({"s": sid, "d": "pass", "why": "advise_capped", "sym": symbol})


def main() -> int:
    try:
        run()
    except Exception:
        # Fail-open: never break the user's Grep.
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
