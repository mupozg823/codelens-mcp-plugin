#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import shutil
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass(frozen=True)
class ArtifactSpec:
    key: str
    relative_path: str
    label: str
    cleanup_safety: str
    regeneration_cost: str
    cleanup_supported: bool
    notes: str


ARTIFACT_SPECS: tuple[ArtifactSpec, ...] = (
    ArtifactSpec(
        key="target",
        relative_path="target",
        label="Rust build artifacts",
        cleanup_safety="safe",
        regeneration_cost="high",
        cleanup_supported=True,
        notes="Largest space user. Rebuilt by cargo check/test/build.",
    ),
    ArtifactSpec(
        key="fastembed_cache",
        relative_path=".fastembed_cache",
        label="Embedding model cache",
        cleanup_safety="safe",
        regeneration_cost="medium",
        cleanup_supported=True,
        notes="Re-downloaded on semantic embedding use.",
    ),
    ArtifactSpec(
        key="model_venv",
        relative_path="models/.venv",
        label="Python benchmark venv",
        cleanup_safety="safe",
        regeneration_cost="medium",
        cleanup_supported=True,
        notes="Recreated by reinstalling benchmark Python deps.",
    ),
    ArtifactSpec(
        key="codelens_cache",
        relative_path=".codelens",
        label="CodeLens runtime cache",
        cleanup_safety="caution",
        regeneration_cost="low",
        cleanup_supported=True,
        notes="Contains index, analysis cache, and audit history.",
    ),
    ArtifactSpec(
        key="benchmark_results",
        relative_path="benchmarks/results",
        label="Generated benchmark results",
        cleanup_safety="tracked",
        regeneration_cost="low",
        cleanup_supported=False,
        notes="Contains committed baseline reports; excluded from automated cleanup.",
    ),
)


def format_bytes(value: int) -> str:
    units = ["B", "KiB", "MiB", "GiB", "TiB"]
    size = float(value)
    for unit in units:
        if size < 1024.0 or unit == units[-1]:
            return f"{size:.1f} {unit}"
        size /= 1024.0
    return f"{value} B"


def stat_key(path: Path) -> tuple[int, int] | None:
    try:
        info = path.stat()
    except FileNotFoundError:
        return None
    if not stat.S_ISREG(info.st_mode):
        return None
    return (info.st_dev, info.st_ino)


def path_size_bytes(path: Path) -> int:
    if not path.exists():
        return 0
    if path.is_file():
        key = stat_key(path)
        return path.stat().st_size if key is not None else 0

    total = 0
    seen: set[tuple[int, int]] = set()
    for root, _, files in os.walk(path):
        for name in files:
            file_path = Path(root) / name
            key = stat_key(file_path)
            if key is None or key in seen:
                continue
            seen.add(key)
            total += file_path.stat().st_size
    return total


def iter_specs(keys: Iterable[str] | None) -> list[ArtifactSpec]:
    if not keys:
        return list(ARTIFACT_SPECS)
    selected = set(keys)
    return [spec for spec in ARTIFACT_SPECS if spec.key in selected]


def build_report(root: Path, keys: Iterable[str] | None, top: int) -> dict:
    repo_total = path_size_bytes(root)
    artifacts = []
    top_files: list[tuple[int, str]] = []
    seen_top_files: set[tuple[int, int]] = set()

    for spec in iter_specs(keys):
        path = root / spec.relative_path
        size = path_size_bytes(path)
        entry = {
            "key": spec.key,
            "path": str(path),
            "exists": path.exists(),
            "bytes": size,
            "size": format_bytes(size),
            "cleanup_safety": spec.cleanup_safety,
            "regeneration_cost": spec.regeneration_cost,
            "cleanup_supported": spec.cleanup_supported,
            "notes": spec.notes,
            "share_of_repo": round((size / repo_total), 4) if repo_total else 0.0,
        }
        artifacts.append(entry)

        if path.exists():
            if path.is_file():
                key = stat_key(path)
                if key is not None and key not in seen_top_files:
                    seen_top_files.add(key)
                    top_files.append((size, str(path)))
            else:
                for child in path.rglob("*"):
                    key = stat_key(child)
                    if key is None or key in seen_top_files:
                        continue
                    seen_top_files.add(key)
                    top_files.append((child.stat().st_size, str(child)))

    top_entries = [
        {"bytes": size, "size": format_bytes(size), "path": path}
        for size, path in sorted(top_files, reverse=True)[:top]
    ]

    return {
        "root": str(root),
        "repo_bytes": repo_total,
        "repo_size": format_bytes(repo_total),
        "artifacts": sorted(artifacts, key=lambda item: item["bytes"], reverse=True),
        "top_files": top_entries,
    }


def remove_path(path: Path) -> None:
    if not path.exists():
        return
    if path.is_file() or path.is_symlink():
        path.unlink()
        return
    shutil.rmtree(path)


def print_text_report(report: dict) -> None:
    print(f"Repo: {report['root']}")
    print(f"Total: {report['repo_size']}")
    print("")
    print("Artifacts:")
    for artifact in report["artifacts"]:
        status = "present" if artifact["exists"] else "missing"
        print(
            f"- {artifact['key']}: {artifact['size']} [{status}] "
            f"(safety={artifact['cleanup_safety']}, regen={artifact['regeneration_cost']}, cleanup={artifact['cleanup_supported']})"
        )
        print(f"  path: {artifact['path']}")
        print(f"  notes: {artifact['notes']}")
    if report["top_files"]:
        print("")
        print("Largest files:")
        for entry in report["top_files"]:
            print(f"- {entry['size']}: {entry['path']}")


def run_cleanup(root: Path, keys: Iterable[str] | None, apply: bool) -> int:
    selected_specs = iter_specs(keys)
    removed = 0
    for spec in selected_specs:
        path = root / spec.relative_path
        if not spec.cleanup_supported:
            print(f"SKIP: {path} ({spec.label}) is excluded from automated cleanup")
            continue
        if not path.exists():
            continue
        action = "REMOVE" if apply else "DRY-RUN"
        print(f"{action}: {path} ({spec.label})")
        if apply:
            remove_path(path)
            removed += 1
    return removed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Inspect and optionally clean large local artifacts."
    )
    parser.add_argument(
        "command",
        choices=("report", "cleanup"),
        nargs="?",
        default="report",
    )
    parser.add_argument("--root", default=".", help="Repo root to inspect.")
    parser.add_argument(
        "--scope",
        action="append",
        choices=[spec.key for spec in ARTIFACT_SPECS],
        help="Restrict to one or more artifact groups.",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=15,
        help="Number of largest files to show in report mode.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit machine-readable JSON for report mode.",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Actually delete selected artifacts in cleanup mode.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(args.root).resolve()

    if args.command == "report":
        report = build_report(root, args.scope, args.top)
        if args.json:
            print(json.dumps(report, indent=2))
        else:
            print_text_report(report)
        return 0

    removed = run_cleanup(root, args.scope, args.apply)
    if not args.apply:
        print("")
        print("No files were deleted. Re-run with --apply to perform cleanup.")
    else:
        print("")
        print(f"Removed {removed} artifact group(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
