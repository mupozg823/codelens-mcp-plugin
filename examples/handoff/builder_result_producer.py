#!/usr/bin/env python3
"""
Reference host adapter: produce a `BuilderResult` handoff artifact.

Sibling of `planner_brief_producer.py`. Demonstrates the second leg of
the planner -> builder -> reviewer handoff chain by querying
`audit_builder_session` for an already-completed builder session and
reshaping the audit output plus per-file diagnostics into a
`BuilderResult` conforming to
`docs/schemas/handoff-artifact.v1.json`.

Usage (after the builder session has actually run against the daemon
or test harness):

    python3 examples/handoff/builder_result_producer.py \\
        --binary target/release/codelens-mcp \\
        --project . \\
        --session-id <builder-session-id> \\
        --changed-file crates/codelens-mcp/src/tools/reports/eval_reports.rs \\
        --tests-command "cargo test -p codelens-mcp --features http" \\
        --output builder-result.json

`--changed-file` can be repeated. If no builder session exists under
that id the script falls back to using the current running session,
which will typically produce a `not_applicable` status — useful for
quickly smoke-testing the adapter shape without a full seed.

Stdlib only. Verify the output against the schema with any JSON
Schema validator; see `examples/handoff/README.md`.
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
        description="Produce a BuilderResult handoff artifact via codelens-mcp stdio",
    )
    parser.add_argument("--binary", required=True, help="Path to codelens-mcp binary")
    parser.add_argument(
        "--project", default=".", help="Project root to pass to codelens-mcp"
    )
    parser.add_argument(
        "--session-id",
        default="",
        help="Builder session id to audit. Omit to audit the current (demo) session.",
    )
    parser.add_argument(
        "--changed-file",
        action="append",
        default=[],
        help="Builder-touched file path (repeatable).",
    )
    parser.add_argument(
        "--tests-command",
        default="",
        help="Command string to record in BuilderResult.tests.commands[].",
    )
    parser.add_argument(
        "--tests-passed",
        type=int,
        default=0,
        help="Passed test count to record in BuilderResult.tests.passed.",
    )
    parser.add_argument(
        "--tests-failed",
        type=int,
        default=0,
        help="Failed test count to record in BuilderResult.tests.failed.",
    )
    parser.add_argument(
        "--planner-brief",
        default="",
        help="Optional path to a planner_brief JSON; its session_id becomes parent_artifact.session_id.",
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
            [binary, project, "--profile", "refactor-full"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=sys.stderr,
            text=True,
            bufsize=1,
        )

    def notify(self, method: str, params: dict[str, Any] | None = None) -> None:
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


def collect_diagnostics(client: StdioClient, paths: list[str]) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for path in paths:
        diag = unwrap_structured(
            call_tool(client, "get_file_diagnostics", {"file_path": path})
        )
        rows = diag.get("diagnostics") or diag.get("data", {}).get("diagnostics") or []
        if not isinstance(rows, list):
            continue
        for row in rows:
            severity = row.get("severity") if isinstance(row, dict) else None
            summary = row.get("message") if isinstance(row, dict) else None
            if severity not in ("info", "warning", "error") or not summary:
                continue
            results.append(
                {
                    "path": path,
                    "severity": severity,
                    "summary": summary,
                }
            )
    return results


def extract_audit_failures(audit: dict[str, Any]) -> list[str]:
    findings = audit.get("findings") or []
    return [
        finding["code"]
        for finding in findings
        if isinstance(finding, dict) and isinstance(finding.get("code"), str)
    ]


def build_builder_result(
    args: argparse.Namespace, client: StdioClient
) -> dict[str, Any]:
    audit_args: dict[str, Any] = {}
    if args.session_id:
        audit_args["session_id"] = args.session_id
    audit = unwrap_structured(call_tool(client, "audit_builder_session", audit_args))

    parent_session_id = ""
    if args.planner_brief:
        try:
            with open(args.planner_brief, "r", encoding="utf-8") as fh:
                brief = json.load(fh)
            parent_session_id = brief.get("session_id", "")
        except (OSError, json.JSONDecodeError):
            parent_session_id = ""

    changed_files = [
        {
            "path": path,
            "change_type": "modify",
        }
        for path in args.changed_file
    ]
    diagnostics = (
        collect_diagnostics(client, args.changed_file) if args.changed_file else []
    )

    tests_obj: dict[str, Any] = {}
    if args.tests_command:
        tests_obj["commands"] = [args.tests_command]
    tests_obj["passed"] = args.tests_passed
    tests_obj["failed"] = args.tests_failed

    audit_status = audit.get("status")
    if audit_status not in ("pass", "warn", "fail"):
        # The schema allows pass/warn/fail only for BuilderResult.audit.status.
        audit_status = "warn"

    result: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "kind": "builder_result",
        "session_id": args.session_id
        or audit.get("session_summary", {}).get(
            "session_id", f"builder-example-{uuid.uuid4().hex[:8]}"
        ),
        "producer": {
            "role": "builder-refactor",
            "surface": audit.get("session_summary", {}).get(
                "current_surface", "refactor-full"
            ),
            "client_name": "builder_result_producer.py",
            "client_version": "0.1.0",
        },
        "created_at": datetime.datetime.now(datetime.UTC).isoformat(),
        "payload": {
            "changed_files": changed_files,
            "tests": tests_obj,
            "diagnostics": diagnostics,
            "audit": {
                "status": audit_status,
                "failed_checks": extract_audit_failures(audit),
            },
        },
    }
    if parent_session_id:
        result["parent_artifact"] = {
            "kind": "planner_brief",
            "session_id": parent_session_id,
        }
    return result


def main() -> int:
    args = parse_args()
    client = StdioClient(args.binary, args.project)
    try:
        client.request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "builder_result_producer",
                    "version": "0.1.0",
                },
            },
        )
        client.notify("notifications/initialized")
        artifact = build_builder_result(args, client)
    finally:
        client.close()

    payload = json.dumps(artifact, indent=2, ensure_ascii=False) + "\n"
    if args.output == "-":
        sys.stdout.write(payload)
    else:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(payload)
    return 0


if __name__ == "__main__":
    sys.exit(main())
