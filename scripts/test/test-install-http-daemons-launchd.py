#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-install-http-daemons-launchd.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-install-http-daemons-launchd.py
# ------------------

from __future__ import annotations

import contextlib
import json
import os
import socket
import stat
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SCRIPT = REPO_ROOT / "scripts" / "install-http-daemons-launchd.sh"
CRATE_README = REPO_ROOT / "crates" / "codelens-mcp" / "README.md"


def write_fake_executable(path: Path) -> None:
    path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def write_launchctl_shim(shim_dir: Path, log_path: Path) -> None:
    """A PATH-injected fake `launchctl` that records each invocation and exits 0,
    so the --load path can be exercised without touching real launchd."""
    shim = shim_dir / "launchctl"
    shim.write_text(
        "#!/bin/sh\n"
        'printf "%s\\n" "$*" >> "$LAUNCHCTL_SHIM_LOG"\n'
        "exit 0\n",
        encoding="utf-8",
    )
    shim.chmod(shim.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def open_listener(port: int) -> socket.socket:
    """Bind + listen on 127.0.0.1:port; stands in for a stale daemon still
    occupying an expected port after bootout."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", port))
    sock.listen(16)
    return sock


def run_installer_with_env(
    args: list[str], env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(INSTALL_SCRIPT), *args],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
        env=env,
    )


def write_fake_repo(path: Path) -> None:
    path.joinpath("crates/codelens-mcp").mkdir(parents=True)
    path.joinpath("Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
    path.joinpath("crates/codelens-mcp/Cargo.toml").write_text(
        "[package]\nname = \"codelens-mcp\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        encoding="utf-8",
    )


def run_installer(args: list[str]) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            ["bash", str(INSTALL_SCRIPT), *args],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise AssertionError(
            "installer timed out while rendering launchd plists; "
            "a shell heredoc writer is likely waiting on stdin"
        ) from exc


def test_shared_http_docs_use_one_canonical_writer_endpoint() -> None:
    text = CRATE_README.read_text(encoding="utf-8")
    assert "--port 7838" in text
    assert "--port 7837" not in text


def test_print_only_launchd_installer_completes_without_stdin_hang() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-installer-") as raw_tmp:
        tmp = Path(raw_tmp)
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()

        proc = run_installer(
            [
                str(REPO_ROOT),
                "--no-build",
                "--print-only",
                "--bin-path",
                str(bin_path),
                "--model-dir",
                str(model_dir),
                "--launch-agents-dir",
                str(agents_dir),
            ]
        )

        assert proc.returncode == 0, (
            "installer should render plists without touching launchd: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        assert "dev.codelens.mcp-mutation" in proc.stdout
        assert "dev.codelens.mcp-readonly" not in proc.stdout
        assert "<string>builder</string>" in proc.stdout
        assert "<string>mutation-enabled</string>" in proc.stdout
        assert "CODELENS_MODEL_DIR" in proc.stdout
        assert not list(agents_dir.glob("*.plist"))


def test_write_launchd_installer_updates_config_without_stdin_hang() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-installer-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_repo = tmp / "repo"
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        write_fake_repo(fake_repo)
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()

        proc = run_installer(
            [
                str(fake_repo),
                "--no-build",
                "--bin-path",
                str(bin_path),
                "--model-dir",
                str(model_dir),
                "--launch-agents-dir",
                str(agents_dir),
            ]
        )

        assert proc.returncode == 0, (
            "installer should write plists and repo-local attach config: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        assert agents_dir.joinpath("dev.codelens.mcp-mutation.plist").is_file()
        assert not agents_dir.joinpath("dev.codelens.mcp-readonly.plist").exists()
        assert fake_repo.joinpath(".codelens/config.json").is_file()
        assert "Updated host attach overrides" in proc.stdout

        config = json.loads(
            fake_repo.joinpath(".codelens/config.json").read_text(encoding="utf-8")
        )
        assert config["host_attach"]["per_host_urls"] == {
            "claude-code": "http://127.0.0.1:7838/mcp",
            "codex": "http://127.0.0.1:7838/mcp",
            "cursor": "http://127.0.0.1:7838/mcp",
        }


def _stripped_lines(text: str) -> list[str]:
    return [line.strip() for line in text.splitlines()]


def _assert_key_followed_by(lines: list[str], key: str, expected_value: str) -> None:
    key_tag = f"<key>{key}</key>"
    indices = [index for index, line in enumerate(lines) if line == key_tag]
    assert indices, f"expected {key_tag} in rendered plist output"
    for index in indices:
        assert index + 1 < len(lines), (
            f"{key_tag} is the final line; no value element follows"
        )
        actual = lines[index + 1]
        assert actual == expected_value, (
            f"{key_tag} should be followed by {expected_value!r}, got {actual!r}"
        )


def test_launchd_plist_keepalive_prevents_successful_exit_respawn_loop() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-installer-") as raw_tmp:
        tmp = Path(raw_tmp)
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()

        proc = run_installer(
            [
                str(REPO_ROOT),
                "--no-build",
                "--print-only",
                "--bin-path",
                str(bin_path),
                "--model-dir",
                str(model_dir),
                "--launch-agents-dir",
                str(agents_dir),
            ]
        )

        assert proc.returncode == 0, (
            "installer should render plists without touching launchd: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )

        lines = _stripped_lines(proc.stdout)
        # SuccessfulExit=false: a deliberate exit(0) (transport_http.rs:330 yields
        # the port to an existing instance) must NOT trigger a launchd respawn,
        # otherwise the yielding instance loops restart -> re-yield forever.
        _assert_key_followed_by(lines, "SuccessfulExit", "<false/>")
        # Crashed=true: abnormal termination should still be respawned.
        _assert_key_followed_by(lines, "Crashed", "<true/>")
        # ThrottleInterval=10: defensive second safety net rate-limiting respawns.
        _assert_key_followed_by(lines, "ThrottleInterval", "<integer>10</integer>")


def test_write_launchd_installer_reports_clear_error_on_corrupt_config_json() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-installer-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_repo = tmp / "repo"
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        write_fake_repo(fake_repo)
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()

        # A partially-written / truncated config.json parses to a JSONDecodeError.
        config_path = fake_repo / ".codelens" / "config.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text("{", encoding="utf-8")

        proc = run_installer(
            [
                str(fake_repo),
                "--no-build",
                "--bin-path",
                str(bin_path),
                "--model-dir",
                str(model_dir),
                "--launch-agents-dir",
                str(agents_dir),
            ]
        )

        # Corrupt config must abort the run (never silently proceed with a partial
        # install), but with a human-readable diagnostic — not a raw traceback.
        assert proc.returncode != 0, (
            "installer must not proceed past a corrupt config.json: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        assert (
            "Traceback" not in proc.stderr
            and "JSONDecodeError" not in proc.stderr
        ), (
            "installer leaked a raw Python traceback instead of a clear diagnostic: "
            f"stderr={proc.stderr}"
        )
        assert "invalid JSON" in proc.stderr, (
            "installer should report a human-readable 'invalid JSON' diagnostic: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )


def test_load_aborts_before_bootstrap_when_port_busy() -> None:
    # --load must bootout the old canonical instance, then WAIT for its port to
    # release before bootstrapping. It must also disable/bootout the legacy
    # readonly label so launchd cannot restart a second project writer.
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-installer-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_repo = tmp / "repo"
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        write_fake_repo(fake_repo)
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()

        shim_dir = tmp / "shim"
        shim_dir.mkdir()
        shim_log = tmp / "launchctl.log"
        write_launchctl_shim(shim_dir, shim_log)

        env = dict(os.environ)
        env["PATH"] = f"{shim_dir}:{env.get('PATH', '')}"
        env["LAUNCHCTL_SHIM_LOG"] = str(shim_log)
        env["CODELENS_PORT_RELEASE_SECS"] = "1"
        label_prefix = f"codelens-test-fixture-{os.getpid()}"

        with contextlib.closing(open_listener(0)) as mu_sock:
            mutation_port = str(mu_sock.getsockname()[1])
            proc = run_installer_with_env(
                [
                    str(fake_repo),
                    "--no-build",
                    "--load",
                    "--label-prefix",
                    label_prefix,
                    "--bin-path",
                    str(bin_path),
                    "--model-dir",
                    str(model_dir),
                    "--launch-agents-dir",
                    str(agents_dir),
                    "--mutation-port",
                    mutation_port,
                ],
                env,
            )

        combined = proc.stdout + proc.stderr
        shim_calls = shim_log.read_text(encoding="utf-8") if shim_log.exists() else ""
        assert proc.returncode != 0, (
            "installer --load exited 0 despite the port never releasing.\n"
            f"returncode={proc.returncode}\nstdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "still occupied" in combined, (
            "expected a port-release timeout diagnostic before bootstrap.\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "bootout" in shim_calls, (
            f"expected bootout first; shim calls:\n{shim_calls}"
        )
        assert "disable" in shim_calls and f"{label_prefix}-readonly" in shim_calls, (
            "installer must disable the legacy readonly label before loading the "
            f"canonical writer; shim calls:\n{shim_calls}"
        )
        assert "bootstrap" not in shim_calls, (
            "installer bootstrapped despite the port still being occupied.\n"
            f"shim calls:\n{shim_calls}"
        )


def test_load_bootstraps_after_ports_release() -> None:
    # Happy path: when the canonical writer port is free after bootout, --load
    # must run the release barrier and bootstrap exactly one service.
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-installer-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_repo = tmp / "repo"
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        write_fake_repo(fake_repo)
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()

        shim_dir = tmp / "shim"
        shim_dir.mkdir()
        shim_log = tmp / "launchctl.log"
        write_launchctl_shim(shim_dir, shim_log)

        # Reserve one ephemeral port, then release it before running installer.
        with contextlib.closing(open_listener(0)) as probe:
            mutation_port = str(probe.getsockname()[1])

        env = dict(os.environ)
        env["PATH"] = f"{shim_dir}:{env.get('PATH', '')}"
        env["LAUNCHCTL_SHIM_LOG"] = str(shim_log)
        env["CODELENS_PORT_RELEASE_SECS"] = "3"
        label_prefix = f"codelens-test-fixture-{os.getpid()}"

        proc = run_installer_with_env(
            [
                str(fake_repo),
                "--no-build",
                "--load",
                "--label-prefix",
                label_prefix,
                "--bin-path",
                str(bin_path),
                "--model-dir",
                str(model_dir),
                "--launch-agents-dir",
                str(agents_dir),
                "--mutation-port",
                mutation_port,
            ],
            env,
        )

        combined = proc.stdout + proc.stderr
        shim_calls = shim_log.read_text(encoding="utf-8") if shim_log.exists() else ""
        assert proc.returncode == 0, (
            "installer --load should complete when the ports are free.\n"
            f"returncode={proc.returncode}\nstdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert "ensuring port" in combined.lower(), (
            "installer --load did not run the port-release barrier before bootstrap.\n"
            f"stdout={proc.stdout}\nstderr={proc.stderr}"
        )
        assert shim_calls.count("bootstrap") == 1, (
            "expected exactly one canonical daemon to be bootstrapped after release.\n"
            f"shim calls:\n{shim_calls}"
        )
        assert f"{label_prefix}-readonly" in shim_calls, (
            "expected the legacy readonly label to be explicitly disabled/booted out.\n"
            f"shim calls:\n{shim_calls}"
        )
        assert f"{label_prefix}-readonly.plist" not in shim_calls, (
            "installer must not bootstrap the legacy readonly plist.\n"
            f"shim calls:\n{shim_calls}"
        )


def main() -> int:
    tests = [
        test_shared_http_docs_use_one_canonical_writer_endpoint,
        test_print_only_launchd_installer_completes_without_stdin_hang,
        test_write_launchd_installer_updates_config_without_stdin_hang,
        test_launchd_plist_keepalive_prevents_successful_exit_respawn_loop,
        test_write_launchd_installer_reports_clear_error_on_corrupt_config_json,
        test_load_aborts_before_bootstrap_when_port_busy,
        test_load_bootstraps_after_ports_release,
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
