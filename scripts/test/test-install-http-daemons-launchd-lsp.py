# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
#   uv run scripts/test/test-install-http-daemons-launchd-lsp.py
#   python3 scripts/test/test-install-http-daemons-launchd-lsp.py
# ------------------

from __future__ import annotations

import os
import plistlib
import stat
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SCRIPT = REPO_ROOT / "scripts" / "install-http-daemons-launchd.sh"


def write_fake_executable(path: Path) -> None:
    path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def test_launchd_plist_preserves_stable_lsp_paths() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-launchd-lsp-") as raw_tmp:
        tmp = Path(raw_tmp)
        fake_repo = tmp / "repo"
        bin_path = tmp / "codelens-mcp-http"
        model_dir = tmp / "models"
        agents_dir = tmp / "LaunchAgents"
        node_install = tmp / "node & lsp install"
        node_bin = node_install / "bin"
        session_link = tmp / "fnm & state" / "session"
        extra_bin = tmp / "extra & lsp bin"

        fake_repo.joinpath("crates/codelens-mcp").mkdir(parents=True)
        fake_repo.joinpath("Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        fake_repo.joinpath("crates/codelens-mcp/Cargo.toml").write_text(
            '[package]\nname = "codelens-mcp"\nversion = "0.0.0"\nedition = "2021"\n',
            encoding="utf-8",
        )
        write_fake_executable(bin_path)
        model_dir.mkdir()
        agents_dir.mkdir()
        node_bin.mkdir(parents=True)
        session_link.parent.mkdir()
        session_link.symlink_to(node_install, target_is_directory=True)
        extra_bin.mkdir()
        write_fake_executable(node_bin / "node")
        write_fake_executable(node_bin / "pyright-langserver")

        installer_path = os.pathsep.join(
            [str(session_link / "bin"), "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        )
        explicit_lsp_path = str(extra_bin)
        expected_lsp_path = os.pathsep.join(
            [str(node_bin.resolve()), explicit_lsp_path]
        )
        expected_launchd_path = os.pathsep.join([expected_lsp_path, installer_path])
        env = dict(os.environ)
        env["PATH"] = installer_path
        env["CODELENS_LSP_PATH_EXTRA"] = explicit_lsp_path

        proc = subprocess.run(
            [
                "bash",
                str(INSTALL_SCRIPT),
                str(fake_repo),
                "--no-build",
                "--bin-path",
                str(bin_path),
                "--model-dir",
                str(model_dir),
                "--launch-agents-dir",
                str(agents_dir),
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
            env=env,
        )

        assert proc.returncode == 0, (
            "installer should preserve the runtime needed by Node-backed LSPs: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        plist = plistlib.loads(
            agents_dir.joinpath("dev.codelens.mcp-mutation.plist").read_bytes()
        )
        launch_env = plist["EnvironmentVariables"]
        assert launch_env.get("PATH") == expected_launchd_path
        assert launch_env.get("CODELENS_LSP_PATH_EXTRA") == expected_lsp_path


def main() -> int:
    test_launchd_plist_preserves_stable_lsp_paths()
    print("PASS  test_launchd_plist_preserves_stable_lsp_paths")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
