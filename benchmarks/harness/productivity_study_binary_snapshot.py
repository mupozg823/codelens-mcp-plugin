"""Private content-addressed executable snapshots for controlled studies."""

from __future__ import annotations

import hashlib
import os
import stat
import tempfile
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class BinarySnapshotError(RuntimeError):
    message: str

    def __str__(self) -> str:
        return self.message


@dataclass(frozen=True, slots=True)
class BinarySnapshotRequest:
    binary: Path
    artifact_root: Path


@dataclass(frozen=True, slots=True)
class BinarySnapshot:
    requested_path: Path
    snapshot_path: Path
    content_sha256: str


def materialize_binary_snapshot(request: BinarySnapshotRequest) -> BinarySnapshot:
    """Publish one immutable snapshot keyed by its captured content hash."""
    snapshot_root = request.artifact_root / "binary-snapshots"
    request.artifact_root.mkdir(parents=True, exist_ok=True)
    ensure_private_directory(snapshot_root)
    temporary, digest = capture_to_temporary(request.binary, snapshot_root)
    try:
        digest_root = snapshot_root / digest
        ensure_private_directory(digest_root)
        snapshot_path = digest_root / "codelens-mcp"
        try:
            os.link(temporary, snapshot_path)
        except FileExistsError as error:
            if not snapshot_path.exists() and not snapshot_path.is_symlink():
                raise BinarySnapshotError(
                    f"snapshot collision disappeared before verification: {snapshot_path}"
                ) from error
        verify_published_snapshot(snapshot_path, digest)
        return BinarySnapshot(request.binary, snapshot_path, digest)
    finally:
        temporary.unlink(missing_ok=True)


def capture_to_temporary(binary: Path, snapshot_root: Path) -> tuple[Path, str]:
    require_requested_executable(binary)
    descriptor, raw_temporary = tempfile.mkstemp(prefix=".capture-", dir=snapshot_root)
    temporary = Path(raw_temporary)
    try:
        with os.fdopen(descriptor, "wb") as target:
            flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
            source_descriptor = os.open(binary, flags)
            with os.fdopen(source_descriptor, "rb") as source:
                before = os.fstat(source.fileno())
                require_stable_executable(before, binary)
                digest = hashlib.sha256()
                while chunk := source.read(1024 * 1024):
                    target.write(chunk)
                    digest.update(chunk)
                target.flush()
                os.fsync(target.fileno())
                after = os.fstat(source.fileno())
        if source_identity(before) != source_identity(after):
            raise BinarySnapshotError(
                f"requested binary mutated during snapshot capture: {binary}"
            )
        temporary.chmod(0o500)
        return temporary, digest.hexdigest()
    except BinarySnapshotError:
        temporary.unlink(missing_ok=True)
        raise
    except OSError as error:
        temporary.unlink(missing_ok=True)
        raise BinarySnapshotError(
            f"cannot capture requested executable binary: {binary}"
        ) from error


def require_requested_executable(binary: Path) -> None:
    try:
        status = binary.lstat()
    except OSError as error:
        raise BinarySnapshotError(
            f"CodeLens binary does not exist: {binary}"
        ) from error
    require_stable_executable(status, binary)


def require_stable_executable(status: os.stat_result, binary: Path) -> None:
    if not stat.S_ISREG(status.st_mode) or status.st_mode & 0o111 == 0:
        raise BinarySnapshotError(
            f"CodeLens binary must be an executable regular file: {binary}"
        )


def source_identity(status: os.stat_result) -> tuple[int, int, int, int, int]:
    return (
        status.st_dev,
        status.st_ino,
        status.st_size,
        status.st_mtime_ns,
        status.st_ctime_ns,
    )


def ensure_private_directory(path: Path) -> None:
    try:
        path.mkdir(mode=0o700)
    except FileExistsError:
        try:
            status = path.lstat()
        except OSError as error:
            raise BinarySnapshotError(
                f"snapshot directory collision: {path}"
            ) from error
        if not stat.S_ISDIR(status.st_mode):
            raise BinarySnapshotError(f"snapshot directory collision: {path}")
    path.chmod(0o700)


def verify_published_snapshot(path: Path, expected_sha256: str) -> None:
    try:
        status = path.lstat()
    except OSError as error:
        raise BinarySnapshotError(f"snapshot publication failed: {path}") from error
    if not stat.S_ISREG(status.st_mode):
        raise BinarySnapshotError(f"snapshot collision is not a regular file: {path}")
    if status.st_mode & 0o222 or status.st_mode & 0o111 == 0:
        raise BinarySnapshotError(f"snapshot collision has unsafe permissions: {path}")
    observed_sha256 = file_sha256(path)
    if observed_sha256 != expected_sha256:
        raise BinarySnapshotError(
            f"snapshot hash mismatch: expected {expected_sha256}, got {observed_sha256}"
        )


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()
