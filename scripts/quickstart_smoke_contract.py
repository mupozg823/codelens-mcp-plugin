from __future__ import annotations

import json
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from types import NoneType
from typing import TypeAlias


JsonValue: TypeAlias = (
    NoneType | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)


class QuickstartSmokeError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class CoverageSmoke:
    status: str
    indexed_symbols: int
    indexed_files: int
    readiness_percent: int
    query_cache_entries: int


@dataclass(frozen=True, slots=True)
class RetrievalSmoke:
    top_symbol: str
    top_file: str
    token_estimate: int


@dataclass(frozen=True, slots=True)
class QuickstartSummary:
    version: str
    temp_root: Path
    coverage: CoverageSmoke
    retrieval: RetrievalSmoke

    def render(self) -> str:
        return (
            "clean_quickstart: "
            f"status={self.coverage.status} "
            f"indexed_symbols={self.coverage.indexed_symbols} "
            f"indexed_files={self.coverage.indexed_files} "
            f"readiness_percent={self.coverage.readiness_percent}% "
            f"query_cache.entries={self.coverage.query_cache_entries} "
            f"top_symbol={self.retrieval.top_symbol} "
            f"top_file={self.retrieval.top_file} "
            f"token_estimate={self.retrieval.token_estimate} "
            f"binary={self.version}"
        )

    def to_json(self) -> dict[str, JsonValue]:
        return {
            "status": self.coverage.status,
            "temp_root": str(self.temp_root),
            "binary": self.version,
            "coverage": {
                "indexed_symbols": self.coverage.indexed_symbols,
                "indexed_files": self.coverage.indexed_files,
                "readiness_percent": self.coverage.readiness_percent,
                "query_cache_entries": self.coverage.query_cache_entries,
            },
            "retrieval": {
                "top_symbol": self.retrieval.top_symbol,
                "top_file": self.retrieval.top_file,
                "token_estimate": self.retrieval.token_estimate,
            },
        }


def as_object(value: JsonValue, label: str) -> Mapping[str, JsonValue]:
    match value:
        case dict() as mapping:
            return mapping
        case _:
            raise QuickstartSmokeError(f"{label} must be an object")


def as_list(value: JsonValue, label: str) -> list[JsonValue]:
    match value:
        case list() as items:
            return items
        case _:
            raise QuickstartSmokeError(f"{label} must be an array")


def require_key(mapping: Mapping[str, JsonValue], key: str, label: str) -> JsonValue:
    if key not in mapping:
        raise QuickstartSmokeError(f"{label}.{key} is missing")
    return mapping[key]


def require_bool(mapping: Mapping[str, JsonValue], key: str, label: str) -> bool:
    value = require_key(mapping, key, label)
    match value:
        case bool() as flag:
            return flag
        case _:
            raise QuickstartSmokeError(f"{label}.{key} must be a boolean")


def require_int(
    mapping: Mapping[str, JsonValue], key: str, label: str, *, minimum: int
) -> int:
    value = require_key(mapping, key, label)
    match value:
        case int() as number if not isinstance(number, bool) and number >= minimum:
            return number
        case _:
            raise QuickstartSmokeError(f"{label}.{key} must be an int >= {minimum}")


def require_str(mapping: Mapping[str, JsonValue], key: str, label: str) -> str:
    value = require_key(mapping, key, label)
    match value:
        case str() as text if text:
            return text
        case _:
            raise QuickstartSmokeError(f"{label}.{key} must be a non-empty string")


def parse_json_stdout(stdout: str, label: str) -> JsonValue:
    stripped = stdout.strip()
    starts = [pos for marker in ("{", "[") if (pos := stripped.find(marker)) >= 0]
    if not starts:
        raise QuickstartSmokeError(f"{label} printed no JSON")
    try:
        parsed, _end = json.JSONDecoder().raw_decode(stripped[min(starts) :])
    except json.JSONDecodeError as error:
        raise QuickstartSmokeError(f"{label} printed invalid JSON: {error}") from error
    return parsed


def extract_tool_data(payload: JsonValue, label: str) -> Mapping[str, JsonValue]:
    root = as_object(payload, label)
    if root.get("success") is not True:
        raise QuickstartSmokeError(f"{label}.success must be true")
    data = root.get("data")
    return as_object(data, f"{label}.data") if data is not None else root


def validate_status(payload: JsonValue) -> None:
    root = as_object(payload, "status")
    hosts = as_list(require_key(root, "hosts", "status"), "status.hosts")
    attached = False
    for host_entry in hosts:
        host = as_object(host_entry, "status.host")
        if host.get("host") != "codex":
            continue
        files = as_list(require_key(host, "files", "status.hosts.codex"), "status.files")
        for file_entry in files:
            file_data = as_object(file_entry, "status.file")
            path = require_str(file_data, "path", "status.file")
            status = require_str(file_data, "status", "status.file")
            attached = attached or (
                path.endswith(".codex/config.toml") and status.startswith("attached")
            )
    if not attached:
        raise QuickstartSmokeError("status must detect attached Codex config")


