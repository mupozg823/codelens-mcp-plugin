#!/usr/bin/env python3
"""
Reference host adapter: produce a `ReviewerVerdict` handoff artifact.

Completes the external producer trio under `examples/handoff/` —
sibling of `planner_brief_producer.py` (first leg) and
`builder_result_producer.py` (second leg).

Flow:
    1. Spawn a codelens-mcp subprocess with the read-only
       `reviewer-graph` profile.
    2. Call `audit_planner_session` for the reviewer's own session to
       prove the reviewer stayed read-only (the schema's
       `ReviewerVerdict.audit` field).
    3. Optionally call `audit_builder_session` for the reviewed
       builder session so the decision logic sees the builder's
       status + findings.
    4. Compute the decision:
        - both audits `pass`            -> approve
        - reviewer `fail`                -> block (reviewer contract break)
        - builder `fail`                 -> block (mutation gate break)
        - any `warn` + builder findings  -> request_changes
        - otherwise                      -> request_changes
    5. Emit a ReviewerVerdict conforming to
       schema_version=codelens-handoff-artifact-v1 / kind=reviewer_verdict.

Usage:
    python3 examples/handoff/reviewer_verdict_producer.py \\
        --binary target/release/codelens-mcp \\
        --project . \\
        --reviewed-session-id <builder-session-id> \\
        --builder-result builder-result.json \\
        --output reviewer-verdict.json
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
        description="Produce a ReviewerVerdict handoff artifact via codelens-mcp stdio",
    )
    parser.add_argument("--binary", required=True, help="Path to codelens-mcp binary")
    parser.add_argument(
        "--project", default=".", help="Project root to pass to codelens-mcp"
    )
    parser.add_argument(
        "--reviewed-session-id",
        default="",
        help="Builder session id to review. Omit to review the current (demo) session.",
    )
    parser.add_argument(
        "--builder-result",
        default="",
        help="Optional path to a builder_result JSON; its session_id becomes parent_artifact.session_id.",
    )
    parser.add_argument(
        "--rationale",
        default="",
        help="Optional free-form rationale. Defaults to a rationale derived from the audits.",
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
            [binary, project, "--profile", "reviewer-graph"],
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


def extract_failed_checks(audit: dict[str, Any]) -> list[str]:
    findings = audit.get("findings") or []
    return [
        finding["code"]
        for finding in findings
        if isinstance(finding, dict) and isinstance(finding.get("code"), str)
    ]


def normalize_audit_status(status: Any) -> str:
    if status in ("pass", "warn", "fail"):
        return status
    return "warn"


def decide(reviewer_status: str, builder_status: str, builder_failed: list[str]) -> str:
    if reviewer_status == "fail":
        return "block"
    if builder_status == "fail":
        return "block"
    if reviewer_status == "pass" and builder_status == "pass":
        return "approve"
    if builder_status in ("warn", "fail") and builder_failed:
        return "request_changes"
    return "request_changes"


def build_reviewer_verdict(
    args: argparse.Namespace, client: StdioClient
) -> dict[str, Any]:
    # Audit the reviewer's own session — proves read-only compliance.
    reviewer_audit = unwrap_structured(call_tool(client, "audit_planner_session", {}))
    reviewer_status = normalize_audit_status(reviewer_audit.get("status"))

    # Audit the reviewed builder session (if provided).
    builder_audit: dict[str, Any] = {}
    if args.reviewed_session_id:
        builder_audit = unwrap_structured(
            call_tool(
                client,
                "audit_builder_session",
                {"session_id": args.reviewed_session_id},
            )
        )
    builder_status = normalize_audit_status(builder_audit.get("status"))
    builder_failed = extract_failed_checks(builder_audit)

    decision = decide(reviewer_status, builder_status, builder_failed)

    rationale = args.rationale
    if not rationale:
        rationale = (
            f"Reviewer audit: {reviewer_status}; "
            f"reviewed builder audit: {builder_status}; "
            f"builder findings: {', '.join(builder_failed) if builder_failed else 'none'}."
        )

    parent_session_id = ""
    if args.builder_result:
        try:
            with open(args.builder_result, "r", encoding="utf-8") as fh:
                prior = json.load(fh)
            parent_session_id = prior.get("session_id", "")
        except (OSError, json.JSONDecodeError):
            parent_session_id = ""

    requested_changes: list[dict[str, Any]] = []
    if decision == "request_changes":
        # Map each builder finding into a concrete change request.
        for code in builder_failed:
            requested_changes.append(
                {
                    "statement": f"Resolve builder audit finding `{code}` before the next dispatch.",
                }
            )
        if not requested_changes:
            requested_changes.append(
                {
                    "statement": (
                        f"Reviewer status is {reviewer_status} / builder status is "
                        f"{builder_status}; re-run with full preflight + audits before merge."
                    ),
                }
            )

    result: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "kind": "reviewer_verdict",
        "session_id": f"reviewer-example-{uuid.uuid4().hex[:8]}",
        "producer": {
            "role": "reviewer",
            "surface": "reviewer-graph",
            "client_name": "reviewer_verdict_producer.py",
            "client_version": "0.1.0",
        },
        "created_at": datetime.datetime.now(datetime.UTC).isoformat(),
        "payload": {
            "decision": decision,
            "rationale": rationale,
            "audit": {
                "status": reviewer_status,
                "failed_checks": extract_failed_checks(reviewer_audit),
            },
        },
    }

    if requested_changes:
        result["payload"]["requested_changes"] = requested_changes

    if parent_session_id:
        result["parent_artifact"] = {
            "kind": "builder_result",
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
                    "name": "reviewer_verdict_producer",
                    "version": "0.1.0",
                },
            },
        )
        client.notify("notifications/initialized")
        verdict = build_reviewer_verdict(args, client)
    finally:
        client.close()

    payload = json.dumps(verdict, indent=2, ensure_ascii=False) + "\n"
    if args.output == "-":
        sys.stdout.write(payload)
    else:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(payload)
    return 0


if __name__ == "__main__":
    sys.exit(main())
