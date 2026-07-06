#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-embedding-index-lifecycle.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-embedding-index-lifecycle.py
# ------------------

"""Contract tests for benchmarks/embedding-index-lifecycle.py."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path
from types import NoneType


JsonValue = (
    NoneType | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
REPO_ROOT = Path(__file__).resolve().parents[2]
BENCHMARKS_DIR = REPO_ROOT / "benchmarks"
sys.path.insert(0, str(BENCHMARKS_DIR))
SPEC = importlib.util.spec_from_file_location(
    "embedding_index_lifecycle_lib",
    BENCHMARKS_DIR / "embedding_index_lifecycle_lib.py",
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

ARTIFACT_SCHEMA_VERSION = MODULE.ARTIFACT_SCHEMA_VERSION
ToolRun = MODULE.ToolRun
compact_coverage = MODULE.compact_coverage
default_output_path = MODULE.default_output_path
delta = MODULE.delta
initialize_git_snapshot = MODULE.initialize_git_snapshot
lifecycle_step = MODULE.lifecycle_step
parse_output_json = MODULE.parse_output_json
tool_data = MODULE.tool_data


def index_envelope(elapsed_ms: int) -> ToolRun:
    return ToolRun(
        name="index_embeddings",
        elapsed_ms=elapsed_ms,
        returncode=0,
        payload={
            "success": True,
            "data": {
                "indexed_symbols": 42,
                "bridges_generated": 2,
                "query_cache": {
                    "enabled": True,
                    "entries": 4,
                    "max_entries": 128,
                    "prewarmed": 0,
                },
                "status": "ok",
            },
        },
        stderr_tail="",
    )


def coverage_envelope(elapsed_ms: int) -> ToolRun:
    return ToolRun(
        name="embedding_coverage_report",
        elapsed_ms=elapsed_ms,
        returncode=0,
        payload={"success": True, "data": coverage_data()},
        stderr_tail="",
    )


def coverage_data() -> dict[str, JsonValue]:
    return {
        "status": "ready",
        "compiled": True,
        "model_assets": {
            "available": True,
            "configured_model": "MiniLM-L12-CodeSearchNet-INT8",
            "sha256": "b" * 64,
        },
        "index": {
            "schema_version": 3,
            "expected_schema_version": 3,
            "schema_mismatch": False,
            "indexed_symbols": 42,
            "indexed_files": 3,
            "readiness_percent": 100,
            "stale_files": 0,
            "model_mismatch": False,
            "current_git_sha": "abc123",
            "last_index_sha": "abc123",
            "last_index_sha_source": "persisted",
            "stale_file_reasons": [
                {
                    "file_path": "src/main.rs",
                    "reason": "embedding_keys_changed",
                }
            ],
            "stale_file_reasons_omitted": 2,
            "freshness": {
                "schema": {
                    "status": "ready",
                    "recommended_action": "none",
                },
                "model": {
                    "status": "ready",
                    "recommended_action": "none",
                },
                "git": {
                    "status": "ready",
                    "recommended_action": "none",
                },
                "files": {
                    "status": "ready",
                    "recommended_action": "none",
                },
            },
        },
        "query_cache": {"entries": 4},
        "remediation": {"action": "none"},
    }


def test_default_output_path_is_tmp_json() -> None:
    path = default_output_path("20260706T000000Z")

    assert path.name == "codelens-index-lifecycle-20260706T000000Z.json"
    assert path.suffix == ".json"
    assert path.parent.is_absolute()


def test_parse_output_json_accepts_log_prefixed_payload() -> None:
    payload = parse_output_json('WARN starting\n{"success": true, "data": {"status": "ok"}}')

    assert tool_data(payload)["status"] == "ok"


def test_compact_coverage_keeps_lifecycle_fields() -> None:
    summary = compact_coverage(coverage_data())

    assert summary["status"] == "ready"
    assert summary["schema_version"] == 3
    assert summary["model_sha256"] == "b" * 64
    assert summary["schema_mismatch"] is False
    assert summary["indexed_symbols"] == 42
    assert summary["readiness_percent"] == 100
    assert summary["stale_files"] == 0
    assert summary["current_git_sha"] == "abc123"
    assert summary["last_index_sha_source"] == "persisted"
    assert summary["stale_file_reasons_omitted"] == 2
    assert summary["stale_file_reasons"] == [
        {
            "file_path": "src/main.rs",
            "reason": "embedding_keys_changed",
        }
    ]
    assert summary["freshness"] == {
        "schema": {
            "status": "ready",
            "recommended_action": "none",
        },
        "model": {
            "status": "ready",
            "recommended_action": "none",
        },
        "git": {
            "status": "ready",
            "recommended_action": "none",
        },
        "files": {
            "status": "ready",
            "recommended_action": "none",
        },
    }
    assert summary["remediation_action"] == "none"


def test_lifecycle_step_preserves_index_and_coverage_contract() -> None:
    step = lifecycle_step(index_envelope(1000), coverage_envelope(25))
    index = step["index_embeddings"]
    coverage = step["coverage"]

    assert isinstance(index, dict)
    assert isinstance(coverage, dict)
    assert index["elapsed_ms"] == 1000
    assert index["indexed_symbols"] == 42
    assert coverage["query_cache_entries"] == 4


def test_delta_reports_warm_ratio_and_savings() -> None:
    cold = lifecycle_step(index_envelope(1000), coverage_envelope(25))
    warm = lifecycle_step(index_envelope(250), coverage_envelope(20))

    summary = delta(cold, warm)

    assert ARTIFACT_SCHEMA_VERSION == "codelens-index-lifecycle-benchmark-v1"
    assert summary["warm_saved_ms"] == 750
    assert summary["warm_to_cold_ratio"] == 0.25


def test_initialize_git_snapshot_creates_head_for_lifecycle_freshness() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "main.py").write_text("def hello():\n    return 1\n")

        mode = initialize_git_snapshot(root)

        head = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--verify", "HEAD"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        assert mode == "isolated_git_snapshot"
        assert head.returncode == 0


if __name__ == "__main__":
    test_default_output_path_is_tmp_json()
    test_parse_output_json_accepts_log_prefixed_payload()
    test_compact_coverage_keeps_lifecycle_fields()
    test_lifecycle_step_preserves_index_and_coverage_contract()
    test_delta_reports_warm_ratio_and_savings()
    test_initialize_git_snapshot_creates_head_for_lifecycle_freshness()
