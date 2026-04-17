#!/usr/bin/env python3
"""
Reference host adapter: produce a `PlannerBrief` handoff artifact.

This demonstrates that `docs/schemas/handoff-artifact.v1.json` is
implementable from an external host using only the stdio MCP transport
and stdlib JSON. It is NOT bundled with the codelens-mcp crate; it is a
sample under `examples/handoff/` that reviewers can run locally to
verify the handoff contract end-to-end.

Flow:
    1. Spawn a codelens-mcp subprocess with stdio transport, planner-readonly profile.
    2. Send MCP `initialize` + `tools/list`.
    3. Call `analyze_change_request` to compress a simulated task into
       ranked files + readiness evidence.
    4. Construct a PlannerBrief JSON conforming to
       schema_version=codelens-handoff-artifact-v1, kind=planner_brief.
    5. Print the artifact to stdout.

Usage:
    python3 examples/handoff/planner_brief_producer.py \\
        --binary target/release/codelens-mcp \\
        --project . \\
        --task "Refactor query_analysis to split intent and bridge modules"

Run `jq < output.json` to pretty-print. Validate with any JSON Schema
validator against `docs/schemas/handoff-artifact.v1.json`.

This script intentionally uses stdlib only so it has no pip
dependency surface. A richer adapter would use ajv, pydantic, or
equivalent to validate before emitting.
"""

from __future__ import annotations

import argparse
import datetime
import json
import subprocess
import sys
import uuid
from typing import Any


SCHEMA_VERSION = "codelens-handoff-artifact-v1"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Produce a PlannerBrief handoff artifact via codelens-mcp stdio",
    )
    parser.add_argument("--binary", required=True, help="Path to codelens-mcp binary")
    parser.add_argument(
        "--project", default=".", help="Project root to pass to codelens-mcp"
    )
    parser.add_argument("--task", required=True, help="Change request task description")
    parser.add_argument(
        "--target-file",
        action="append",
        default=None,
        help="Optional target file paths (repeatable). Defaults to analyze_change_request's top findings.",
    )
    parser.add_argument(
        "--output",
        default="-",
        help="Output path for the produced artifact JSON. `-` prints to stdout.",
    )
    return parser.parse_args()


class StdioClient:
    """Minimal JSON-RPC 2.0 client over a child process's stdio."""

    def __init__(self, binary: str, project: str) -> None:
        self.proc = subprocess.Popen(
            [binary, project, "--profile", "planner-readonly"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=sys.stderr,
            text=True,
            bufsize=1,
        )

    def notify(self, method: str, params: dict[str, Any] | None = None) -> None:
        """Fire-and-forget JSON-RPC notification — no response expected."""
        frame: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            frame["params"] = params
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(frame) + "\n")
        self.proc.stdin.flush()

    def request(self, method: str, params: dict[str, Any] | None = None) -> Any:
        request_id = str(uuid.uuid4())
        frame: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            frame["params"] = params
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(frame) + "\n")
        self.proc.stdin.flush()
        while True:
            assert self.proc.stdout is not None
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("codelens-mcp closed stdout before responding")
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue
            # Notifications have no id; skip.
            if frame.get("id") == request_id:
                if "error" in frame:
                    raise RuntimeError(f"JSON-RPC error: {frame['error']}")
                return frame.get("result")

    def close(self) -> None:
        try:
            if self.proc.stdin is not None:
                self.proc.stdin.close()
        finally:
            self.proc.terminate()
            self.proc.wait(timeout=5)


def call_tool(
    client: StdioClient, name: str, arguments: dict[str, Any]
) -> dict[str, Any]:
    return client.request(
        "tools/call",
        {"name": name, "arguments": arguments},
    )


def unwrap_structured(tool_result: Any) -> dict[str, Any]:
    """MCP tools/call returns structuredContent as the typed payload."""
    if isinstance(tool_result, dict):
        if "structuredContent" in tool_result:
            return tool_result["structuredContent"]
        if "content" in tool_result and tool_result["content"]:
            first = tool_result["content"][0]
            if isinstance(first, dict) and "text" in first:
                try:
                    return json.loads(first["text"])
                except json.JSONDecodeError:
                    return {}
    return {}


def build_planner_brief(
    args: argparse.Namespace, client: StdioClient
) -> dict[str, Any]:
    analysis_result = unwrap_structured(
        call_tool(
            client,
            "analyze_change_request",
            {"task": args.task, "profile_hint": "planner-readonly"},
        )
    )

    ranked_context = []
    for finding in analysis_result.get("top_findings", []):
        # analyze_change_request top_findings are "symbol: start in <path>" strings.
        if isinstance(finding, str) and ": start in " in finding:
            symbol, path = finding.split(": start in ", 1)
            ranked_context.append(
                {
                    "kind": "symbol",
                    "reference": f"{path}#{symbol}",
                    "why": "analyze_change_request top finding",
                }
            )

    target_paths = args.target_file or []
    if not target_paths:
        for item in analysis_result.get("ranked_context", []):
            if isinstance(item, dict) and item.get("path"):
                target_paths.append(item["path"])

    readiness = analysis_result.get("readiness", {})
    verifier_checks = analysis_result.get("verifier_checks", [])
    mutation_ready = readiness.get("mutation_ready", "unknown")

    preflight_report = {
        "status": (
            mutation_ready
            if mutation_ready in ("ready", "caution", "blocked")
            else "caution"
        ),
        "generated_at": datetime.datetime.now(datetime.UTC).isoformat(),
        "blockers": [str(b) for b in analysis_result.get("blockers", [])],
        "cautions": [
            check["summary"]
            for check in verifier_checks
            if isinstance(check, dict) and check.get("status") == "caution"
        ],
    }

    artifact: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "kind": "planner_brief",
        "session_id": f"planner-example-{uuid.uuid4().hex[:8]}",
        "producer": {
            "role": "planner-reviewer",
            "surface": "planner-readonly",
            "client_name": "planner_brief_producer.py",
            "client_version": "0.1.0",
        },
        "created_at": datetime.datetime.now(datetime.UTC).isoformat(),
        "payload": {
            "goal": args.task,
            "rationale": "Generated from analyze_change_request via stdio MCP",
            "ranked_context": ranked_context,
            "target_paths": target_paths,
            "acceptance": [
                {
                    "id": "ac-1",
                    "statement": f"The change addresses: {args.task}",
                    "verification": "cargo test --features http",
                }
            ],
            "preflight": {"verify_change_readiness": preflight_report},
        },
    }
    return artifact


def main() -> int:
    args = parse_args()
    client = StdioClient(args.binary, args.project)
    try:
        # Initialize handshake.
        client.request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "planner_brief_producer",
                    "version": "0.1.0",
                },
            },
        )
        client.notify("notifications/initialized")
        brief = build_planner_brief(args, client)
    finally:
        client.close()

    payload = json.dumps(brief, indent=2, ensure_ascii=False) + "\n"
    if args.output == "-":
        sys.stdout.write(payload)
    else:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(payload)
    return 0


if __name__ == "__main__":
    sys.exit(main())
