"""Cold/warm semantic index lifecycle benchmark support."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import time
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from types import NoneType
from typing import Final

from embedding_index_lifecycle_worktree import (
    initialize_git_snapshot,
    isolated_project_copy,
)


JsonValue = (
    NoneType | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
DEFAULT_TIMEOUT_SECONDS: Final = 1800
ARTIFACT_SCHEMA_VERSION: Final = "codelens-index-lifecycle-benchmark-v1"


class IndexLifecycleError(RuntimeError):
    """Raised when the lifecycle benchmark cannot produce trustworthy evidence."""


@dataclass(frozen=True, slots=True)
class CliArgs:
    project: Path
    binary: Path
    output: Path
    timeout: int
    keep_worktree: bool


@dataclass(frozen=True, slots=True)
class ToolRun:
    name: str
    elapsed_ms: int
    returncode: int
    payload: JsonValue
    stderr_tail: str


@dataclass(frozen=True, slots=True)
class BenchmarkSummary:
    output: Path
    cold_ms: JsonValue
    warm_ms: JsonValue
    worktree: Path | None


def utc_stamp() -> str:
    return datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")


def default_output_path(stamp: str | None = None) -> Path:
    suffix = stamp if stamp is not None else utc_stamp()
    return Path(tempfile.gettempdir()) / f"codelens-index-lifecycle-{suffix}.json"


def parse_output_json(output: str) -> JsonValue:
    text = output.strip()
    if not text:
        raise IndexLifecycleError("tool printed no JSON")
    decoder = json.JSONDecoder()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    for line in reversed(text.splitlines()):
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    for index, char in enumerate(text):
        if char not in "{[":
            continue
        try:
            payload, _end = decoder.raw_decode(text, index)
            return payload
        except json.JSONDecodeError:
            continue
    raise IndexLifecycleError("tool printed invalid JSON")


def as_mapping(value: JsonValue, label: str) -> dict[str, JsonValue]:
    match value:
        case dict() as mapping:
            return mapping
        case _:
            raise IndexLifecycleError(f"{label} must be an object")


def tool_data(payload: JsonValue) -> dict[str, JsonValue]:
    root = as_mapping(payload, "tool payload")
    if root.get("success") is False:
        raise IndexLifecycleError("tool returned success=false")
    data = root.get("data")
    if data is None:
        return root
    return as_mapping(data, "tool payload.data")


def default_model_dir(project: Path) -> Path | None:
    candidate = project / "crates" / "codelens-engine" / "models"
    return candidate if candidate.is_dir() else None


def run_tool(
    binary: Path, project: Path, model_source: Path, name: str, timeout: int
) -> ToolRun:
    env = os.environ.copy()
    env.setdefault("CODELENS_LOG", "warn")
    model_dir = default_model_dir(model_source)
    if model_dir is not None:
        env.setdefault("CODELENS_MODEL_DIR", str(model_dir))
    started = time.perf_counter()
    try:
        result = subprocess.run(
            [str(binary), str(project), "--cmd", name],
            cwd=project,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise IndexLifecycleError(f"{name} timed out after {timeout}s") from error
    elapsed_ms = int(round((time.perf_counter() - started) * 1000))
    if result.returncode != 0:
        raise IndexLifecycleError(
            f"{name} failed exit={result.returncode}: {result.stderr.strip()[-4000:]}"
        )
    return ToolRun(
        name=name,
        elapsed_ms=elapsed_ms,
        returncode=result.returncode,
        payload=parse_output_json(result.stdout),
        stderr_tail=result.stderr.strip()[-4000:],
    )


def compact_coverage(report: dict[str, JsonValue]) -> dict[str, JsonValue]:
    model_assets = as_mapping(report.get("model_assets"), "coverage.model_assets")
    index = as_mapping(report.get("index"), "coverage.index")
    query_cache = as_mapping(report.get("query_cache"), "coverage.query_cache")
    remediation = as_mapping(report.get("remediation"), "coverage.remediation")
    return {
        "status": report.get("status"),
        "compiled": report.get("compiled"),
        "model_sha256": model_assets.get("sha256"),
        "schema_version": index.get("schema_version"),
        "expected_schema_version": index.get("expected_schema_version"),
        "schema_mismatch": index.get("schema_mismatch"),
        "indexed_symbols": index.get("indexed_symbols"),
        "indexed_files": index.get("indexed_files"),
        "readiness_percent": index.get("readiness_percent"),
        "stale_files": index.get("stale_files"),
        "model_mismatch": index.get("model_mismatch"),
        "current_git_sha": index.get("current_git_sha"),
        "last_index_sha": index.get("last_index_sha"),
        "last_index_sha_source": index.get("last_index_sha_source"),
        "stale_file_reasons": index.get("stale_file_reasons", []),
        "stale_file_reasons_omitted": index.get("stale_file_reasons_omitted"),
        "freshness": as_mapping(index.get("freshness"), "coverage.index.freshness"),
        "query_cache_entries": query_cache.get("entries"),
        "remediation_action": remediation.get("action"),
    }


def lifecycle_step(index_run: ToolRun, coverage_run: ToolRun) -> dict[str, JsonValue]:
    index_data = tool_data(index_run.payload)
    coverage_data = tool_data(coverage_run.payload)
    return {
        "index_embeddings": {
            "elapsed_ms": index_run.elapsed_ms,
            "indexed_symbols": index_data.get("indexed_symbols"),
            "bridges_generated": index_data.get("bridges_generated"),
            "query_cache": index_data.get("query_cache"),
        },
        "coverage": compact_coverage(coverage_data),
    }


def delta(cold: dict[str, JsonValue], warm: dict[str, JsonValue]) -> dict[str, JsonValue]:
    cold_index = as_mapping(cold["index_embeddings"], "cold.index_embeddings")
    warm_index = as_mapping(warm["index_embeddings"], "warm.index_embeddings")
    cold_ms = cold_index.get("elapsed_ms")
    warm_ms = warm_index.get("elapsed_ms")
    if not isinstance(cold_ms, int) or not isinstance(warm_ms, int) or cold_ms <= 0:
        return {"warm_saved_ms": None, "warm_to_cold_ratio": None}
    return {
        "warm_saved_ms": cold_ms - warm_ms,
        "warm_to_cold_ratio": round(warm_ms / cold_ms, 4),
    }


def build_artifact(
    args: CliArgs, worktree: Path, worktree_mode: str
) -> dict[str, JsonValue]:
    started_at = utc_stamp()
    cold_index = run_tool(
        args.binary, worktree, args.project, "index_embeddings", args.timeout
    )
    cold_coverage = run_tool(
        args.binary,
        worktree,
        args.project,
        "embedding_coverage_report",
        args.timeout,
    )
    warm_index = run_tool(
        args.binary, worktree, args.project, "index_embeddings", args.timeout
    )
    warm_coverage = run_tool(
        args.binary,
        worktree,
        args.project,
        "embedding_coverage_report",
        args.timeout,
    )
    cold = lifecycle_step(cold_index, cold_coverage)
    warm = lifecycle_step(warm_index, warm_coverage)
    return {
        "schema_version": ARTIFACT_SCHEMA_VERSION,
        "started_at": started_at,
        "finished_at": utc_stamp(),
        "project": str(args.project),
        "benchmark_project": str(worktree),
        "binary": str(args.binary),
        "worktree_mode": worktree_mode,
        "artifact_policy": "default output is under /tmp; copy excludes runtime state",
        "cold": cold,
        "warm": warm,
        "delta": delta(cold, warm),
    }


def write_artifact(path: Path, artifact: dict[str, JsonValue]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(artifact, indent=2, sort_keys=True) + "\n")


def index_elapsed_ms(step: JsonValue, label: str) -> JsonValue:
    index = as_mapping(as_mapping(step, label)["index_embeddings"], f"{label}.index_embeddings")
    return index["elapsed_ms"]


def run_benchmark(args: CliArgs) -> BenchmarkSummary:
    tmpdir, worktree = isolated_project_copy(args.project)
    try:
        worktree_mode = initialize_git_snapshot(worktree)
        artifact = build_artifact(args, worktree, worktree_mode)
        write_artifact(args.output, artifact)
        kept_worktree = worktree if args.keep_worktree else None
        return BenchmarkSummary(
            output=args.output,
            cold_ms=index_elapsed_ms(artifact["cold"], "cold"),
            warm_ms=index_elapsed_ms(artifact["warm"], "warm"),
            worktree=kept_worktree,
        )
    finally:
        if not args.keep_worktree:
            tmpdir.cleanup()
