"""Base-only Git repositories for productivity-study candidates."""

from __future__ import annotations

import os
import shutil
import subprocess
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Final, Iterator


GIT_OVERRIDE_KEYS: Final = frozenset(
    {
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_CONFIG_COUNT",
        "GIT_DIR",
        "GIT_INDEX_FILE",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_PREFIX",
        "GIT_QUARANTINE_PATH",
        "GIT_WORK_TREE",
    }
)


@dataclass(frozen=True, slots=True)
class CandidateCheckoutError(RuntimeError):
    message: str

    def __str__(self) -> str:
        return self.message


@dataclass(frozen=True, slots=True)
class CandidateCheckoutRequest:
    source_repo: Path
    base_sha: str
    target_sha: str
    run_root: Path
    run_id: str

    @property
    def destination(self) -> Path:
        return self.run_root / self.run_id


@contextmanager
def disposable_candidate_checkout(
    request: CandidateCheckoutRequest,
) -> Iterator[Path]:
    """Yield an isolated shallow repository containing only the base commit."""
    destination = request.destination
    if destination.exists():
        raise CandidateCheckoutError(
            f"candidate destination already exists: {destination}"
        )
    request.run_root.mkdir(parents=True, exist_ok=True)
    environment = isolated_git_environment()
    try:
        run_git(request.run_root, environment, "init", "--quiet", str(destination))
        run_git(
            destination,
            environment,
            "fetch",
            "--quiet",
            "--depth=1",
            "--no-tags",
            "--no-write-fetch-head",
            str(request.source_repo.resolve()),
            f"{request.base_sha}:refs/codelens-study/base",
        )
        run_git(
            destination,
            environment,
            "checkout",
            "--quiet",
            "--detach",
            request.base_sha,
        )
        run_git(
            destination, environment, "update-ref", "-d", "refs/codelens-study/base"
        )
        verify_candidate_repository(request, destination, environment)
        yield destination
    finally:
        if destination.exists():
            shutil.rmtree(destination)


def isolated_git_environment() -> dict[str, str]:
    return {
        key: value
        for key, value in os.environ.items()
        if key not in GIT_OVERRIDE_KEYS
        and not key.startswith("GIT_CONFIG_KEY_")
        and not key.startswith("GIT_CONFIG_VALUE_")
    }


def verify_candidate_repository(
    request: CandidateCheckoutRequest,
    destination: Path,
    environment: dict[str, str],
) -> None:
    head = run_git(destination, environment, "rev-parse", "HEAD").stdout.strip()
    if head != request.base_sha:
        raise CandidateCheckoutError(
            f"candidate HEAD mismatch: expected {request.base_sha}, got {head}"
        )
    target = run_git(
        destination,
        environment,
        "cat-file",
        "-e",
        f"{request.target_sha}^{{commit}}",
        check=False,
    )
    if target.returncode == 0:
        raise CandidateCheckoutError(
            f"candidate can resolve hidden target commit: {request.target_sha}"
        )
    if run_git(destination, environment, "remote").stdout.strip():
        raise CandidateCheckoutError(
            "candidate repository retained a configured remote"
        )
    refs = run_git(destination, environment, "for-each-ref", "--format=%(refname)")
    if refs.stdout.strip():
        raise CandidateCheckoutError("candidate repository retained source refs")
    shallow = run_git(
        destination, environment, "rev-parse", "--is-shallow-repository"
    ).stdout.strip()
    if shallow != "true":
        raise CandidateCheckoutError("candidate repository is not a depth-one fetch")


def run_git(
    cwd: Path,
    environment: dict[str, str],
    *args: str,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            ["git", *args],
            cwd=cwd,
            env=environment,
            check=check,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise CandidateCheckoutError(
            f"candidate Git command failed: git {' '.join(args)}"
        ) from error
