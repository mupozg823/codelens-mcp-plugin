#!/usr/bin/env python3

# --- How to run ---
#   python3 scripts/test/test-codesign-daemon.py
# CI runs every scripts/test/test-*.py with system Python.
# ------------------

from __future__ import annotations

import os
import stat
import subprocess
import tempfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SIGN_LIB = REPO_ROOT / "scripts" / "lib" / "codesign-daemon.sh"
IDENTITY = "CodeLens Test Identity"
KEYCHAIN = "/Users/test/Library/Keychains/codelens-test.keychain-db"


def write_shim(path: Path, body: str) -> None:
    path.write_text("#!/bin/sh\n" + body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def write_security_shim(shim_dir: Path) -> None:
    """A fake `security` that knows one identity, stored in KEYCHAIN."""
    write_shim(
        shim_dir / "security",
        'case "$1" in\n'
        f'  find-identity) echo \'  1) 0123456789ABCDEF0123456789ABCDEF01234567 "{IDENTITY}"\' ;;\n'
        f"  find-certificate) echo 'keychain: \"{KEYCHAIN}\"' ;;\n"
        "esac\n"
        "exit 0\n",
    )


def sign(shim_dir: Path, binary: Path, timeout_secs: int) -> tuple[int, str, float]:
    env = {
        **os.environ,
        "PATH": f"{shim_dir}:{os.environ['PATH']}",
        "CODELENS_CODESIGN_IDENTITY": IDENTITY,
        "CODELENS_CODESIGN_TIMEOUT_SECS": str(timeout_secs),
    }
    started = time.monotonic()
    try:
        result = subprocess.run(
            ["bash", "-c", f'source "{SIGN_LIB}" && codelens_sign_daemon "{binary}"'],
            capture_output=True,
            text=True,
            env=env,
            timeout=20,
        )
    except subprocess.TimeoutExpired:
        raise AssertionError("codelens_sign_daemon was still waiting on codesign after 20s")
    return result.returncode, result.stdout + result.stderr, time.monotonic() - started


def test_a_codesign_stuck_on_a_locked_keychain_fails_and_names_the_keychain() -> None:
    # A locked keychain makes codesign wait on a GUI password prompt that a
    # non-interactive redeploy can never answer. The fake codesign stands in
    # for that prompt by never returning.
    with tempfile.TemporaryDirectory() as tmp:
        shim_dir = Path(tmp)
        write_security_shim(shim_dir)
        write_shim(shim_dir / "codesign", "exec sleep 60\n")
        binary = shim_dir / "codelens-mcp-http"
        binary.write_bytes(b"")

        status, output, elapsed = sign(shim_dir, binary, timeout_secs=1)

        assert status != 0, f"a stuck codesign must fail the signing step\n{output}"
        assert elapsed < 10, f"the timeout did not bound the wait ({elapsed:.1f}s)"
        assert f"security unlock-keychain {KEYCHAIN}" in output, (
            f"the timeout must name the keychain to unlock\n{output}"
        )


def test_a_fast_codesign_failure_is_not_reported_as_a_locked_keychain() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        shim_dir = Path(tmp)
        write_security_shim(shim_dir)
        write_shim(shim_dir / "codesign", "exit 1\n")
        binary = shim_dir / "codelens-mcp-http"
        binary.write_bytes(b"")

        status, output, _ = sign(shim_dir, binary, timeout_secs=5)

        assert status != 0, f"a failing codesign must fail the signing step\n{output}"
        assert "failed" in output, output
        assert "unlock-keychain" not in output, (
            f"an ordinary failure must not blame the keychain\n{output}"
        )


def test_a_signing_identity_that_works_signs_and_verifies() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        shim_dir = Path(tmp)
        write_security_shim(shim_dir)
        log = shim_dir / "codesign.log"
        write_shim(shim_dir / "codesign", f'printf "%s\\n" "$*" >> "{log}"\nexit 0\n')
        binary = shim_dir / "codelens-mcp-http"
        binary.write_bytes(b"")

        status, output, _ = sign(shim_dir, binary, timeout_secs=5)

        assert status == 0, f"a working identity must sign\n{output}"
        calls = log.read_text(encoding="utf-8")
        assert f"--sign {IDENTITY} --identifier dev.codelens.mcp-http {binary}" in calls, calls
        assert f"--verify --strict {binary}" in calls, calls


def main() -> int:
    tests = [
        test_a_codesign_stuck_on_a_locked_keychain_fails_and_names_the_keychain,
        test_a_fast_codesign_failure_is_not_reported_as_a_locked_keychain,
        test_a_signing_identity_that_works_signs_and_verifies,
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
