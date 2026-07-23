#!/usr/bin/env python3
"""Per-tool response token profiler (MCP Interviewer prescription).

Fires a fixed suite of read-only probe calls at the already-running daemon
(default http://127.0.0.1:7838) and measures the actual response size of each
tool (chars, approx tokens = chars / 4). Tools whose measured response exceeds
an absolute budget (default 1500 tokens) are flagged as heavy.

The manifest's ``estimated_tokens`` is reported as a side column named
``schema_tokens`` for context only: that field measures the serialized size of
the TOOL DEFINITION (its context-window cost in tools/list), not an expected
response size — comparing response tokens against it as a ratio is a category
error (an earlier revision of this script did exactly that).

- read-only probes only; never calls a mutation tool.
- probes are intersected with the daemon's active surface (tools/list), so
  tools absent from the current profile are reported as skipped, not failures.
- If the daemon is unreachable: prints a clear error and exits 0 (skip), so a
  CI matrix without a live daemon degrades gracefully rather than hard-failing.

stdlib-only, matching the other benchmark scripts.
"""

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import benchmark_runtime_common as rc  # noqa: E402

DEFAULT_MANIFEST = "docs/generated/surface-manifest.json"

# Fixed read-only probe suite. Args are concrete and target THIS repo so the
# measured response is representative. Tools not in the active surface are
# skipped automatically.
PROBE_SUITE = [
    ("prepare_harness_session", {"project": "."}),
    ("find_symbol", {"name": "rename_symbol"}),
    ("get_symbols_overview", {"file_path": "crates/codelens-engine/src/rename.rs"}),
    ("bm25_symbol_search", {"query": "rename symbol across project", "max_results": 5}),
    ("get_ranked_context", {"query": "rename a symbol across the project", "max_results": 5}),
    (
        "find_referencing_symbols",
        {"symbol_name": "rename_symbol", "path": "crates/codelens-engine/src/rename.rs"},
    ),
    ("get_file_diagnostics", {"file_path": "crates/codelens-engine/src/rename.rs"}),
    ("search_symbols_fuzzy", {"query": "rename", "max_results": 5}),
    ("get_complexity", {"file_path": "crates/codelens-engine/src/rename.rs"}),
    ("impact_report", {"symbol": "rename_symbol"}),
]


def parse_args():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--base-url", default="http://127.0.0.1:7838")
    p.add_argument(
        "--project",
        default=None,
        help="Project for prepare_harness_session probe. Default: the daemon's "
        "already-bound project (avoids rebinding to a non-indexed worktree).",
    )
    p.add_argument("--manifest", default=DEFAULT_MANIFEST)
    p.add_argument("--chars-per-token", type=float, default=4.0)
    p.add_argument(
        "--response-budget",
        type=float,
        default=1500.0,
        help="Flag tools whose measured response exceeds this many tokens",
    )
    p.add_argument("--output", default=None, help="Optional JSON output path")
    return p.parse_args()


def load_estimates(manifest_path: str) -> dict:
    data = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
    tools = data.get("tool_registry", {}).get("tools", [])
    return {t["name"]: t.get("estimated_tokens") for t in tools if "name" in t}


def response_chars(raw_response: dict) -> int:
    """Chars the model actually receives: the tool result content text."""
    try:
        result = raw_response.get("result", {})
        content = result.get("content")
        if isinstance(content, list) and content:
            return sum(len(part.get("text", "")) for part in content)
        return len(json.dumps(result, ensure_ascii=False))
    except Exception:  # noqa: BLE001
        return len(json.dumps(raw_response, ensure_ascii=False))


def available_tools(base_url: str, session_id: str) -> set:
    resp = rc.mcp_http_call(
        base_url, "tools/list", {}, headers={"mcp-session-id": session_id}
    )
    result = resp.get("result", {}) if isinstance(resp, dict) else {}
    return {t.get("name") for t in result.get("tools", []) if t.get("name")}


