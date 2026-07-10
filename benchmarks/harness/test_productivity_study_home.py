#!/usr/bin/env python3
"""Tests for isolated productivity-study process homes."""

from __future__ import annotations

import os
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_home as home


PROFILE_NAMES = (
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".zprofile",
    ".zlogin",
    ".zshrc",
)


def make_source_home(root: Path) -> Path:
    source = root / "source-home"
    source.mkdir()
    for profile_name in PROFILE_NAMES:
        (source / profile_name).write_text(
            "export STUDY_PROFILE_SENTINEL=loaded\n"
            "export GIT_DIR=/tmp/reintroduced-from-profile.git\n",
            encoding="utf-8",
        )
    (source / ".gitconfig").write_text("[core]\n\thooksPath = /tmp/hooks\n")
    source_claude = source / ".claude"
    source_claude.mkdir()
    (source_claude / "settings.json").write_text('{"unsafe":true}\n')
    (source / ".claude.json").write_text('{"identity":"preserved"}\n')
    for relative in (
        "Library/Keychains",
        ".cache/codelens",
        ".codex",
        ".cargo",
        ".rustup",
    ):
        (source / relative).mkdir(parents=True)
    return source


def test_isolated_home_copies_only_identity_and_links_exact_shared_state() -> None:
    # Given: a source home containing identity, caches, profiles, and user config.
    with tempfile.TemporaryDirectory(prefix="codelens-study-home-test-") as raw_tmp:
        root = Path(raw_tmp)
        source = make_source_home(root)
        candidate = root / "candidate"
        candidate.mkdir()

        # When: a study process home is materialized outside the candidate.
        with home.isolated_process_home(source, candidate) as process_home:
            mode = stat.S_IMODE(process_home.stat().st_mode)

            # Then: only the explicit identity and cache contract is present.
            assert process_home.is_relative_to(candidate) is False
            assert mode == 0o700
            assert tuple((process_home / ".claude").iterdir()) == ()
            identity = process_home / ".claude.json"
            assert identity.is_file()
            assert identity.is_symlink() is False
            assert identity.read_bytes() == (source / ".claude.json").read_bytes()
            assert os.readlink(process_home / "Library/Keychains") == str(
                source / "Library/Keychains"
            )
            assert os.readlink(process_home / ".cache/codelens") == str(
                source / ".cache/codelens"
            )
            assert all(
                (process_home / name).exists() is False for name in PROFILE_NAMES
            )
            assert (process_home / ".gitconfig").exists() is False
            assert (process_home / "Library/Preferences").exists() is False
            assert (process_home / ".codex").exists() is False

        assert process_home.exists() is False


def test_process_environment_blocks_source_login_profiles_in_descendant_shells() -> (
    None
):
    # Given: malicious login profiles in the source home.
    with tempfile.TemporaryDirectory(prefix="codelens-study-home-test-") as raw_tmp:
        root = Path(raw_tmp)
        source = make_source_home(root)
        candidate = root / "candidate"
        candidate.mkdir()

        # When: login descendants run with the isolated process environment.
        with home.isolated_process_home(source, candidate) as process_home:
            environment = home.study_process_environment(
                process_home=process_home,
                source_home=source,
            )
            for shell in ("bash", "sh", "zsh"):
                completed = subprocess.run(
                    [
                        shell,
                        "-lc",
                        'test -z "${STUDY_PROFILE_SENTINEL+x}" && test -z "${GIT_DIR+x}"',
                    ],
                    env=environment,
                    check=False,
                    capture_output=True,
                    text=True,
                )
                assert completed.returncode == 0, (shell, completed.stderr)

            # Then: tool homes point only at explicitly preserved source directories.
            assert environment["HOME"] == str(process_home)
            assert environment["CODEX_HOME"] == str(source / ".codex")
            assert environment["CARGO_HOME"] == str(source / ".cargo")
            assert environment["RUSTUP_HOME"] == str(source / ".rustup")


def main() -> int:
    tests = (
        test_isolated_home_copies_only_identity_and_links_exact_shared_state,
        test_process_environment_blocks_source_login_profiles_in_descendant_shells,
    )
    for test in tests:
        test()
        print(f"PASS  {test.__name__}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
