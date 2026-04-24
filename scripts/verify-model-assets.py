#!/usr/bin/env python3
"""Verify packaged CodeLens semantic model assets."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

REQUIRED_MODEL_ASSETS = (
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
)
DEFAULT_MODEL_NAME = "MiniLM-L12-CodeSearchNet-INT8"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("models_root", help="Model root or direct codesearch model directory")
    parser.add_argument("--model-name", default=DEFAULT_MODEL_NAME)
    parser.add_argument("--write-manifest", default="")
    parser.add_argument("--quiet", action="store_true")
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_codesearch_dir(root: Path) -> Path:
    candidates = [root, root / "codesearch", root / "models" / "codesearch"]
    for candidate in candidates:
        if all((candidate / asset).is_file() for asset in REQUIRED_MODEL_ASSETS):
            return candidate
    checked = ", ".join(str(path) for path in candidates)
    raise SystemExit(
        "semantic model assets are incomplete; expected "
        f"{', '.join(REQUIRED_MODEL_ASSETS)} under one of: {checked}"
    )


def main() -> None:
    args = parse_args()
    model_dir = resolve_codesearch_dir(Path(args.models_root).expanduser().resolve())
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

    manifest = {
        "schema_version": "codelens-model-assets-v1",
        "model_name": args.model_name,
        "model_dir": "models/codesearch",
        "required_assets": list(REQUIRED_MODEL_ASSETS),
        "assets": assets,
    }
    if args.write_manifest:
        output = Path(args.write_manifest)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    if not args.quiet:
        print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
