#!/usr/bin/env python3

# --- How to run ---
#   python3 scripts/test/test-install-http-daemons-markerless.py
# CI runs every scripts/test/test-*.py with system Python.
# ------------------

from __future__ import annotations

import plistlib
import stat
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SCRIPT = REPO_ROOT / "scripts" / "install-http-daemons-launchd.sh"


def print_plist(*extra: str) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-markerless-") as raw:
        tmp = Path(raw)
        repo = tmp / "repo"
        (repo / "crates" / "codelens-mcp").mkdir(parents=True)
        (repo / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (repo / "crates" / "codelens-mcp" / "Cargo.toml").write_text(
            '[package]\nname = "codelens-mcp"\nversion = "0.0.0"\nedition = "2021"\n',
            encoding="utf-8",
        )
        binary = tmp / "codelens-mcp-http"
        binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        binary.chmod(binary.stat().st_mode | stat.S_IXUSR)
        return subprocess.run(
            [
                "bash",
                str(INSTALL_SCRIPT),
                str(repo),
                "--no-build",
                "--no-semantic",
                "--bin-path",
                str(binary),
                "--launch-agents-dir",
                str(tmp / "LaunchAgents"),
                "--print-only",
                *extra,
            ],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )


def markerless_setting(proc: subprocess.CompletedProcess[str]) -> str | None:
    assert proc.returncode == 0, f"stdout={proc.stdout}\nstderr={proc.stderr}"
    start = proc.stdout.index("<?xml")
    plist = plistlib.loads(proc.stdout[start:].encode())
    return plist["EnvironmentVariables"].get("CODELENS_ALLOW_MARKERLESS_ROOT")


def test_the_shared_daemon_refuses_unmarked_roots_by_default() -> None:
    # Hosts send `x-codelens-project: ${PWD}`; a session launched in an
    # unmarked folder such as ~/Downloads must not index that whole folder.
    assert markerless_setting(print_plist()) == "0"


def test_markerless_roots_can_be_allowed_explicitly() -> None:
    assert markerless_setting(print_plist("--markerless-root", "allow")) == "1"


def test_an_unknown_markerless_mode_is_rejected() -> None:
    proc = print_plist("--markerless-root", "sometimes")
    assert proc.returncode != 0
    assert "--markerless-root must be deny or allow" in proc.stderr


def main() -> int:
    tests = [
        test_the_shared_daemon_refuses_unmarked_roots_by_default,
        test_markerless_roots_can_be_allowed_explicitly,
        test_an_unknown_markerless_mode_is_rejected,
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
