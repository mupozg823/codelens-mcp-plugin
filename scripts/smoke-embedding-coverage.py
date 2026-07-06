#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/smoke-embedding-coverage.py --binary target/release/codelens-mcp --project .
# 3. CI can also run it with system Python:
#      python3 scripts/smoke-embedding-coverage.py --binary target/release/codelens-mcp --project .
# ------------------

"""Fail-closed smoke for the embedding_coverage_report operational surface."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from types import NoneType
from typing import Final


JsonValue = (
    NoneType | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
DEFAULT_TIMEOUT_SECONDS: Final = 120


class CoverageReportError(RuntimeError):
    """Raised when embedding_coverage_report is missing required evidence."""


@dataclass(frozen=True, slots=True)
class CoverageSummary:
    """Operator-facing coverage fields printed by the smoke gate."""

    status: str
    compiled: bool
    model_assets_available: bool
    indexed_symbols: int
    readiness_percent: int
    stale_files: int
    stale_reason: str | None
    model_mismatch: bool
    remediation_action: str
    query_cache_entries: int
    last_index_sha: str | None

    def render(self) -> str:
        """Render the compact line CI logs need."""
        last_sha = self.last_index_sha if self.last_index_sha else "null"
        return (
            "embedding_coverage_report: "
            f"status={self.status} "
            f"compiled={self.compiled} "
            f"model_assets.available={self.model_assets_available} "
            f"indexed_symbols={self.indexed_symbols} "
            f"readiness_percent={self.readiness_percent}% "
            f"stale_files={self.stale_files} "
            f"stale_reason={self.stale_reason or 'none'} "
            f"model_mismatch={self.model_mismatch} "
            f"remediation.action={self.remediation_action} "
            f"query_cache.entries={self.query_cache_entries} "
            f"last_index_sha={last_sha}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="Path to the codelens-mcp binary")
    parser.add_argument(
        "--project",
        default=".",
        help="Project root used for the one-shot embedding_coverage_report call",
    )
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    return parser.parse_args()


def run_report(binary: Path, project: Path, timeout: int) -> JsonValue:
    env = os.environ.copy()
    env.setdefault("CODELENS_LOG", "warn")
    try:
        completed = subprocess.run(
            [str(binary), str(project), "--cmd", "embedding_coverage_report"],
            cwd=project,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise CoverageReportError(
            f"embedding_coverage_report timed out after {timeout}s"
        ) from error

    if completed.returncode != 0:
        stderr_tail = completed.stderr.strip()[-4000:]
        raise CoverageReportError(
            "embedding_coverage_report command failed "
            f"(exit={completed.returncode}): {stderr_tail}"
        )
    stdout = completed.stdout.strip()
    if not stdout:
        raise CoverageReportError("embedding_coverage_report printed no JSON")
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as error:
        raise CoverageReportError(
            f"embedding_coverage_report printed invalid JSON: {error}"
        ) from error


def as_object(value: JsonValue, label: str) -> Mapping[str, JsonValue]:
    match value:
        case dict() as mapping:
            return mapping
        case _:
            raise CoverageReportError(f"{label} must be an object")


def extract_report(payload: JsonValue) -> Mapping[str, JsonValue]:
    root = as_object(payload, "root")
    if root.get("success") is False:
        raise CoverageReportError("one-shot tool response reported success=false")
    data = root.get("data")
    if data is not None:
        return as_object(data, "root.data")
    return root


def require_key(mapping: Mapping[str, JsonValue], key: str, label: str) -> JsonValue:
    if key not in mapping:
        raise CoverageReportError(f"{label}.{key} is missing")
    return mapping[key]


def require_bool(
    mapping: Mapping[str, JsonValue],
    key: str,
    label: str,
    *,
    expected: bool | None = None,
) -> bool:
    value = require_key(mapping, key, label)
    match value:
        case bool() as flag:
            if expected is not None and flag is not expected:
                raise CoverageReportError(f"{label}.{key} must be {expected}")
            return flag
        case _:
            raise CoverageReportError(f"{label}.{key} must be a boolean")


def require_int(
    mapping: Mapping[str, JsonValue],
    key: str,
    label: str,
    *,
    minimum: int,
    maximum: int | None = None,
) -> int:
    value = require_key(mapping, key, label)
    match value:
        case int() as number if (
            not isinstance(number, bool)
            and number >= minimum
            and (maximum is None or number <= maximum)
        ):
            return number
        case _:
            maximum_hint = "" if maximum is None else f" and <= {maximum}"
            raise CoverageReportError(
                f"{label}.{key} must be an int >= {minimum}{maximum_hint}"
            )


def require_str(mapping: Mapping[str, JsonValue], key: str, label: str) -> str:
    value = require_key(mapping, key, label)
    match value:
        case str() as text if text:
            return text
        case _:
            raise CoverageReportError(f"{label}.{key} must be a non-empty string")


def require_optional_str(
    mapping: Mapping[str, JsonValue], key: str, label: str
) -> str | None:
    value = require_key(mapping, key, label)
    match value:
        case None:
            return None
        case str() as text:
            return text
        case _:
            raise CoverageReportError(f"{label}.{key} must be string or null")


def require_list(
    mapping: Mapping[str, JsonValue], key: str, label: str
) -> list[JsonValue]:
    value = require_key(mapping, key, label)
    match value:
        case list() as items:
            return items
        case _:
            raise CoverageReportError(f"{label}.{key} must be an array")


def first_stale_reason(reasons: list[JsonValue]) -> str | None:
    if not reasons:
        return None
    first = as_object(reasons[0], "data.index.stale_file_reasons[0]")
    file_path = require_str(first, "file_path", "data.index.stale_file_reasons[0]")
    reason = require_str(first, "reason", "data.index.stale_file_reasons[0]")
    return f"{file_path}:{reason}"


def validate_report(report: Mapping[str, JsonValue]) -> CoverageSummary:
    compiled = require_bool(report, "compiled", "data", expected=True)
    status = require_str(report, "status", "data")

    model_assets = as_object(require_key(report, "model_assets", "data"), "data.model_assets")
    assets_available = require_bool(
        model_assets, "available", "data.model_assets", expected=True
    )

    index = as_object(require_key(report, "index", "data"), "data.index")
    _schema_version = require_int(index, "schema_version", "data.index", minimum=0)
    _expected_schema_version = require_int(
        index, "expected_schema_version", "data.index", minimum=0
    )
    indexed_symbols = require_int(index, "indexed_symbols", "data.index", minimum=0)
    readiness_percent = require_int(
        index, "readiness_percent", "data.index", minimum=0, maximum=100
    )
    stale_files = require_int(index, "stale_files", "data.index", minimum=0)
    stale_reasons = require_list(index, "stale_file_reasons", "data.index")
    _stale_reasons_omitted = require_int(
        index, "stale_file_reasons_omitted", "data.index", minimum=0
    )
    model_mismatch = require_bool(
        index, "model_mismatch", "data.index", expected=False
    )
    last_index_sha = require_optional_str(index, "last_index_sha", "data.index")
    freshness = as_object(require_key(index, "freshness", "data.index"), "data.index.freshness")
    for dimension in ("schema", "model", "git", "files"):
        as_object(
            require_key(freshness, dimension, "data.index.freshness"),
            f"data.index.freshness.{dimension}",
        )

    query_cache = as_object(
        require_key(report, "query_cache", "data"), "data.query_cache"
    )
    query_cache_entries = require_int(
        query_cache, "entries", "data.query_cache", minimum=0
    )
    remediation = as_object(
        require_key(report, "remediation", "data"), "data.remediation"
    )
    remediation_action = require_str(remediation, "action", "data.remediation")

    return CoverageSummary(
        status=status,
        compiled=compiled,
        model_assets_available=assets_available,
        indexed_symbols=indexed_symbols,
        readiness_percent=readiness_percent,
        stale_files=stale_files,
        stale_reason=first_stale_reason(stale_reasons),
        model_mismatch=model_mismatch,
        remediation_action=remediation_action,
        query_cache_entries=query_cache_entries,
        last_index_sha=last_index_sha,
    )


def main() -> None:
    args = parse_args()
    binary = Path(args.binary).expanduser().resolve()
    project = Path(args.project).expanduser().resolve()
    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}")
    if not project.is_dir():
        raise SystemExit(f"project directory not found: {project}")

    try:
        report = extract_report(run_report(binary, project, args.timeout))
        summary = validate_report(report)
    except CoverageReportError as error:
        raise SystemExit(str(error)) from error

    print(summary.render())


if __name__ == "__main__":
    main()
