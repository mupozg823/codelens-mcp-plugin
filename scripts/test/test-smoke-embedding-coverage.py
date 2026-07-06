#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-smoke-embedding-coverage.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-smoke-embedding-coverage.py
# ------------------

"""Contract tests for scripts/smoke-embedding-coverage.py."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "smoke_embedding_coverage", REPO_ROOT / "scripts" / "smoke-embedding-coverage.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

CoverageReportError = MODULE.CoverageReportError
extract_report = MODULE.extract_report
validate_report = MODULE.validate_report


def valid_data() -> dict[str, object]:
    return {
        "compiled": True,
        "status": "stale",
        "model_assets": {
            "available": True,
            "configured_model": "MiniLM-L12-CodeSearchNet-INT8",
            "sha256": "a" * 64,
        },
        "index": {
            "model": "MiniLM-L12-CodeSearchNet-INT8",
            "expected_model": "MiniLM-L12-CodeSearchNet-INT8",
            "model_mismatch": False,
            "schema_version": 2,
            "expected_schema_version": 2,
            "schema_mismatch": False,
            "indexed_symbols": 42,
            "indexed_files": 3,
            "checked_files": 3,
            "ready_files": 2,
            "readiness_percent": 66,
            "unchanged_files": 2,
            "stale_files": 1,
            "missing_files": 0,
            "extra_files": 0,
            "skipped_new_files": 0,
            "stale_file_reasons": [
                {
                    "file_path": "src/main.rs",
                    "reason": "embedding_keys_changed",
                }
            ],
            "stale_file_reasons_omitted": 0,
            "current_git_sha": "abc123",
            "last_index_sha": None,
            "last_index_sha_source": "unavailable",
            "freshness": {
                "schema": {
                    "status": "ready",
                    "indexed_version": 2,
                    "expected_version": 2,
                    "recommended_action": "none",
                },
                "model": {
                    "status": "ready",
                    "indexed_model": "MiniLM-L12-CodeSearchNet-INT8",
                    "expected_model": "MiniLM-L12-CodeSearchNet-INT8",
                    "recommended_action": "none",
                },
                "git": {
                    "status": "unknown",
                    "current_git_sha": "abc123",
                    "last_index_sha": None,
                    "recommended_action": "inspect_embedding_runtime",
                },
                "files": {
                    "status": "stale",
                    "checked_files": 3,
                    "ready_files": 2,
                    "readiness_percent": 66,
                    "stale_files": 1,
                    "missing_files": 0,
                    "extra_files": 0,
                    "recommended_action": "refresh_embedding_index",
                },
            },
        },
        "query_cache": {
            "enabled": True,
            "entries": 4,
            "max_entries": 128,
        },
        "recommended_action": "refresh_embedding_index",
        "remediation": {
            "reason": "stale",
            "action": "refresh_embedding_index",
            "description": "refresh embeddings for changed, missing, or orphaned files",
        },
    }


def assert_rejects(payload: dict[str, object], expected: str) -> None:
    try:
        validate_report(payload)
    except CoverageReportError as error:
        assert expected in str(error), str(error)
        return
    raise AssertionError(f"expected rejection containing {expected!r}")


def test_extract_report_accepts_oneshot_envelope() -> None:
    envelope = {"success": True, "data": valid_data()}
    summary = validate_report(extract_report(envelope))

    assert summary.status == "stale"
    assert summary.compiled is True
    assert summary.model_assets_available is True
    assert summary.model_sha256 == "a" * 64
    assert summary.indexed_symbols == 42
    assert summary.readiness_percent == 66
    assert summary.stale_files == 1
    assert summary.stale_reason == "src/main.rs:embedding_keys_changed"
    assert summary.model_mismatch is False
    assert summary.remediation_action == "refresh_embedding_index"
    assert summary.query_cache_entries == 4
    assert "readiness_percent=66%" in summary.render()
    assert "stale_reason=src/main.rs:embedding_keys_changed" in summary.render()
    assert "remediation.action=refresh_embedding_index" in summary.render()
    assert "last_index_sha=null" in summary.render()
    assert "model_assets.sha256=aaaaaaaaaaaa" in summary.render()


def test_validate_report_rejects_semantic_feature_off() -> None:
    payload = valid_data()
    payload["compiled"] = False

    assert_rejects(payload, "data.compiled must be True")


def test_validate_report_rejects_missing_model_assets() -> None:
    payload = valid_data()
    payload["model_assets"] = {"available": False}

    assert_rejects(payload, "data.model_assets.available must be True")


def test_validate_report_rejects_model_mismatch() -> None:
    payload = valid_data()
    index = dict(payload["index"])
    index["model_mismatch"] = True
    payload["index"] = index

    assert_rejects(payload, "data.index.model_mismatch must be False")


def test_validate_report_requires_operational_fields() -> None:
    payload = valid_data()
    index = dict(payload["index"])
    del index["last_index_sha"]
    payload["index"] = index

    assert_rejects(payload, "data.index.last_index_sha is missing")


def test_validate_report_requires_readiness_percent() -> None:
    payload = valid_data()
    index = dict(payload["index"])
    del index["readiness_percent"]
    payload["index"] = index

    assert_rejects(payload, "data.index.readiness_percent is missing")


def test_validate_report_requires_stale_reasons() -> None:
    payload = valid_data()
    index = dict(payload["index"])
    del index["stale_file_reasons"]
    payload["index"] = index

    assert_rejects(payload, "data.index.stale_file_reasons is missing")


def test_validate_report_requires_freshness_taxonomy() -> None:
    payload = valid_data()
    index = dict(payload["index"])
    del index["freshness"]
    payload["index"] = index

    assert_rejects(payload, "data.index.freshness is missing")


def test_validate_report_requires_remediation() -> None:
    payload = valid_data()
    del payload["remediation"]

    assert_rejects(payload, "data.remediation is missing")


if __name__ == "__main__":
    test_extract_report_accepts_oneshot_envelope()
    test_validate_report_rejects_semantic_feature_off()
    test_validate_report_rejects_missing_model_assets()
    test_validate_report_rejects_model_mismatch()
    test_validate_report_requires_operational_fields()
    test_validate_report_requires_readiness_percent()
    test_validate_report_requires_stale_reasons()
    test_validate_report_requires_freshness_taxonomy()
    test_validate_report_requires_remediation()
