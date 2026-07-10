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

import stat
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SCRIPT = REPO_ROOT / "scripts" / "install-http-daemons-launchd.sh"


def write_fake_executable(path: Path) -> None:
    path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


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
        assert "dev.codelens.mcp-readonly" in proc.stdout
        assert "dev.codelens.mcp-mutation" in proc.stdout
        assert "<string>review</string>" in proc.stdout
        assert "<string>builder</string>" in proc.stdout
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
        assert agents_dir.joinpath("dev.codelens.mcp-readonly.plist").is_file()
        assert agents_dir.joinpath("dev.codelens.mcp-mutation.plist").is_file()
        assert fake_repo.joinpath(".codelens/config.json").is_file()
        assert "Updated host attach overrides" in proc.stdout


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


def main() -> int:
    tests = [
        test_print_only_launchd_installer_completes_without_stdin_hang,
        test_write_launchd_installer_updates_config_without_stdin_hang,
        test_launchd_plist_keepalive_prevents_successful_exit_respawn_loop,
        test_write_launchd_installer_reports_clear_error_on_corrupt_config_json,
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
