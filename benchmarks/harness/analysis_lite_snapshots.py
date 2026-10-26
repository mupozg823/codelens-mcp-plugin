"""Source-only Git-object snapshots; receipts stay outside evaluated folders."""

from __future__ import annotations

import hashlib
import io
import subprocess
import tarfile
from pathlib import Path, PurePosixPath

SOURCE_EXTENSIONS = {".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json", ".css", ".svg"}
SAFE_IGNORE_NAMES = {".gitignore", ".ignore", ".codelensignore"}
RUNTIME_PARTS = {"node_modules", ".codelens"}


def source_hashes(root):
    result = {}
    for path in sorted(Path(root).rglob("*")):
        relative = path.relative_to(root)
        if relative.parts[0] in RUNTIME_PARTS:
            continue
        if path.is_symlink():
            raise ValueError(f"unexpected source symlink: {relative}")
        if path.is_file():
            result[relative.as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def prepare_snapshot(snapshot, destination):
    """Create once, archive only selected tracked blobs, and verify every byte.

    All exclusions include immutable Git blob identities. No original worktree
    contents, symlinks, history, credentials, docs, old answers or gold are copied.
    The caller must save the returned inventory outside the source snapshot.
    """
    repo, revision = snapshot["repo"], snapshot["base_sha"]
    if len(revision) != 40 or any(char not in "0123456789abcdef" for char in revision):
        raise ValueError("snapshot requires an exact commit SHA")
    tree = subprocess.check_output(["git", "-C", repo, "rev-parse", f"{revision}^{{tree}}"], text=True).strip()
    if tree != snapshot["tree_sha"]:
        raise ValueError("frozen Git tree drift")
    raw = subprocess.check_output(["git", "-C", repo, "ls-tree", "-r", "-l", "-z", revision])
    selected, excluded = [], []
    for record in raw.split(b"\0"):
        if not record:
            continue
        metadata, encoded_name = record.split(b"\t", 1)
        mode, _, blob, size = metadata.decode().split()
        name = encoded_name.decode(); path = PurePosixPath(name)
        row = {"path": name, "git_blob_sha1": blob, "bytes": int(size) if size != "-" else 0}
        forbidden = (path.is_absolute() or ".." in path.parts or any(part.startswith(".") for part in path.parts[:-1])
                     or (path.name.startswith(".") and path.name not in SAFE_IGNORE_NAMES)
                     or path.name in {"auth.json", "credentials.json", "gold.json"}
                     or any(part in {"docs", "backups", "releases", "dist", "out", "coverage", "node_modules"} for part in path.parts)
                     or path.suffix in {".pem", ".key"} or any(part.lower() in {"answers", "gold", "grader"} for part in path.parts))
        allowed = ((path.suffix in SOURCE_EXTENSIONS or path.name in {"AGENTS.md", "CLAUDE.md"} | SAFE_IGNORE_NAMES)
                   and not forbidden and mode in {"100644", "100755"})
        row["reason"] = "selected-source/config" if allowed else "excluded-path/type/link"
        (selected if allowed else excluded).append(row)
    included = {row["path"] for row in selected}
    if not set(snapshot["file_sha256"]).issubset(included):
        raise ValueError("selection excludes a mandatory frozen source file")
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=False)
    for start in range(0, len(selected), 100):
        names = [row["path"] for row in selected[start:start + 100]]
        archive = subprocess.check_output(["git", "--literal-pathspecs", "-C", repo, "archive", "--format=tar", revision, "--", *names])
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            for member in tar:
                path = PurePosixPath(member.name)
                if member.isdir():
                    continue
                if not member.isfile() or member.name not in included or path.is_absolute() or ".." in path.parts:
                    raise ValueError(f"unsafe archive member: {member.name}")
                target = destination / member.name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(tar.extractfile(member).read())
                target.chmod(0o755 if member.mode & 0o111 else 0o644)
    hashes = source_hashes(destination)
    if set(hashes) != included:
        raise ValueError("archive omitted or added source paths")
    for row in selected:
        content = (destination / row["path"]).read_bytes()
        if hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest() != row["git_blob_sha1"]:
            raise ValueError(f"archive content drift: {row['path']}")
    for name, expected in snapshot["file_sha256"].items():
        if hashes[name] != expected:
            raise ValueError(f"frozen source hash drift: {name}")
    return {"revision": revision, "tree_sha": tree, "destination": str(destination),
            "selected": selected, "excluded": excluded, "source_sha256": hashes,
            "selected_bytes": sum(row["bytes"] for row in selected),
            "excluded_bytes": sum(row["bytes"] for row in excluded)}
