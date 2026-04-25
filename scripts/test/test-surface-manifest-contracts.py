#!/usr/bin/env python3
"""Tests for surface-manifest.py contract A and B (Phase 0 mutation surface truthing)."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SURFACE_MANIFEST = REPO_ROOT / "scripts" / "surface-manifest.py"

VALID_MATRIX = {
    "schema": "codelens-semantic-operation-matrix-v1",
    "tier1_languages": ["rust", "typescript", "javascript", "java"],
    "operations": [
        {
            "operation": "rename",
            "backend": "tree-sitter",
            "languages": ["rust"],
            "support": "syntax_preview",
            "authority": "syntax",
            "can_preview": True,
            "can_apply": False,
            "verified": True,
            "blocker_reason": "tree-sitter rename is preview-only",
            "required_methods": [],
            "failure_policy": "fail_closed",
        },
        {
            "operation": "rename",
            "backend": "lsp",
            "languages": ["rust", "typescript", "javascript", "java"],
            "support": "authoritative_apply",
            "authority": "workspace_edit",
            "can_preview": True,
            "can_apply": True,
            "verified": True,
            "blocker_reason": None,
            "required_methods": ["textDocument/rename"],
            "failure_policy": "fail_closed",
        },
    ],
}


def run_contract_check(matrix: dict) -> subprocess.CompletedProcess:
    """Run surface-manifest.py --check-operation-matrix against a temp matrix file."""
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, encoding="utf-8"
    ) as f:
        json.dump(matrix, f)
        matrix_path = f.name
    try:
        return subprocess.run(
            [
                sys.executable,
                str(SURFACE_MANIFEST),
                "--check-operation-matrix",
                matrix_path,
            ],
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        Path(matrix_path).unlink(missing_ok=True)


def test_valid_matrix_passes() -> None:
    proc = run_contract_check(VALID_MATRIX)
    assert (
        proc.returncode == 0
    ), f"valid matrix should pass: stdout={proc.stdout} stderr={proc.stderr}"


def test_contract_a_verified_false_can_apply_true_rejected() -> None:
    bad = json.loads(json.dumps(VALID_MATRIX))
    bad["operations"].append(
        {
            "operation": "extract_function",
            "backend": "lsp",
            "languages": ["rust"],
            "support": "conditional_authoritative_apply",
            "authority": "workspace_edit",
            "can_preview": True,
            "can_apply": True,  # contract A 위반
            "verified": False,  # ↑
            "blocker_reason": "fixture coverage missing",
            "required_methods": [],
            "failure_policy": "fail_closed",
        }
    )
    proc = run_contract_check(bad)
    assert proc.returncode == 1, (
        f"expected exit 1 for contract A violation, got {proc.returncode}: "
        f"stdout={proc.stdout} stderr={proc.stderr}"
    )
    assert "extract_function" in proc.stderr or "extract_function" in proc.stdout, (
        f"expected violation to enumerate extract_function: "
        f"stdout={proc.stdout} stderr={proc.stderr}"
    )


def main() -> int:
    failures: list[str] = []
    for name, fn in [
        ("valid_matrix_passes", test_valid_matrix_passes),
        (
            "contract_a_violation_rejected",
            test_contract_a_verified_false_can_apply_true_rejected,
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
