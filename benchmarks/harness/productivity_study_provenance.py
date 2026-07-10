"""Fail-closed CodeLens binary provenance for controlled studies."""

from __future__ import annotations

import hashlib
import os
import re
import stat
import subprocess
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Final, TypedDict


VERSION_PATTERN: Final = re.compile(
    r"^codelens-mcp (?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?) "
    r"\(git (?P<git_sha>[0-9a-f]{7,40}|unknown), dirty (?P<dirty>true|false), "
    r"built (?P<built_at>[^\s)]+)\)$"
)


class CodelensBinaryProvenancePayload(TypedDict):
    version: str
    embedded_git_sha: str
    dirty: bool
    built_at: str
    content_sha256: str
    repo_head_sha: str


@dataclass(frozen=True, slots=True)
class CodelensBinaryProvenanceError(RuntimeError):
    message: str

    def __str__(self) -> str:
        return self.message


@dataclass(frozen=True, slots=True)
class CodelensBinaryRequest:
    repo: Path
    binary: Path


@dataclass(frozen=True, slots=True)
class CodelensBinaryProvenance:
    version: str
    embedded_git_sha: str
    dirty: bool
    built_at: str
    content_sha256: str
    repo_head_sha: str

    def payload(self) -> CodelensBinaryProvenancePayload:
        return {
            "version": self.version,
            "embedded_git_sha": self.embedded_git_sha,
            "dirty": self.dirty,
            "built_at": self.built_at,
            "content_sha256": self.content_sha256,
            "repo_head_sha": self.repo_head_sha,
        }


def inspect_codelens_binary(
    request: CodelensBinaryRequest,
) -> CodelensBinaryProvenance:
    """Parse and bind executable metadata to a clean repository HEAD."""
    require_executable_regular_file(request.binary)
    repo_head = git_text(request.repo, "rev-parse", "--verify", "HEAD")
    if git_text(request.repo, "status", "--porcelain=v1", "--untracked-files=all"):
        raise CodelensBinaryProvenanceError(
            f"CodeLens repository contains file mutations: {request.repo}"
        )
    content_before = file_sha256(request.binary)
    try:
        completed = subprocess.run(
            [str(request.binary), "--version"],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        raise CodelensBinaryProvenanceError(
            f"cannot execute CodeLens binary: {request.binary}"
        ) from error
    content_after = file_sha256(request.binary)
    if content_before != content_after:
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary mutated during provenance inspection: {request.binary}"
        )
    if completed.returncode != 0:
        raise CodelensBinaryProvenanceError(
            f"CodeLens --version failed with exit code {completed.returncode}"
        )
    match = VERSION_PATTERN.fullmatch(completed.stdout.strip())
    if match is None:
        raise CodelensBinaryProvenanceError("CodeLens --version output is malformed")
    embedded_git_sha = match.group("git_sha")
    if embedded_git_sha == "unknown":
        raise CodelensBinaryProvenanceError("CodeLens binary Git SHA is unknown")
    dirty = match.group("dirty") == "true"
    if dirty:
        raise CodelensBinaryProvenanceError(
            "CodeLens binary was built from a dirty tree"
        )
    if not repo_head.startswith(embedded_git_sha):
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary SHA {embedded_git_sha} does not match repository HEAD {repo_head}"
        )
    built_at = match.group("built_at")
    require_timezone_aware_timestamp(built_at)
    return CodelensBinaryProvenance(
        version=match.group("version"),
        embedded_git_sha=embedded_git_sha,
        dirty=dirty,
        built_at=built_at,
        content_sha256=content_after,
        repo_head_sha=repo_head,
    )


def require_executable_regular_file(binary: Path) -> None:
    try:
        mode = binary.stat().st_mode
    except OSError as error:
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary does not exist: {binary}"
        ) from error
    if not stat.S_ISREG(mode) or not os.access(binary, os.X_OK):
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary must be an executable regular file: {binary}"
        )


def require_timezone_aware_timestamp(value: str) -> None:
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary build timestamp is malformed: {value}"
        ) from error
    if parsed.tzinfo is None:
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary build timestamp lacks a timezone: {value}"
        )


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_text(repo: Path, *args: str) -> str:
    try:
        completed = subprocess.run(
            ["git", *args], cwd=repo, check=True, capture_output=True, text=True
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise CodelensBinaryProvenanceError(
            f"cannot inspect CodeLens repository: {repo}"
        ) from error
    return completed.stdout.strip()
