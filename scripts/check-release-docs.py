#!/usr/bin/env python3
"""Check release-note and support-policy freshness for the workspace version."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify release documentation for the current workspace version"
    )
    parser.add_argument(
        "--root",
        default=".",
        help="Repository root (default: current directory)",
    )
    parser.add_argument(
        "--version",
        help="Override workspace version instead of reading Cargo.toml",
    )
    return parser.parse_args()


def read_workspace_version(cargo_toml: Path) -> str:
    text = cargo_toml.read_text(encoding="utf-8")
    in_workspace_package = False
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if line.startswith("["):
            in_workspace_package = line == "[workspace.package]"
            continue
        if in_workspace_package:
            match = re.match(r'version\s*=\s*"([^"]+)"', line)
            if match:
                return match.group(1)
    raise SystemExit(f"could not find workspace.package version in {cargo_toml}")


def require(condition: bool, message: str, failures: list[str]) -> None:
    if condition:
        print(f"[ok] {message}")
    else:
        print(f"[fail] {message}", file=sys.stderr)
        failures.append(message)


def main() -> int:
    args = parse_args()
    root = Path(args.root).resolve()
    cargo_toml = root / "Cargo.toml"
    version = args.version or read_workspace_version(cargo_toml)

    release_notes = root / "docs" / "release-notes" / f"v{version}.md"
    changelog = root / "CHANGELOG.md"
    support_policy = root / "docs" / "support-policy.md"

    failures: list[str] = []

    require(cargo_toml.is_file(), "workspace Cargo.toml exists", failures)
    require(release_notes.is_file(), f"release note exists for v{version}", failures)
    require(changelog.is_file(), "CHANGELOG.md exists", failures)
    require(support_policy.is_file(), "support policy exists", failures)

    if changelog.is_file():
        changelog_text = changelog.read_text(encoding="utf-8")
        require(
            f"docs/release-notes/v{version}.md" in changelog_text
            or f"## [{version}]" in changelog_text,
            f"CHANGELOG references v{version}",
            failures,
        )

    if support_policy.is_file():
        support_text = support_policy.read_text(encoding="utf-8")
        require(
            "Active support" in support_text and "Maintenance support" in support_text,
            "support policy defines support windows",
            failures,
        )
        require(
            "PATCH" in support_text and "MINOR" in support_text and "MAJOR" in support_text,
            "support policy defines semver behavior",
            failures,
        )

    if failures:
        print(
            f"release documentation check failed for v{version} with {len(failures)} issue(s)",
            file=sys.stderr,
        )
        return 1

    print(f"release documentation check passed for v{version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
