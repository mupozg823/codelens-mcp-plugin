#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-smoke-clean-quickstart.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-smoke-clean-quickstart.py
# ------------------

from __future__ import annotations

import sys
import shutil
from collections.abc import Callable
from pathlib import Path
from types import NoneType
from typing import TypeAlias


JsonValue: TypeAlias = (
    NoneType | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from quickstart_smoke_contract import (  # noqa: E402
    CoverageSmoke,
    QuickstartSmokeError,
    RetrievalSmoke,
    parse_json_stdout,
    validate_capabilities,
    validate_coverage,
    validate_retrieval,
    validate_status,
)
from quickstart_smoke_archive import find_archive_binary  # noqa: E402
from quickstart_smoke_runner import (  # noqa: E402
    build_smoke_env,
    install_homebrew_layout,
)


def status_payload() -> dict[str, JsonValue]:
    return {
        "command": "status",
        "strict_semantic_coverage": False,
        "hosts": [
            {
                "host": "codex",
                "files": [
                    {
                        "path": "/tmp/home/.codex/config.toml",
                        "format": "toml",
                        "status": "attached_customized",
                    }
                ],
            }
        ],
    }


def coverage_payload() -> dict[str, JsonValue]:
    return {
        "success": True,
        "data": {
            "compiled": True,
            "status": "ready",
            "model_assets": {"available": True},
            "index": {
                "model_mismatch": False,
                "stale_files": 0,
                "indexed_symbols": 2,
                "indexed_files": 2,
                "readiness_percent": 100,
            },
            "query_cache": {"entries": 0},
        },
    }


def retrieval_payload() -> dict[str, JsonValue]:
    return {
        "success": True,
        "token_estimate": 714,
        "data": {
            "retrieval": {
                "semantic_used_in_core": True,
                "sparse_used_in_core": True,
                "query_type": "natural_language",
            },
            "symbols": [
                {
                    "name": "add_values",
                    "file": "src/lib.rs",
                }
            ],
        },
    }


def assert_rejects(
    fn_name: str,
    fn: Callable[[dict[str, JsonValue]], None | CoverageSmoke | RetrievalSmoke],
    payload: dict[str, JsonValue],
    expected: str,
) -> None:
    try:
        fn(payload)
    except QuickstartSmokeError as error:
        assert expected in str(error), str(error)
        return
    raise AssertionError(f"{fn_name} should reject payload containing {expected!r}")


def test_parse_json_stdout_skips_banner() -> None:
    parsed = parse_json_stdout("codelens-mcp: banner\n{\"success\": true}\n", "tool")

    assert parsed == {"success": True}


def test_validate_status_accepts_attached_codex_config() -> None:
    validate_status(status_payload())


def test_validate_capabilities_requires_pre_index_state() -> None:
    validate_capabilities(
        {
            "success": True,
            "data": {
                "semantic_search_status": "index_missing",
                "embedding_indexed": False,
                "supported_files": 2,
            },
        }
    )


def test_validate_coverage_keeps_operational_fields() -> None:
    summary = validate_coverage(coverage_payload())

    assert summary.status == "ready"
    assert summary.indexed_symbols == 2
    assert summary.indexed_files == 2
    assert summary.readiness_percent == 100
    assert summary.query_cache_entries == 0


def test_validate_retrieval_requires_fixture_symbol() -> None:
    summary = validate_retrieval(retrieval_payload())

    assert summary.top_symbol == "add_values"
    assert summary.top_file == "src/lib.rs"
    assert summary.token_estimate == 714


def test_validate_retrieval_rejects_sparse_only_path() -> None:
    payload = retrieval_payload()
    data = dict(payload["data"])
    retrieval = dict(data["retrieval"])
    retrieval["semantic_used_in_core"] = False
    data["retrieval"] = retrieval
    payload["data"] = data

    assert_rejects(
        "validate_retrieval",
        validate_retrieval,
        payload,
        "retrieval.semantic_used_in_core must be true",
    )


def test_validate_coverage_rejects_stale_files() -> None:
    payload = coverage_payload()
    data = dict(payload["data"])
    index = dict(data["index"])
    index["stale_files"] = 1
    data["index"] = index
    payload["data"] = data

    assert_rejects(
        "validate_coverage",
        validate_coverage,
        payload,
        "coverage.data.index.stale_files must be 0",
    )


def test_build_smoke_env_omits_model_env_by_default() -> None:
    env = build_smoke_env(Path("/tmp/home"), Path("/tmp/prefix/models"), use_model_env=False)

    assert env["HOME"] == "/tmp/home"
    assert env["CODELENS_LOG"] == "error"
    assert "CODELENS_MODEL_DIR" not in env


def test_build_smoke_env_can_force_model_env() -> None:
    env = build_smoke_env(Path("/tmp/home"), Path("/tmp/prefix/models"), use_model_env=True)

    assert env["CODELENS_MODEL_DIR"] == "/tmp/prefix/models"


def test_find_archive_binary_accepts_release_root() -> None:
    root = REPO_ROOT / "target" / "quickstart-smoke-test"
    binary = root / "codelens-mcp"
    binary.parent.mkdir(parents=True, exist_ok=True)
    binary.write_text("#!/bin/sh\n", encoding="utf-8")

    assert find_archive_binary(root) == binary
    binary.unlink()


def test_homebrew_layout_uses_prefix_model_root() -> None:
    root = REPO_ROOT / "target" / "quickstart-smoke-homebrew-test"
    shutil.rmtree(root, ignore_errors=True)
    source = root / "source" / "codelens-mcp"
    model_root = root / "model-root" / "codesearch"
    source.parent.mkdir(parents=True, exist_ok=True)
    model_root.mkdir(parents=True, exist_ok=True)
    source.write_text("#!/bin/sh\n", encoding="utf-8")
    for asset in (
        "model.onnx",
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ):
        model_root.joinpath(asset).write_text(asset, encoding="utf-8")

    binary, model_env_root = install_homebrew_layout(source, root / "model-root", root / "install")

    assert binary.as_posix().endswith("/Cellar/codelens-mcp/0.0.0/bin/codelens-mcp")
    assert model_env_root.as_posix().endswith("/Cellar/codelens-mcp/0.0.0/models")


if __name__ == "__main__":
    test_parse_json_stdout_skips_banner()
    test_validate_status_accepts_attached_codex_config()
    test_validate_capabilities_requires_pre_index_state()
    test_validate_coverage_keeps_operational_fields()
    test_validate_retrieval_requires_fixture_symbol()
    test_validate_retrieval_rejects_sparse_only_path()
    test_validate_coverage_rejects_stale_files()
    test_build_smoke_env_omits_model_env_by_default()
    test_build_smoke_env_can_force_model_env()
    test_find_archive_binary_accepts_release_root()
    test_homebrew_layout_uses_prefix_model_root()
