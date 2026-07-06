#!/usr/bin/env python3
"""Regression tests for standard release archive payload contracts."""

from __future__ import annotations

import hashlib
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PACKAGE_SCRIPT = REPO_ROOT / "scripts" / "package-release-artifact.sh"
VERIFY_SCRIPT = REPO_ROOT / "scripts" / "verify-release-artifacts.sh"
FORMULA = REPO_ROOT / "Formula" / "codelens-mcp.rb"
REQUIRED_MODEL_ASSETS = (
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_fake_models(root: Path) -> None:
    model_dir = root / "codesearch"
    model_dir.mkdir(parents=True)
    for asset in REQUIRED_MODEL_ASSETS:
        model_dir.joinpath(asset).write_text(f"fake {asset}\n", encoding="utf-8")


def test_package_script_emits_standard_payload_only() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-package-contract-") as raw_tmp:
        tmp = Path(raw_tmp)
        models_dir = tmp / "models"
        output_dir = tmp / "dist"
        binary = tmp / "codelens-mcp"
        binary.write_text("#!/bin/sh\n", encoding="utf-8")
        write_fake_models(models_dir)

        proc = subprocess.run(
            [
                "bash",
                str(PACKAGE_SCRIPT),
                "--name",
                "linux-x86_64",
                "--binary",
                str(binary),
                "--binary-name",
                "codelens-mcp",
                "--archive-ext",
                ".tar.gz",
                "--output-dir",
                str(output_dir),
                "--models-dir",
                str(models_dir),
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        assert proc.returncode == 0, (
            f"package script should succeed: stdout={proc.stdout} stderr={proc.stderr}"
        )

        archive = output_dir / "codelens-mcp-linux-x86_64.tar.gz"
        with tarfile.open(archive, "r:gz") as tf:
            entries = sorted(member.name for member in tf.getmembers() if member.isfile())

        assert entries == [
            "codelens-mcp",
            "models/codesearch/config.json",
            "models/codesearch/model-manifest.json",
            "models/codesearch/model.onnx",
            "models/codesearch/special_tokens_map.json",
            "models/codesearch/tokenizer.json",
            "models/codesearch/tokenizer_config.json",
        ]


def test_release_verifier_rejects_standard_archive_adapter_payload() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-release-contract-") as raw_tmp:
        dist = Path(raw_tmp)
        payload = dist / "payload"
        payload.joinpath("models/codesearch").mkdir(parents=True)
        payload.joinpath("adapters/roslyn-workspace-service").mkdir(parents=True)
        payload.joinpath("codelens-mcp").write_text("#!/bin/sh\n", encoding="utf-8")
        for asset in REQUIRED_MODEL_ASSETS:
            payload.joinpath("models/codesearch", asset).write_text(
                f"fake {asset}\n", encoding="utf-8"
            )
        payload.joinpath("models/codesearch/model-manifest.json").write_text(
            "{}\n", encoding="utf-8"
        )
        payload.joinpath(
            "adapters/roslyn-workspace-service/CodeLens.Roslyn.WorkspaceService.dll"
        ).write_text("should not ship\n", encoding="utf-8")

        archive = dist / "codelens-mcp-linux-x86_64.tar.gz"
        with tarfile.open(archive, "w:gz") as tf:
            for path in sorted(payload.rglob("*")):
                tf.add(path, arcname=path.relative_to(payload).as_posix())

        archive.with_suffix(archive.suffix + ".sig").write_text("sig\n", encoding="utf-8")
        archive.with_suffix(archive.suffix + ".pem").write_text("cert\n", encoding="utf-8")
        dist.joinpath("checksums-sha256.txt").write_text(
            f"{sha256(archive)}  {archive.name}\n", encoding="utf-8"
        )

        proc = subprocess.run(
            [
                "bash",
                str(VERIFY_SCRIPT),
                str(dist),
                "--require-targets",
                "linux-x86_64",
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        assert proc.returncode == 1, (
            "release verifier should reject adapter payloads in standard archives: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        assert "adapter" in proc.stderr.lower() or "roslyn" in proc.stderr.lower()


def test_package_script_emits_standard_windows_zip_payload_only() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-package-contract-") as raw_tmp:
        tmp = Path(raw_tmp)
        models_dir = tmp / "models"
        output_dir = tmp / "dist"
        binary = tmp / "codelens-mcp.exe"
        binary.write_text("fake exe\n", encoding="utf-8")
        write_fake_models(models_dir)

        proc = subprocess.run(
            [
                "bash",
                str(PACKAGE_SCRIPT),
                "--name",
                "windows-x86_64",
                "--binary",
                str(binary),
                "--binary-name",
                "codelens-mcp.exe",
                "--archive-ext",
                ".zip",
                "--output-dir",
                str(output_dir),
                "--models-dir",
                str(models_dir),
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        assert proc.returncode == 0, (
            f"package script should succeed: stdout={proc.stdout} stderr={proc.stderr}"
        )

        archive = output_dir / "codelens-mcp-windows-x86_64.zip"
        with zipfile.ZipFile(archive) as zf:
            entries = sorted(
                info.filename for info in zf.infolist() if not info.is_dir()
            )

        assert entries == [
            "codelens-mcp.exe",
            "models/codesearch/config.json",
            "models/codesearch/model-manifest.json",
            "models/codesearch/model.onnx",
            "models/codesearch/special_tokens_map.json",
            "models/codesearch/tokenizer.json",
            "models/codesearch/tokenizer_config.json",
        ]


def test_homebrew_formula_installs_model_sidecar() -> None:
    formula = FORMULA.read_text(encoding="utf-8")

    assert 'bin.install "codelens-mcp"' in formula
    assert 'prefix.install "models" if File.directory?("models")' in formula


def main() -> int:
    failures: list[str] = []
    for name, fn in [
        (
            "package_script_emits_standard_payload_only",
            test_package_script_emits_standard_payload_only,
        ),
        (
            "release_verifier_rejects_standard_archive_adapter_payload",
            test_release_verifier_rejects_standard_archive_adapter_payload,
        ),
        (
            "package_script_emits_standard_windows_zip_payload_only",
            test_package_script_emits_standard_windows_zip_payload_only,
        ),
        (
            "homebrew_formula_installs_model_sidecar",
            test_homebrew_formula_installs_model_sidecar,
        ),
    ]:
        try:
            fn()
            print(f"PASS  {name}")
        except AssertionError as exc:
            print(f"FAIL  {name}: {exc}")
            failures.append(name)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
