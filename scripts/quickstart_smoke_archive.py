from __future__ import annotations

import tarfile
import zipfile
from pathlib import Path

from quickstart_smoke_contract import QuickstartSmokeError, QuickstartSummary
from quickstart_smoke_runner import run_installed_smoke


def extract_archive(path: Path, destination: Path) -> Path:
    archive_root = destination / "archive"
    archive_root.mkdir(parents=True)
    name = path.name
    if name.endswith(".tar.gz") or name.endswith(".tgz"):
        with tarfile.open(path, "r:gz") as archive:
            archive.extractall(archive_root)
        return archive_root
    if name.endswith(".zip"):
        with zipfile.ZipFile(path) as archive:
            archive.extractall(archive_root)
        return archive_root
    raise QuickstartSmokeError(f"unsupported archive format: {path}")


def find_archive_binary(root: Path) -> Path:
    candidates = [
        path
        for name in ("codelens-mcp", "codelens-mcp.exe")
        for path in sorted(root.rglob(name))
        if path.is_file()
    ]
    match candidates:
        case [binary]:
            return binary
        case []:
            raise QuickstartSmokeError(f"release archive binary not found under {root}")
        case _:
            rendered = ", ".join(str(path) for path in candidates)
            raise QuickstartSmokeError(f"release archive has multiple binaries: {rendered}")


def run_archive_smoke(
    archive: Path,
    root: Path,
    timeout: int,
    *,
    use_model_env: bool = False,
) -> QuickstartSummary:
    archive_root = extract_archive(archive, root)
    binary = find_archive_binary(archive_root)
    if archive.suffix == ".zip":
        binary.chmod(binary.stat().st_mode | 0o755)
    return run_installed_smoke(
        binary,
        root / "smoke",
        timeout,
        use_model_env=use_model_env,
        model_env_root=archive_root,
    )
