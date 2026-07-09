#!/usr/bin/env python3

# --- How to run ---
#   python3 scripts/test/test-redeploy-daemons.py
# CI runs every scripts/test/test-*.py with system Python.
# ------------------

from __future__ import annotations

import contextlib
import os
import socket
import stat
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
REDEPLOY_SCRIPT = REPO_ROOT / "scripts" / "redeploy-daemons.sh"


def write_fake_executable(path: Path) -> None:
    path.write_text("#!/bin/sh\necho fake\nexit 0\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def open_listener(port: int) -> socket.socket:
    """Bind + listen on 127.0.0.1:port so `lsof -sTCP:LISTEN` sees it — stands in
    for a stale daemon already occupying an expected port."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", port))
    sock.listen(1)
    return sock


def test_redeploy_reaches_listen_wait_when_plist_missing() -> None:
    # A missing LaunchAgent plist must not abort the script at kickstart. Before
    # the fix, `launchctl kickstart` ran outside the plist-exists guard; on macOS
    # kickstart of an unloaded label exits 113 ("Could not find service ... in
    # domain") and `set -euo pipefail` killed the script before it ever reached
    # the LISTEN-wait diagnostics. The single observable signal for RED/GREEN is
    # whether the "waiting up to" LISTEN-wait log is emitted at all. Ports are not
    # expected to open here, so a non-zero final exit is fine.
    with tempfile.TemporaryDirectory(prefix="codelens-redeploy-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_home = tmp / "home"
        # Intentionally create an EMPTY LaunchAgents dir: no plist for our labels.
        (fake_home / "Library" / "LaunchAgents").mkdir(parents=True)
        source_bin = tmp / "codelens-mcp"
        target_bin = tmp / "target" / "codelens-mcp-http"
        write_fake_executable(source_bin)

        # Unique label so kickstart can never collide with a real installed daemon.
        label_prefix = f"codelens-test-fixture-{os.getpid()}"
        # High ports unlikely to be in real use; they will never LISTEN here.
        readonly_port = "18839"
        mutation_port = "18838"

        env = dict(os.environ)
        env["HOME"] = str(fake_home)

        proc = subprocess.run(
            [
                "bash",
                str(REDEPLOY_SCRIPT),
                str(REPO_ROOT),
                "--label-prefix",
                label_prefix,
                "--readonly-port",
                readonly_port,
                "--mutation-port",
                mutation_port,
                "--source",
                str(source_bin),
                "--target",
                str(target_bin),
                "--wait-secs",
                "1",
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
            env=env,
        )

        combined = proc.stdout + proc.stderr
        assert "waiting up to" in combined, (
            "redeploy aborted before the LISTEN-wait stage (kickstart exit 113 on "
            "a missing plist); expected the script to skip bootstrap+kickstart for "
            "the missing label and still reach the 'waiting up to' LISTEN-wait "
            f"diagnostics.\nreturncode={proc.returncode}\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )


def test_redeploy_fails_when_plist_missing_even_if_ports_already_listen() -> None:
    # Codex review on PR #378: a missing LaunchAgent plist (e.g. a mistyped
    # --label-prefix) must FAIL loudly, not silently exit 0. Before the fix the
    # missing-plist branch only logged a warning; the subsequent LISTEN check then
    # passed against pre-existing (stale) daemons already bound to the expected
    # ports, so redeploy reported success without bootstrapping/kickstarting
    # anything. Occupy the expected ports to simulate those stale daemons, then
    # assert a non-zero exit plus a missing-plist diagnostic naming the label.
    with tempfile.TemporaryDirectory(prefix="codelens-redeploy-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_home = tmp / "home"
        # Empty LaunchAgents dir: no plist exists for our labels.
        (fake_home / "Library" / "LaunchAgents").mkdir(parents=True)
        source_bin = tmp / "codelens-mcp"
        target_bin = tmp / "target" / "codelens-mcp-http"
        write_fake_executable(source_bin)

        label_prefix = f"codelens-test-fixture-{os.getpid()}"
        env = dict(os.environ)
        env["HOME"] = str(fake_home)

        # Ephemeral ports we hold LISTEN on for the whole subprocess run — these
        # stand in for stale daemons already occupying the expected ports.
        with contextlib.closing(open_listener(0)) as ro_sock, contextlib.closing(
            open_listener(0)
        ) as mu_sock:
            readonly_port = str(ro_sock.getsockname()[1])
            mutation_port = str(mu_sock.getsockname()[1])
            proc = subprocess.run(
                [
                    "bash",
                    str(REDEPLOY_SCRIPT),
                    str(REPO_ROOT),
                    "--label-prefix",
                    label_prefix,
                    "--readonly-port",
                    readonly_port,
                    "--mutation-port",
                    mutation_port,
                    "--source",
                    str(source_bin),
                    "--target",
                    str(target_bin),
                    "--wait-secs",
                    "5",
                ],
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
                env=env,
            )

        combined = proc.stdout + proc.stderr
        assert proc.returncode != 0, (
            "redeploy exited 0 despite a missing plist while stale daemons were "
            "listening on the expected ports — a mistyped --label-prefix would be "
            "reported as a successful redeploy.\n"
            f"returncode={proc.returncode}\nstdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "plist not found" in combined, (
            "expected a missing-plist diagnostic naming the label.\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )


def main() -> int:
    tests = [
        test_redeploy_reaches_listen_wait_when_plist_missing,
        test_redeploy_fails_when_plist_missing_even_if_ports_already_listen,
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
