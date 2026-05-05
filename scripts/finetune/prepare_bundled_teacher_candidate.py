#!/usr/bin/env python3
"""Prepare a bundled-teacher no-op candidate for promotion-gate validation."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import shutil
from datetime import datetime, timezone
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent
DEFAULT_TEACHER_DIR = REPO_ROOT / "crates" / "codelens-engine" / "models"
DEFAULT_OUTPUT_DIR = SCRIPT_DIR / "output" / "bundled-teacher-candidate"
DEFAULT_LABEL = "bundled-teacher-noop"
REQUIRED_MODEL_ASSETS = (
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
)


def preferred_variant() -> str:
    return "arm64" if platform.machine().lower() in {"arm64", "aarch64"} else "avx2"


def model_dir_candidates(base: Path) -> list[Path]:
    variant = preferred_variant()
    return [
        base,
        base / "codesearch",
        base / "onnx",
        base / variant,
        base / "codelens-code-search" / variant,
    ]


def has_required_assets(path: Path) -> bool:
    return all((path / asset).exists() for asset in REQUIRED_MODEL_ASSETS)


def resolve_model_dir(path: str | Path) -> Path:
    base = Path(path).expanduser().resolve()
    for candidate in model_dir_candidates(base):
        if has_required_assets(candidate):
            return candidate
    searched = ", ".join(str(candidate) for candidate in model_dir_candidates(base))
    raise SystemExit(f"No complete bundled teacher model found. Searched: {searched}")


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def copy_model_assets(source: Path, destination: Path) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    for item in source.iterdir():
        if item.is_file():
            shutil.copy2(item, destination / item.name)


def promotion_gate_command(output_dir: Path, label: str) -> list[str]:
    onnx_dir = output_dir / "onnx"
    manifest_path = onnx_dir / "model-manifest.json"
    return [
        "python3",
        "scripts/finetune/promotion_gate.py",
        "--candidate-onnx-dir",
        str(onnx_dir),
        "--candidate-label",
        label,
        "--candidate-manifest",
        str(manifest_path),
    ]


def prepare_candidate(
    teacher_dir: str | Path,
    output_dir: str | Path,
    *,
    label: str = DEFAULT_LABEL,
) -> Path:
    resolved_teacher = resolve_model_dir(teacher_dir)
    output = Path(output_dir).expanduser().resolve()
    onnx_dir = output / "onnx"
    copy_model_assets(resolved_teacher, onnx_dir)

    config_path = resolved_teacher / "config.json"
    config = (
        json.loads(config_path.read_text(encoding="utf-8"))
        if config_path.exists()
        else {}
    )
    model_path = resolved_teacher / "model.onnx"
    manifest = {
        "schema_version": "codelens-teacher-candidate-v1",
        "candidate_type": "bundled_teacher_noop",
        "model_name": label,
        "base_model": "MiniLM-L12-CodeSearchNet-INT8",
        "teacher_model": "MiniLM-L12-CodeSearchNet-INT8",
        "teacher_model_dir": str(resolved_teacher),
        "teacher_sha256": file_sha256(model_path),
        "teacher_size_bytes": model_path.stat().st_size,
        "num_hidden_layers": config.get("num_hidden_layers"),
        "hidden_size": config.get("hidden_size"),
        "adapter_type": "none",
        "export_backend": "onnx",
        "quantization": "bundled-int8",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "promotion_gate_command": promotion_gate_command(output, label),
    }
    manifest_path = onnx_dir / "model-manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return manifest_path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--teacher-dir", default=str(DEFAULT_TEACHER_DIR))
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument("--label", default=DEFAULT_LABEL)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    manifest_path = prepare_candidate(
        args.teacher_dir,
        args.output_dir,
        label=args.label,
    )
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    print(json.dumps(manifest, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
