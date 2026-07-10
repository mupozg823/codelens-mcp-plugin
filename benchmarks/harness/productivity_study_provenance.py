"""Fail-closed CodeLens binary provenance for controlled studies."""

from __future__ import annotations

import re
import subprocess
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Final, TypedDict

from productivity_study_binary_snapshot import (
    BinarySnapshotError,
    BinarySnapshotRequest,
    file_sha256,
    materialize_binary_snapshot,
)
from productivity_study_candidate import study_process_environment

VERSION_PATTERN: Final = re.compile(
    r"^codelens-mcp (?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?) "
    r"\(git (?P<git_sha>[0-9a-f]{7,40}|unknown), dirty (?P<dirty>true|false), "
    r"built (?P<built_at>[^\s)]+)\)$"
)


class CodelensBinaryProvenancePayload(TypedDict):
    requested_path: str
    snapshot_path: str
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
    artifact_root: Path


@dataclass(frozen=True, slots=True)
class CodelensBinaryProvenance:
    requested_path: Path
    snapshot_path: Path
    version: str
    embedded_git_sha: str
    dirty: bool
    built_at: str
    content_sha256: str
    repo_head_sha: str

    def payload(self) -> CodelensBinaryProvenancePayload:
        return {
            "requested_path": str(self.requested_path),
            "snapshot_path": str(self.snapshot_path),
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
    """Capture, inspect, and bind an immutable executable to a clean HEAD."""
    repo_head = clean_repo_head(request.repo)
    try:
        snapshot = materialize_binary_snapshot(
            BinarySnapshotRequest(request.binary, request.artifact_root)
        )
    except BinarySnapshotError as error:
        raise CodelensBinaryProvenanceError(str(error)) from error
    if clean_repo_head(request.repo) != repo_head:
        raise CodelensBinaryProvenanceError(
            "CodeLens repository HEAD changed during binary snapshot capture"
        )
    try:
        completed = subprocess.run(
            [str(snapshot.snapshot_path), "--version"],
            env=study_process_environment(),
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        raise CodelensBinaryProvenanceError(
            f"cannot execute CodeLens binary snapshot: {snapshot.snapshot_path}"
        ) from error
    content_after = file_sha256(snapshot.snapshot_path)
    if content_after != snapshot.content_sha256:
        raise CodelensBinaryProvenanceError(
            f"CodeLens binary snapshot mutated during provenance inspection: {snapshot.snapshot_path}"
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
    if clean_repo_head(request.repo) != repo_head:
        raise CodelensBinaryProvenanceError(
            "CodeLens repository HEAD changed during binary provenance inspection"
        )
    return CodelensBinaryProvenance(
        requested_path=snapshot.requested_path,
        snapshot_path=snapshot.snapshot_path,
        version=match.group("version"),
        embedded_git_sha=embedded_git_sha,
        dirty=dirty,
        built_at=built_at,
        content_sha256=snapshot.content_sha256,
        repo_head_sha=repo_head,
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


def clean_repo_head(repo: Path) -> str:
    head = git_text(repo, "rev-parse", "--verify", "HEAD")
    if git_text(repo, "status", "--porcelain=v1", "--untracked-files=all"):
        raise CodelensBinaryProvenanceError(
            f"CodeLens repository contains file mutations: {repo}"
        )
    return head


def git_text(repo: Path, *args: str) -> str:
    try:
        completed = subprocess.run(
            ["git", *args],
            cwd=repo,
            env=study_process_environment(),
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise CodelensBinaryProvenanceError(
            f"cannot inspect CodeLens repository: {repo}"
        ) from error
    return completed.stdout.strip()
