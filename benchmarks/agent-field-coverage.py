#!/usr/bin/env python3
"""Agent-facing field coverage A/B harness.

Probes a CodeLens MCP binary with the workflow tools that agents
actually drive (impact_report, get_impact_analysis, analyze_change_request,
refactor_safety_report, semantic_code_review) and records which
decision-ready fields are present in the response. Used to compare
v1.9.50 vs v1.9.52 directly on the axes that matter for agent harnesses
(Opus/Sonnet/Haiku/Codex/parallel subagents), not on raw token ratio.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

# Fields grouped by which agent tier relies on them most heavily.
AGENT_FIELDS = {
    "haiku_preclassified": [
        "traversal_kind",  # O8a — per-call + per-entry
        "session_continuation_hint",  # O8a — doom-loop avoidance
        "risk_level",
        "mutation_ready",
    ],
    "sonnet_chain_direct": [
        "suggested_next_tools",
        "section_handles",
        "available_sections",
        "summary_resource",
        "suggestion_reasons",
    ],
    "codex_mutation_gate": [
        "blockers",
        "blocker_count",
        "verifier_checks",
        "readiness",
        "readiness_score",
    ],
    "opus_deep_context": [
        "analysis_id",
        "top_findings",
        "next_actions",
        "quality_focus",
        "recommended_checks",
        "performance_watchpoints",
    ],
    "parallel_handoff": [
        "handoff_id",
        "carry_forward",
        "delegate_tool",
        "delegate_arguments",
    ],
}

ALL_FIELDS = [f for group in AGENT_FIELDS.values() for f in group]


def probe_tool(
    binary: Path, project: Path, tool: str, args: dict, timeout: float = 20.0
) -> dict:
    """Drive the MCP stdio server through one initialize + tools/call round."""
    init = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"},
        },
    }
    initialized = {"jsonrpc": "2.0", "method": "notifications/initialized"}
    call = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": tool, "arguments": args},
    }
    payload = "\n".join(json.dumps(m) for m in [init, initialized, call]) + "\n"

    env = os.environ.copy()
    env.setdefault("CODELENS_EFFORT_LEVEL", "high")
    env["CODELENS_PROJECT"] = str(project)

    proc = subprocess.run(
        [str(binary)],
        input=payload,
        capture_output=True,
        text=True,
        timeout=timeout,
        cwd=str(project),
        env=env,
    )
    # Server may emit multiple JSON-RPC lines; find the tools/call response (id=2).
    out = proc.stdout.splitlines()
    response = None
    for line in out:
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get("id") == 2:
            response = obj
            break
    return {
        "tool": tool,
        "args": args,
        "stdout": proc.stdout[-2000:] if response is None else None,
        "stderr": (
            proc.stderr[-2000:] if proc.returncode != 0 or response is None else None
        ),
        "returncode": proc.returncode,
        "response": response,
    }


def extract_data(response: dict | None) -> dict:
    if not response:
        return {}
    result = response.get("result") or {}
    # Prefer structuredContent; fall back to parsed text.
    sc = result.get("structuredContent")
    if isinstance(sc, dict):
        return sc
    contents = result.get("content") or []
    for block in contents:
        if block.get("type") == "text":
            text = block.get("text", "")
            start = text.find("{")
            if start >= 0:
                depth = 0
                end = len(text)
                for i, ch in enumerate(text[start:], start=start):
                    if ch in "{[":
                        depth += 1
                    elif ch in "}]":
                        depth -= 1
                        if depth == 0:
                            end = i + 1
                            break
                try:
                    parsed = json.loads(text[start:end])
                    if isinstance(parsed, dict) and "data" in parsed:
                        return parsed["data"]
                    return parsed
                except json.JSONDecodeError:
                    continue
    return {}


def has_field(data: dict, field: str) -> bool:
    if field in data:
        return True
    # also check nested blast_radius entries for traversal_kind
    if field == "traversal_kind":
        br = data.get("blast_radius") or []
        if (
            isinstance(br, list)
            and br
            and isinstance(br[0], dict)
            and "traversal_kind" in br[0]
        ):
            return True
    return False


def score(data: dict) -> dict:
    coverage = {}
    for group, fields in AGENT_FIELDS.items():
        present = [f for f in fields if has_field(data, f)]
        coverage[group] = {
            "present": present,
            "missing": [f for f in fields if f not in present],
            "rate": len(present) / len(fields) if fields else 0.0,
        }
    return coverage


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True, type=Path)
    ap.add_argument("--project", required=True, type=Path)
    ap.add_argument("--output", required=True, type=Path)
    ap.add_argument("--label", required=True)
    args = ap.parse_args()

    probes = [
        (
            "get_impact_analysis",
            {"file_path": "crates/codelens-mcp/src/main.rs", "max_depth": 2},
        ),
        ("impact_report", {"file_path": "crates/codelens-mcp/src/main.rs"}),
        (
            "analyze_change_request",
            {"task": "Add session_continuation_hint to all workflow handle payloads"},
        ),
        (
            "refactor_safety_report",
            {
                "task": "rename handle_request",
                "symbol": "handle_request",
                "file_path": "crates/codelens-mcp/src/server/router.rs",
            },
        ),
        ("semantic_code_review", {"task": "review the MCP dispatch pipeline"}),
    ]

    results = []
    for tool, tool_args in probes:
        t0 = time.time()
        probe = probe_tool(args.binary, args.project, tool, tool_args)
        probe["elapsed_ms"] = int((time.time() - t0) * 1000)
        data = extract_data(probe.get("response"))
        probe["data_keys"] = sorted(data.keys()) if isinstance(data, dict) else []
        probe["coverage"] = score(data) if isinstance(data, dict) else {}
        # drop raw response from the output to keep artifact small
        probe["response_summary"] = {
            "ok": probe.get("response") is not None,
            "has_structured": isinstance(
                (probe.get("response") or {})
                .get("result", {})
                .get("structuredContent"),
                dict,
            ),
        }
        probe.pop("response", None)
        results.append(probe)

    # Aggregate per-group coverage
    agg = {}
    for group in AGENT_FIELDS:
        rates = [r["coverage"][group]["rate"] for r in results if r.get("coverage")]
        agg[group] = {
            "mean_rate": sum(rates) / len(rates) if rates else 0.0,
            "per_tool": {
                r["tool"]: r["coverage"][group]["rate"]
                for r in results
                if r.get("coverage")
            },
        }

    payload = {
        "label": args.label,
        "binary": str(args.binary),
        "project": str(args.project),
        "results": results,
        "aggregate_coverage": agg,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, indent=2))
    print(f"[{args.label}] wrote {args.output}")
    for group, data in agg.items():
        print(f"  {group:<28} mean_rate={data['mean_rate']:.2f}")


if __name__ == "__main__":
    sys.exit(main())
