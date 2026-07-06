"""Isolated worktree setup for semantic index lifecycle benchmarks."""

from __future__ import annotations

import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Final


GIT_SNAPSHOT_TIMEOUT_SECONDS: Final = 120
COPY_IGNORE_NAMES: Final = frozenset(
    {
        ".codelens",
        ".fastembed_cache",
        ".git",
        ".pytest_cache",
        ".ruff_cache",
        ".serena",
        ".venv",
        ".worktrees",
        "__pycache__",
        "node_modules",
        "target",
    }
)
COPY_IGNORE_SUFFIXES: Final = (".onnx", ".onnx.bak", ".pyc", ".pyo")


def copy_ignore(_directory: str, names: list[str]) -> set[str]:
    ignored: set[str] = set()
    for name in names:
        if name in COPY_IGNORE_NAMES or name.endswith(COPY_IGNORE_SUFFIXES):
            ignored.add(name)
    return ignored


def isolated_project_copy(source: Path) -> tuple[tempfile.TemporaryDirectory[str], Path]:
    tmpdir = tempfile.TemporaryDirectory(prefix="codelens-index-lifecycle-")
    destination = Path(tmpdir.name) / source.name
    shutil.copytree(source, destination, symlinks=True, ignore=copy_ignore)
    return tmpdir, destination


def initialize_git_snapshot(worktree: Path) -> str:
    commands = [
        ["git", "init", "-q"],
        ["git", "config", "user.email", "codelens-benchmark@example.invalid"],
        ["git", "config", "user.name", "CodeLens Benchmark"],
        ["git", "add", "-A"],
        ["git", "commit", "-qm", "benchmark snapshot"],
    ]
    for command in commands:
        try:
            result = subprocess.run(
                command,
                cwd=worktree,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=GIT_SNAPSHOT_TIMEOUT_SECONDS,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return "isolated_copy_no_git"
        if result.returncode != 0:
            return "isolated_copy_no_git"
    return "isolated_git_snapshot"
