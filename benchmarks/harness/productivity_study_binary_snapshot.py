"""Private content-addressed executable snapshots for controlled studies."""

from __future__ import annotations

import hashlib
import os
import shutil
import stat
import tempfile
from contextlib import ExitStack, contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator


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


@contextmanager
def bound_binary_snapshot(snapshot_path: Path, expected_sha256: str) -> Iterator[Path]:
    """Bind verified snapshot content to one private daemon execution inode."""
    with ExitStack() as resources:
        source_descriptor = open_snapshot_descriptor(snapshot_path)
        resources.callback(os.close, source_descriptor)
        source_status = os.fstat(source_descriptor)
        verify_source_descriptor(
            source_descriptor, source_status, snapshot_path, expected_sha256
        )
        try:
            raw_execution_root = tempfile.mkdtemp(
                prefix=".exec-", dir=snapshot_path.parent
            )
        except OSError as error:
            raise BinarySnapshotError(
                f"cannot create private binary execution directory: {snapshot_path.parent}"
            ) from error
        execution_root = Path(raw_execution_root)
        execution_root.chmod(0o700)
        resources.callback(shutil.rmtree, execution_root)
        bound_path = execution_root / "codelens-mcp"
        create_bound_hardlink(snapshot_path, bound_path)
        bound_path.chmod(0o500)
        source_identity_value = (source_status.st_dev, source_status.st_ino)
        verify_bound_executable(
            bound_path, source_identity_value, expected_sha256, phase="before execution"
        )
        try:
            yield bound_path
        finally:
            verify_bound_executable(
                bound_path,
                source_identity_value,
                expected_sha256,
                phase="during daemon execution",
            )


def open_snapshot_descriptor(path: Path) -> int:
    try:
        return os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    except OSError as error:
        raise BinarySnapshotError(
            f"cannot open snapshot for execution binding: {path}"
        ) from error


def verify_source_descriptor(
    descriptor: int,
    status: os.stat_result,
    path: Path,
    expected_sha256: str,
) -> None:
    if not stat.S_ISREG(status.st_mode):
        raise BinarySnapshotError(f"snapshot execution source is not regular: {path}")
    if hash_descriptor(descriptor) != expected_sha256:
        raise BinarySnapshotError(f"snapshot hash mismatch before binding: {path}")


def create_bound_hardlink(source: Path, destination: Path) -> None:
    try:
        os.link(source, destination, follow_symlinks=False)
    except OSError as error:
        raise BinarySnapshotError(
            f"cannot bind snapshot inode for daemon execution: {source}"
        ) from error


def verify_bound_executable(
    path: Path,
    expected_identity: tuple[int, int],
    expected_sha256: str,
    *,
    phase: str,
) -> None:
    try:
        status = path.lstat()
    except OSError as error:
        raise BinarySnapshotError(
            f"bound executable mutated {phase}: {path}"
        ) from error
    identity = (status.st_dev, status.st_ino)
    if not stat.S_ISREG(status.st_mode) or identity != expected_identity:
        raise BinarySnapshotError(f"bound executable mutated {phase}: {path}")
    descriptor = open_snapshot_descriptor(path)
    try:
        opened_status = os.fstat(descriptor)
        opened_identity = (opened_status.st_dev, opened_status.st_ino)
        content_matches = hash_descriptor(descriptor) == expected_sha256
    finally:
        os.close(descriptor)
    if opened_identity != expected_identity or not content_matches:
        raise BinarySnapshotError(f"bound executable mutated {phase}: {path}")
    if status.st_mode & 0o222 or status.st_mode & 0o111 == 0:
        raise BinarySnapshotError(f"bound executable mutated {phase}: {path}")


def hash_descriptor(descriptor: int) -> str:
    os.lseek(descriptor, 0, os.SEEK_SET)
    digest = hashlib.sha256()
    while chunk := os.read(descriptor, 1024 * 1024):
        digest.update(chunk)
    os.lseek(descriptor, 0, os.SEEK_SET)
    return digest.hexdigest()
