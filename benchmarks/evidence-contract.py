#!/usr/bin/env python3
"""Validate additive graph/retrieval evidence payloads from a CodeLens binary."""

from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from benchmark_runtime_common import build_token_counter, codelens, tool_payload_succeeded


DEFAULT_BINARY = ROOT / "target" / "release" / "codelens-mcp"
EVIDENCE_SCHEMA_VERSION = "codelens-evidence-v1"
KNOWN_EVIDENCE_BACKENDS = {"tree-sitter", "hybrid", "semantic", "sqlite", "scip", "lsp"}
PROBES = (
    {
        "id": "find_symbol",
        "tool": "find_symbol",
        "domain": "symbol",
        "requires_precision_signals": True,
        "args": {
            "name": "handle_request",
            "file_path": "crates/codelens-mcp/src/server/router.rs",
            "max_matches": 5,
        },
    },
    {
        "id": "find_referencing_symbols",
        "tool": "find_referencing_symbols",
        "domain": "references",
        "requires_precision_signals": True,
        "args": {
            "symbol_name": "handle_request",
            "file_path": "crates/codelens-mcp/src/server/router.rs",
            "max_results": 10,
        },
    },
    {
        "id": "get_ranked_context",
        "tool": "get_ranked_context",
        "domain": "retrieval",
        "requires_precision_signals": True,
        "args": {
            "query": "route an incoming tool request to the right handler",
            "max_tokens": 1200,
        },
    },
    {
        "id": "bm25_symbol_search",
        "tool": "bm25_symbol_search",
        "domain": "retrieval",
        "requires_precision_signals": True,
        "args": {
            "query": "dispatch tool",
            "max_results": 5,
        },
    },
    {
        "id": "get_callers",
        "tool": "get_callers",
        "domain": "call_graph",
        "requires_precision_signals": True,
        "args": {
            "function_name": "handle_request",
            "file_path": "crates/codelens-mcp/src/server/router.rs",
            "max_results": 10,
        },
    },
    {
        "id": "get_callees",
        "tool": "get_callees",
        "domain": "call_graph",
        "requires_precision_signals": True,
        "args": {
            "function_name": "handle_request",
            "file_path": "crates/codelens-mcp/src/server/router.rs",
            "max_results": 10,
        },
    },
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=None)
    parser.add_argument("--project", dest="project_option")
    parser.add_argument("--binary", default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY)))
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--output", default=str(SCRIPT_DIR / "evidence-contract-results.json"))
    parser.add_argument("--timeout-seconds", type=int, default=120)
    return parser.parse_args()


def validate_evidence(evidence: object, *, expected_domain: str) -> list[str]:
    errors: list[str] = []
    if not isinstance(evidence, dict):
        return ["evidence must be an object"]
    if evidence.get("schema_version") != EVIDENCE_SCHEMA_VERSION:
        errors.append(f"schema_version must be {EVIDENCE_SCHEMA_VERSION}")
    if evidence.get("domain") != expected_domain:
        errors.append(f"domain must be {expected_domain}")
    if evidence.get("active_backend") not in KNOWN_EVIDENCE_BACKENDS:
        errors.append("active_backend must be one of known backend labels")
    confidence = evidence.get("confidence")
    if isinstance(confidence, bool) or not isinstance(confidence, (int, float)):
        errors.append("confidence must be a number")
    if not isinstance(evidence.get("confidence_basis"), str) or not evidence.get(
        "confidence_basis"
    ):
        errors.append("confidence_basis must be a string")
    degraded_reason = evidence.get("degraded_reason")
    if degraded_reason is not None and not isinstance(degraded_reason, str):
        errors.append("degraded_reason must be null or string")
    if not isinstance(evidence.get("signals"), dict):
        errors.append("signals must be an object")
    return errors


def validate_precision_signals(evidence: object) -> list[str]:
    if not isinstance(evidence, dict) or not isinstance(evidence.get("signals"), dict):
        return []
    errors: list[str] = []
    signals = evidence["signals"]
    for key in ("precise_available", "precise_used"):
        if not isinstance(signals.get(key), bool):
            errors.append(f"signals.{key} must be boolean")
    for key in ("precise_source", "fallback_source"):
        value = signals.get(key)
        if value is not None and not isinstance(value, str):
            errors.append(f"signals.{key} must be null or string")
    count = signals.get("precise_result_count")
    if isinstance(count, bool) or not isinstance(count, int) or count < 0:
        errors.append("signals.precise_result_count must be a non-negative integer")
    return errors


def data_payload(payload: object) -> dict:
    if not isinstance(payload, dict):
        return {}
    data = payload.get("data")
    return data if isinstance(data, dict) else payload


def run_contract(
    *,
    binary: Path,
    project: Path,
    preset: str,
    timeout_seconds: int,
) -> dict:
    count_tokens, _warning = build_token_counter()
    checks = []
    failures = []
    for probe in PROBES:
        output, _tokens, elapsed_ms, payload = codelens(
            binary,
            project,
            probe["tool"],
            probe["args"],
            count_tokens,
            timeout=timeout_seconds,
            preset=preset,
        )
        errors = []
        if not tool_payload_succeeded(payload):
            errors.append("tool call failed")
        data = data_payload(payload)
        evidence = data.get("evidence")
        errors.extend(validate_evidence(evidence, expected_domain=probe["domain"]))
        if probe.get("requires_precision_signals"):
            errors.extend(validate_precision_signals(evidence))
        check = {
            "id": probe["id"],
            "tool": probe["tool"],
            "domain": probe["domain"],
            "elapsed_ms": elapsed_ms,
            "ok": not errors,
            "errors": errors,
        }
        if errors:
            check["stdout_preview"] = output[-800:]
            failures.append(check)
        checks.append(check)
    return {
        "schema_version": "codelens-evidence-contract-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "binary": str(binary),
        "project": str(project),
        "preset": preset,
        "ok": not failures,
        "failure_count": len(failures),
        "checks": checks,
    }


def main() -> None:
    args = parse_args()
    binary = Path(args.binary).expanduser().resolve()
    project_arg = args.project_option or args.project_path or "."
    project = Path(project_arg).expanduser().resolve()
    report = run_contract(
        binary=binary,
        project=project,
        preset=args.preset,
        timeout_seconds=args.timeout_seconds,
    )
    output = Path(args.output).expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if not report["ok"]:
        raise SystemExit(2)


if __name__ == "__main__":
    main()