def validate_capabilities(payload: JsonValue) -> None:
    data = extract_tool_data(payload, "get_capabilities")
    status = require_str(data, "semantic_search_status", "capabilities.data")
    if status != "index_missing":
        raise QuickstartSmokeError(
            "capabilities must start with semantic_search_status=index_missing"
        )
    if require_bool(data, "embedding_indexed", "capabilities.data"):
        raise QuickstartSmokeError(
            "capabilities.embedding_indexed must be false before indexing"
        )
    require_int(data, "supported_files", "capabilities.data", minimum=1)


def validate_index(payload: JsonValue) -> None:
    data = extract_tool_data(payload, "index_embeddings")
    require_int(data, "indexed_symbols", "index_embeddings.data", minimum=1)
    query_cache = as_object(
        require_key(data, "query_cache", "index_embeddings.data"),
        "index_embeddings.data.query_cache",
    )
    if not require_bool(query_cache, "enabled", "index_embeddings.data.query_cache"):
        raise QuickstartSmokeError("index_embeddings.data.query_cache.enabled must be true")


def validate_coverage(payload: JsonValue) -> CoverageSmoke:
    data = extract_tool_data(payload, "embedding_coverage_report")
    if not require_bool(data, "compiled", "coverage.data"):
        raise QuickstartSmokeError("coverage.data.compiled must be true")
    if require_str(data, "status", "coverage.data") != "ready":
        raise QuickstartSmokeError("coverage.data.status must be ready")
    model_assets = as_object(
        require_key(data, "model_assets", "coverage.data"),
        "coverage.data.model_assets",
    )
    if not require_bool(model_assets, "available", "coverage.data.model_assets"):
        raise QuickstartSmokeError("coverage.data.model_assets.available must be true")
    index = as_object(require_key(data, "index", "coverage.data"), "coverage.data.index")
    if require_bool(index, "model_mismatch", "coverage.data.index"):
        raise QuickstartSmokeError("coverage.data.index.model_mismatch must be false")
    stale_files = require_int(index, "stale_files", "coverage.data.index", minimum=0)
    if stale_files != 0:
        raise QuickstartSmokeError("coverage.data.index.stale_files must be 0")
    query_cache = as_object(
        require_key(data, "query_cache", "coverage.data"),
        "coverage.data.query_cache",
    )
    return CoverageSmoke(
        status="ready",
        indexed_symbols=require_int(
            index, "indexed_symbols", "coverage.data.index", minimum=1
        ),
        indexed_files=require_int(index, "indexed_files", "coverage.data.index", minimum=1),
        readiness_percent=require_int(
            index, "readiness_percent", "coverage.data.index", minimum=100
        ),
        query_cache_entries=require_int(
            query_cache, "entries", "coverage.data.query_cache", minimum=0
        ),
    )


def validate_retrieval(payload: JsonValue) -> RetrievalSmoke:
    root = as_object(payload, "get_ranked_context")
    if root.get("success") is not True:
        raise QuickstartSmokeError("get_ranked_context.success must be true")
    token_estimate = require_int(root, "token_estimate", "get_ranked_context", minimum=1)
    data = extract_tool_data(payload, "get_ranked_context")
    retrieval = as_object(
        require_key(data, "retrieval", "get_ranked_context.data"),
        "get_ranked_context.data.retrieval",
    )
    for key in ("semantic_used_in_core", "sparse_used_in_core"):
        if not require_bool(retrieval, key, "get_ranked_context.data.retrieval"):
            raise QuickstartSmokeError(f"retrieval.{key} must be true")
    query_type = require_str(retrieval, "query_type", "get_ranked_context.data.retrieval")
    if query_type != "natural_language":
        raise QuickstartSmokeError("retrieval.query_type must be natural_language")
    symbols = as_list(
        require_key(data, "symbols", "get_ranked_context.data"),
        "get_ranked_context.data.symbols",
    )
    if not symbols:
        raise QuickstartSmokeError("get_ranked_context.data.symbols must be non-empty")
    top = as_object(symbols[0], "get_ranked_context.data.symbols[0]")
    top_symbol = require_str(top, "name", "get_ranked_context.data.symbols[0]")
    top_file = require_str(top, "file", "get_ranked_context.data.symbols[0]")
    if top_symbol != "add_values":
        raise QuickstartSmokeError("top ranked symbol must be add_values")
    return RetrievalSmoke(top_symbol=top_symbol, top_file=top_file, token_estimate=token_estimate)