def active_project(base_url: str, session_id: str):
    """Read the daemon's already-bound project so the prepare_harness_session
    probe binds to the indexed repo instead of a non-indexed worktree."""
    resp = rc.mcp_http_tool_call(
        base_url, "bm25_symbol_search", {"query": "a", "max_results": 1},
        session_id=session_id,
    )
    payload = rc.extract_tool_payload(resp)
    data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
    binding = data.get("project_binding") if isinstance(data, dict) else None
    if isinstance(binding, dict):
        return binding.get("active_project") or binding.get("session_project")
    return None


def profile(args) -> dict:
    try:
        session_id, _, _ = rc.initialize_http_session(
            args.base_url, client_name="tool-token-profile"
        )
    except Exception as exc:  # noqa: BLE001
        print(
            f"SKIP: daemon unreachable at {args.base_url}/mcp: {exc}",
            file=sys.stderr,
        )
        print(
            "Start the read-only daemon (see CLAUDE.md HTTP Daemon Operations).",
            file=sys.stderr,
        )
        return {"skipped": True, "reason": "daemon_unreachable"}

    estimates = load_estimates(args.manifest)
    surface = available_tools(args.base_url, session_id)
    if args.project:
        bound_project = str(Path(args.project).resolve())
    else:
        bound_project = active_project(args.base_url, session_id)

    records = []
    for tool, base_args in PROBE_SUITE:
        call_args = dict(base_args)
        if call_args.get("project") == "." and bound_project:
            call_args["project"] = bound_project
        if tool not in surface:
            records.append({"tool": tool, "status": "skipped_not_in_surface"})
            continue
        raw = rc.mcp_http_tool_call(
            args.base_url, tool, call_args, session_id=session_id
        )
        if isinstance(raw, dict) and raw.get("error"):
            records.append(
                {
                    "tool": tool,
                    "status": "error",
                    "error": str(raw["error"].get("message", raw["error"]))[:120],
                }
            )
            continue
        if tool == "prepare_harness_session":
            # #357: bootstrap listing is compact; prepare flips the session to
            # full exposure. Re-fetch the surface so post-bootstrap tools are
            # probed instead of reported as skipped_not_in_surface.
            surface = available_tools(args.base_url, session_id)
        chars = response_chars(raw)
        actual_tokens = round(chars / args.chars_per_token)
        schema_tokens = estimates.get(tool)
        flagged = actual_tokens > args.response_budget
        records.append(
            {
                "tool": tool,
                "status": "measured",
                "actual_chars": chars,
                "actual_tokens": actual_tokens,
                "schema_tokens": schema_tokens,
                "flagged": flagged,
            }
        )
    return {
        "skipped": False,
        "response_budget": args.response_budget,
        "chars_per_token": args.chars_per_token,
        "records": records,
        "flagged": [r["tool"] for r in records if r.get("flagged")],
    }


def render(report: dict) -> str:
    if report.get("skipped"):
        return f"Profiler skipped: {report.get('reason')}\n"
    lines = ["# Tool Response Token Profile", ""]
    lines.append(f"- Flag: measured response > {report['response_budget']:.0f} tokens")
    lines.append(f"- chars/token: {report['chars_per_token']}")
    lines.append(
        "- schema_tokens = serialized tool-DEFINITION cost in tools/list "
        "(context cost), shown for reference only — not a response estimate"
    )
    lines.append("")
    lines.append("| Tool | Response tokens | Schema tokens | Flag |")
    lines.append("| --- | --- | --- | --- |")
    for r in report["records"]:
        if r["status"] != "measured":
            lines.append(f"| {r['tool']} | — | — | {r['status']} |")
            continue
        flag = "HEAVY" if r["flagged"] else ""
        lines.append(
            f"| {r['tool']} | {r['actual_tokens']} | {r['schema_tokens']} | {flag} |"
        )
    lines.append("")
    if report["flagged"]:
        lines.append(
            f"**Heavy responses (> {report['response_budget']:.0f} tokens):** "
            + ", ".join(report["flagged"]))
    else:
        lines.append("No tool exceeded the response budget.")
    return "\n".join(lines) + "\n"


def main():
    args = parse_args()
    report = profile(args)
    print(render(report))
    if args.output and not report.get("skipped"):
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"Wrote {out_path}")


if __name__ == "__main__":
    main()
