#!/usr/bin/env python3
"""Auto-generate CHANGELOG entries from git tags and commit history.

Usage:
    python3 scripts/gen-changelog.py          # show Unreleased + latest tag
    python3 scripts/gen-changelog.py --all    # regenerate full CHANGELOG
    python3 scripts/gen-changelog.py --since v1.9.30  # from specific tag

Output is printed to stdout. Pipe to CHANGELOG.md or review manually.

Conventional commit prefixes recognized: feat, fix, refactor, docs, chore,
ci, test, perf, build, revert. Commits without a prefix go under "Other".
"""

import subprocess
import re
import sys
from collections import defaultdict

CATEGORY_MAP = {
    "feat": "Added",
    "fix": "Fixed",
    "refactor": "Refactor",
    "docs": "Docs",
    "chore": "Chore",
    "ci": "CI",
    "test": "Tests",
    "perf": "Performance",
    "build": "Build",
    "revert": "Reverted",
}

COMMIT_RE = re.compile(
    r"^(?P<hash>[a-f0-9]+)\s+(?:(?P<prefix>feat|fix|refactor|docs|chore|ci|test|perf|build|revert)(?:\([^)]*\))?:\s*)?(?P<message>.+)$"
)


def run(cmd: str) -> str:
    return subprocess.run(cmd, shell=True, capture_output=True, text=True).stdout.strip()


def get_tags() -> list[str]:
    """Get sorted tags (newest first)."""
    raw = run("git tag --sort=-v:refname")
    return [t for t in raw.splitlines() if t]


def get_commits_between(old_tag: str | None, new_tag: str | None) -> list[tuple[str, str, str]]:
    """Get commits between two tags. Returns list of (hash, prefix, message)."""
    ref = f"{old_tag}..{new_tag}" if old_tag else new_tag or "HEAD"
    raw = run(f"git log {ref} --pretty=format:'%H %s'")
    commits = []
    for line in raw.splitlines():
        m = COMMIT_RE.match(line.strip("'"))
        if m:
            commits.append((m.group("hash")[:7], m.group("prefix") or "other", m.group("message")))
        else:
            commits.append((line[:7], "other", line[8:]))
    return commits


def group_commits(commits: list[tuple[str, str, str]]) -> dict[str, list[str]]:
    """Group commits by category."""
    groups: dict[str, list[str]] = defaultdict(list)
    for _hash, prefix, message in commits:
        cat = CATEGORY_MAP.get(prefix, "Other")
        entry = message.strip()
        # Remove scope from message if present
        entry = re.sub(r"^\w+\([^)]*\):\s*", "", entry)
        groups[cat].append(entry)
    return dict(groups)


def format_section(tag: str, date: str, commits: list[tuple[str, str, str]]) -> str:
    """Format a CHANGELOG section for a tag."""
    groups = group_commits(commits)
    lines = [f"## [{tag}] — {date}", ""]
    # Order: Added, Fixed, Refactor, Performance, Docs, CI, Tests, Build, Chore, Other, Reverted
    order = ["Added", "Fixed", "Refactor", "Performance", "Docs", "CI", "Tests", "Build", "Chore", "Other", "Reverted"]
    for cat in order:
        if cat in groups:
            lines.append(f"### {cat}")
            lines.append("")
            for msg in groups[cat]:
                lines.append(f"- {msg}")
            lines.append("")
    return "\n".join(lines)


def tag_date(tag: str) -> str:
    """Get the date of a tag in YYYY-MM-DD format."""
    raw = run(f"git log -1 --format=%ci {tag}")
    if raw:
        return raw.split()[0]
    return "unknown"


def main():
    tags = get_tags()
    if not tags:
        print("No git tags found.", file=sys.stderr)
        sys.exit(1)

    if "--all" in sys.argv:
        # Generate full changelog
        print("# Changelog\n")
        print("All notable changes to **CodeLens MCP**.\n")
        print("The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).")
        print("This project adheres to [Semantic Versioning](https://semsemver.org/spec/v2.0.0.html).\n")
        for i in range(len(tags) - 1):
            old, new = tags[i + 1], tags[i]
            commits = get_commits_between(old, new)
            if commits:
                print(format_section(new, tag_date(new), commits))
        # First tag (oldest)
        commits = get_commits_between(None, tags[-1])
        if commits:
            print(format_section(tags[-1], tag_date(tags[-1]), commits))
    elif "--since" in sys.argv:
        idx = sys.argv.index("--since")
        since_tag = sys.argv[idx + 1] if idx + 1 < len(sys.argv) else tags[0]
        # Find the tag after since_tag
        try:
            start = tags.index(since_tag)
            relevant = tags[:start]
        except ValueError:
            relevant = tags
        if not relevant:
            print(f"Tag {since_tag} not found.", file=sys.stderr)
            sys.exit(1)
        print(f"# Changes since {since_tag}\n")
        for i in range(len(relevant) - 1):
            old, new = relevant[i + 1], relevant[i]
            commits = get_commits_between(old, new)
            if commits:
                print(format_section(new, tag_date(new), commits))
        print(format_section(relevant[-1], tag_date(relevant[-1]), get_commits_between(None, relevant[-1])))
    else:
        # Default: show Unreleased + latest tag
        latest = tags[0]
        prev = tags[1] if len(tags) > 1 else None

        unreleased = get_commits_between(latest, "HEAD")
        if unreleased:
            print("## [Unreleased]\n")
            for cat, msgs in group_commits(unreleased).items():
                print(f"### {cat}\n")
                for msg in msgs:
                    print(f"- {msg}")
                print()

        latest_commits = get_commits_between(prev, latest) if prev else get_commits_between(None, latest)
        if latest_commits:
            print(format_section(latest, tag_date(latest), latest_commits))


if __name__ == "__main__":
    main()
