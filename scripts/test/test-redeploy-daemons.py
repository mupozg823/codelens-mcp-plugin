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
    sock.listen(16)
    return sock


def write_launchctl_shim(shim_dir: Path, log_path: Path) -> None:
    """A PATH-injected fake `launchctl` that records each invocation and never
    touches real launchd. `print` exits non-zero so the redeploy label-gone wait
    breaks immediately; everything else exits 0."""
    shim = shim_dir / "launchctl"
    shim.write_text(
        "#!/bin/sh\n"
        'printf "%s\\n" "$*" >> "$LAUNCHCTL_SHIM_LOG"\n'
        'case "$1" in\n'
        "  print) exit 1 ;;\n"
        "  *) exit 0 ;;\n"
        "esac\n",
        encoding="utf-8",
    )
    shim.chmod(shim.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


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


def test_redeploy_waits_for_port_release_before_bootstrap() -> None:
    # 2026-07-10 incident: bootout is async at the socket layer, so the launchd
    # label can disappear while the old daemon still holds its listening socket.
    # Bootstrapping then spawns a new instance that meets the busy port and
    # yields with exit(0) (transport_http.rs:330); KeepAlive SuccessfulExit=false
    # (PR #378) never respawns it -> silent permanent-down. The fix must block
    # after bootout until the port is released and, on timeout, abort BEFORE
    # bootstrap with a diagnostic. launchctl is shimmed so no real launchd is
    # touched; the occupied port is a real ephemeral listener held all run.
    with tempfile.TemporaryDirectory(prefix="codelens-redeploy-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_home = tmp / "home"
        agents = fake_home / "Library" / "LaunchAgents"
        agents.mkdir(parents=True)
        label_prefix = f"codelens-test-fixture-{os.getpid()}"
        # A plist must exist so redeploy enters the bootout->bootstrap path.
        (agents / f"{label_prefix}-readonly.plist").write_text(
            "<plist/>\n", encoding="utf-8"
        )

        source_bin = tmp / "codelens-mcp"
        target_bin = tmp / "target" / "codelens-mcp-http"
        write_fake_executable(source_bin)

        shim_dir = tmp / "shim"
        shim_dir.mkdir()
        shim_log = tmp / "launchctl.log"
        write_launchctl_shim(shim_dir, shim_log)

        env = dict(os.environ)
        env["HOME"] = str(fake_home)
        env["PATH"] = f"{shim_dir}:{env.get('PATH', '')}"
        env["LAUNCHCTL_SHIM_LOG"] = str(shim_log)
        env["CODELENS_PORT_RELEASE_SECS"] = "1"

        with contextlib.closing(open_listener(0)) as ro_sock:
            readonly_port = str(ro_sock.getsockname()[1])
            proc = subprocess.run(
                [
                    "bash",
                    str(REDEPLOY_SCRIPT),
                    str(REPO_ROOT),
                    "--label-prefix",
                    label_prefix,
                    "--skip-mutation",
                    "--readonly-port",
                    readonly_port,
                    "--mutation-port",
                    "18838",
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
        shim_calls = shim_log.read_text(encoding="utf-8") if shim_log.exists() else ""
        assert proc.returncode != 0, (
            "redeploy should abort when the port is never released, not proceed.\n"
            f"returncode={proc.returncode}\nstdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "still occupied" in combined, (
            "expected a port-release timeout diagnostic before bootstrap.\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "bootout" in shim_calls, (
            "expected the old instance to be booted out first.\n"
            f"shim calls:\n{shim_calls}"
        )
        assert "bootstrap" not in shim_calls, (
            "redeploy bootstrapped despite the port still being occupied — the "
            "yield-exit(0) permanent-down is still reachable.\n"
            f"shim calls:\n{shim_calls}"
        )


def test_redeploy_bootstraps_after_port_released() -> None:
    # When the port is free after bootout, the barrier must confirm the release
    # and proceed to bootstrap (never block forever). The "ensuring port ... free
    # before bootstrapping" log is the RED/GREEN signal: the unfixed script has
    # no such barrier at all.
    with tempfile.TemporaryDirectory(prefix="codelens-redeploy-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_home = tmp / "home"
        agents = fake_home / "Library" / "LaunchAgents"
        agents.mkdir(parents=True)
        label_prefix = f"codelens-test-fixture-{os.getpid()}"
        (agents / f"{label_prefix}-readonly.plist").write_text(
            "<plist/>\n", encoding="utf-8"
        )

        source_bin = tmp / "codelens-mcp"
        target_bin = tmp / "target" / "codelens-mcp-http"
        write_fake_executable(source_bin)

        shim_dir = tmp / "shim"
        shim_dir.mkdir()
        shim_log = tmp / "launchctl.log"
        write_launchctl_shim(shim_dir, shim_log)

        # A free port: bind to reserve an ephemeral number, then release it.
        with contextlib.closing(open_listener(0)) as probe:
            free_port = str(probe.getsockname()[1])

        env = dict(os.environ)
        env["HOME"] = str(fake_home)
        env["PATH"] = f"{shim_dir}:{env.get('PATH', '')}"
        env["LAUNCHCTL_SHIM_LOG"] = str(shim_log)
        env["CODELENS_PORT_RELEASE_SECS"] = "3"

        proc = subprocess.run(
            [
                "bash",
                str(REDEPLOY_SCRIPT),
                str(REPO_ROOT),
                "--label-prefix",
                label_prefix,
                "--skip-mutation",
                "--readonly-port",
                free_port,
                "--mutation-port",
                "18838",
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
        shim_calls = shim_log.read_text(encoding="utf-8") if shim_log.exists() else ""
        assert "ensuring port" in combined.lower(), (
            "redeploy did not run the port-release barrier before bootstrap.\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "still occupied" not in combined, (
            "port-release barrier timed out even though the port was free.\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "bootstrap" in shim_calls, (
            "redeploy did not proceed to bootstrap after the port was released.\n"
            f"shim calls:\n{shim_calls}"
        )


def main() -> int:
    tests = [
        test_redeploy_reaches_listen_wait_when_plist_missing,
        test_redeploy_fails_when_plist_missing_even_if_ports_already_listen,
        test_redeploy_waits_for_port_release_before_bootstrap,
        test_redeploy_bootstraps_after_port_released,
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
