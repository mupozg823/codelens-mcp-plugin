#!/usr/bin/env python3
"""Fail-closed checks for bundled CodeLens semantic model assets."""

from __future__ import annotations

import argparse
import hashlib
import json
import tarfile
import tempfile
import zipfile
from pathlib import Path


REQUIRED_MODEL_ASSETS = (
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
)
REQUIRED_FILES = REQUIRED_MODEL_ASSETS
DEFAULT_MODEL_NAME = "MiniLM-L12-CodeSearchNet-INT8"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group()
    target.add_argument(
        "models_root",
        nargs="?",
        help="Model root or direct codesearch model directory",
    )
    target.add_argument("--root", help="Directory to inspect")
    target.add_argument("--archive", help="Release archive to extract and inspect")
    parser.add_argument("--model-name", default=DEFAULT_MODEL_NAME)
    parser.add_argument("--write-manifest", default="")
    parser.add_argument("--quiet", action="store_true")
    parser.add_argument(
        "--arch",
        choices=("auto", "arm64", "avx2"),
        default="auto",
        help="Architecture variant for release layout checks",
    )
    parser.add_argument("--json", action="store_true", help="Print machine-readable result")
    parser.add_argument(
        "--check",
        action="store_true",
        help="Compatibility flag for CI gate calls; failures are always non-zero",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def candidate_dirs(root: Path, arch: str) -> list[Path]:
    variants = ["arm64", "avx2"] if arch == "auto" else [arch]
    bases = [root]
    bases.extend(path for path in sorted(root.iterdir()) if path.is_dir())
    dirs = []
    for base in bases:
        dirs.extend(
            [
                base / "models" / "codesearch",
                base / "codesearch",
                base,
            ]
        )
        for variant in variants:
            dirs.extend(
                [
                    base / "models" / "codelens-code-search" / variant / "onnx",
                    base / "models" / "codesearch" / variant / "onnx",
                    base / "codelens-code-search" / variant / "onnx",
                    base / "codesearch" / variant / "onnx",
                ]
            )
    seen: set[Path] = set()
    unique = []
    for path in dirs:
        resolved = path.resolve()
        if resolved not in seen:
            seen.add(resolved)
            unique.append(path)
    return unique


def missing_files(model_dir: Path) -> list[str]:
    return [name for name in REQUIRED_MODEL_ASSETS if not (model_dir / name).is_file()]


def symlinked_required_files(model_dir: Path) -> list[str]:
    return [name for name in REQUIRED_MODEL_ASSETS if (model_dir / name).is_symlink()]


def find_model_dir(root: Path, arch: str) -> tuple[Path | None, list[dict[str, object]]]:
    attempts = []
    for path in candidate_dirs(root, arch):
        missing = missing_files(path)
        symlinked = symlinked_required_files(path)
        attempts.append(
            {"path": str(path), "missing": missing, "symlinked": symlinked}
        )
        if not missing and not symlinked:
            return path, attempts
    return None, attempts


def extract_archive(path: Path, dest: Path) -> None:
    name = path.name
    if name.endswith(".tar.gz") or name.endswith(".tgz"):
        with tarfile.open(path, "r:gz") as archive:
            archive.extractall(dest)
        return
    if name.endswith(".zip"):
        with zipfile.ZipFile(path) as archive:
            archive.extractall(dest)
        return
    raise SystemExit(f"unsupported archive format: {path}")


def root_from_args(args: argparse.Namespace, tmpdir: Path | None) -> Path:
    if args.archive:
        archive = Path(args.archive).resolve()
        if not archive.is_file():
            raise SystemExit(f"archive not found: {archive}")
        assert tmpdir is not None
        extract_archive(archive, tmpdir)
        children = [p for p in tmpdir.iterdir()]
        if len(children) == 1 and children[0].is_dir():
            return children[0]
        return tmpdir
    root = args.root or args.models_root or "."
    return Path(root).expanduser().resolve()


def model_manifest(model_dir: Path, model_name: str) -> dict[str, object]:
    assets = []
    for asset in REQUIRED_MODEL_ASSETS:
        path = model_dir / asset
        assets.append(
            {
                "name": asset,
                "sha256": sha256(path),
                "size_bytes": path.stat().st_size,
            }
        )
    return {
        "schema_version": "codelens-model-assets-v1",
        "model_name": model_name,
        "model_dir": "models/codesearch",
        "required_assets": list(REQUIRED_MODEL_ASSETS),
        "assets": assets,
    }


def print_failure(root: Path, attempts: list[dict[str, object]]) -> None:
    print(f"model assets missing under: {root}")
    for attempt in attempts:
        details = []
        if attempt["missing"]:
            details.append(f"missing {', '.join(attempt['missing'])}")
        if attempt["symlinked"]:
            details.append(
                f"symlinked required files {', '.join(attempt['symlinked'])}"
            )
        print(f"- {attempt['path']}: {'; '.join(details) or 'ok'}")


def main() -> None:
    args = parse_args()
    with tempfile.TemporaryDirectory(prefix="codelens-model-assets-") as raw_tmp:
        tmpdir = Path(raw_tmp) if args.archive else None
        root = root_from_args(args, tmpdir)
        model_dir, attempts = find_model_dir(root, args.arch)
        result = {
            "ok": model_dir is not None,
            "root": str(root),
            "model_dir": str(model_dir) if model_dir else None,
            "required_files": list(REQUIRED_MODEL_ASSETS),
            "attempts": attempts,
        }

        if model_dir is None:
            if args.json:
                print(json.dumps(result, indent=2))
            elif not args.quiet:
                print_failure(root, attempts)
            raise SystemExit(1)

        manifest = model_manifest(model_dir, args.model_name)
        if args.write_manifest:
            output = Path(args.write_manifest)
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

        if args.json:
            result["manifest"] = manifest
            print(json.dumps(result, indent=2))
        elif not args.quiet:
            print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
