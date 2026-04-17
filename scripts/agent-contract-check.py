#!/usr/bin/env python3
"""Prompt-contract check for repo-shipped agents.

Verifies that every ``mcp__codelens__X`` tool mentioned in ``agents/*.md``
matches the MCP server's registered tool surface defined in
``crates/codelens-mcp/src/tool_defs/build.rs``.

Three checks:

1. **Allowlist existence** — every tool in the agent's frontmatter
   ``tools:`` block must be registered in build.rs.
2. **Body/allowlist consistency** — every ``mcp__codelens__X`` mentioned in
   the prompt body must either be in the frontmatter allowlist or the
   ``disallowedTools`` list (so agents can document what they explicitly
   block as well).
3. **Deprecated tool usage** — if the frontmatter lists a tool marked
   ``[DEPRECATED`` in build.rs, emit a warning with the successor.

Exit codes:
- 0 → clean or warnings only (without ``--strict``)
- 1 → any error, or warnings with ``--strict``

This is a static prompt/contract audit, not a live server check.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


TOOL_NEW_RE = re.compile(r'Tool::new\("([A-Za-z_][A-Za-z0-9_]*)"')
DEPRECATED_RE = re.compile(
    r'Tool::new\("([A-Za-z_][A-Za-z0-9_]*)"\s*,\s*"(\[DEPRECATED[^"]*)"'
)
AGENT_TOOL_MENTION_RE = re.compile(r"mcp__codelens__([A-Za-z_][A-Za-z0-9_]*)")


def extract_registered_tools(build_rs: Path) -> tuple[set[str], dict[str, str]]:
    """Return (registered, deprecated_with_successor_hint)."""
    text = build_rs.read_text(encoding="utf-8")
    registered = set(TOOL_NEW_RE.findall(text))
    deprecated: dict[str, str] = {}
    for name, desc in DEPRECATED_RE.findall(text):
        successor_match = re.search(r"Use (\w+) directly", desc)
        deprecated[name] = (
            successor_match.group(1) if successor_match else "see build.rs"
        )
    return registered, deprecated


def parse_agent_frontmatter(agent_md: Path) -> tuple[set[str], set[str], str]:
    """Return (allowlist, disallowed, body)."""
    text = agent_md.read_text(encoding="utf-8")
    if not text.startswith("---"):
        return set(), set(), text

    parts = text.split("---", 2)
    if len(parts) < 3:
        return set(), set(), text

    frontmatter, body = parts[1], parts[2]

    allowlist = set(
        AGENT_TOOL_MENTION_RE.findall(_extract_block(frontmatter, "tools:"))
    )
    disallowed = set(
        AGENT_TOOL_MENTION_RE.findall(_extract_block(frontmatter, "disallowedTools:"))
    )
    return allowlist, disallowed, body


def _extract_block(frontmatter: str, key: str) -> str:
    """Extract the raw text of a YAML list-style block like ``tools: [ ... ]``."""
    idx = frontmatter.find(key)
    if idx < 0:
        return ""
    tail = frontmatter[idx + len(key) :]
    depth = 0
    started = False
    out: list[str] = []
    for ch in tail:
        if ch == "[":
            depth += 1
            started = True
            out.append(ch)
        elif ch == "]":
            depth -= 1
            out.append(ch)
            if started and depth == 0:
                break
        else:
            if started:
                out.append(ch)
    return "".join(out)


def check_agent(
    agent_md: Path,
    registered: set[str],
    deprecated: dict[str, str],
) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []
    allowlist, disallowed, body = parse_agent_frontmatter(agent_md)
    mentioned_in_body = set(AGENT_TOOL_MENTION_RE.findall(body))

    for tool in sorted(allowlist):
        if tool not in registered:
            errors.append(
                f"{agent_md}: tool 'mcp__codelens__{tool}' is in allowlist but not "
                f"registered in build.rs — did it get renamed or removed?"
            )
        if tool in deprecated:
            warnings.append(
                f"{agent_md}: tool 'mcp__codelens__{tool}' is DEPRECATED "
                f"(successor: {deprecated[tool]}). Update the allowlist."
            )

    for tool in sorted(mentioned_in_body):
        if tool in allowlist or tool in disallowed:
            continue
        if tool in registered:
            warnings.append(
                f"{agent_md}: body mentions 'mcp__codelens__{tool}' but allowlist "
                f"does not include it — call would fail at runtime."
            )
        else:
            errors.append(
                f"{agent_md}: body mentions 'mcp__codelens__{tool}' which is not "
                f"registered in build.rs."
            )
        if tool in deprecated:
            warnings.append(
                f"{agent_md}: body mentions DEPRECATED tool 'mcp__codelens__{tool}' "
                f"(successor: {deprecated[tool]})."
            )

    return errors, warnings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--project",
        default=".",
        help="Project root (default: cwd)",
    )
    parser.add_argument(
        "--agents-dir",
        default="agents",
        help="Directory containing agent prompt files (default: agents/)",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Treat warnings as errors.",
    )
    args = parser.parse_args()

    project = Path(args.project).resolve()
    agents_dir = project / args.agents_dir
    build_rs = project / "crates" / "codelens-mcp" / "src" / "tool_defs" / "build.rs"

    if not build_rs.exists():
        print(f"ERROR: build.rs not found at {build_rs}", file=sys.stderr)
        return 2
    if not agents_dir.is_dir():
        print(f"ERROR: agents dir not found at {agents_dir}", file=sys.stderr)
        return 2

    registered, deprecated = extract_registered_tools(build_rs)
    print(
        f"build.rs: {len(registered)} registered tools, "
        f"{len(deprecated)} marked DEPRECATED"
    )

    total_errors: list[str] = []
    total_warnings: list[str] = []

    agent_files = sorted(agents_dir.glob("*.md"))
    if not agent_files:
        print(f"WARNING: no agent .md files under {agents_dir}", file=sys.stderr)
        return 0

    for agent_md in agent_files:
        errors, warnings = check_agent(agent_md, registered, deprecated)
        total_errors.extend(errors)
        total_warnings.extend(warnings)
        ok_marker = (
            "OK" if not errors and not warnings else ("FAIL" if errors else "WARN")
        )
        print(f"  {ok_marker}: {agent_md.relative_to(project)}")

    for warning in total_warnings:
        print(f"WARN: {warning}", file=sys.stderr)
    for error in total_errors:
        print(f"ERROR: {error}", file=sys.stderr)

    print(
        f"\nTotal: {len(total_errors)} errors, {len(total_warnings)} warnings, "
        f"{len(agent_files)} agent files checked"
    )

    if total_errors:
        return 1
    if args.strict and total_warnings:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
