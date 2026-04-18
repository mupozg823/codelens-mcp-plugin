#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path


DEFAULT_TELEMETRY_PATH = Path(".codelens/telemetry/tool_usage.jsonl")
DEFAULT_MANIFEST_PATH = Path("docs/generated/surface-manifest.json")
DEFAULT_TELEMETRY_RS_PATH = Path("crates/codelens-mcp/src/telemetry.rs")
DEFAULT_ANALYSIS_CACHE_DIR = Path(".codelens/analysis-cache")
TOP_N = 5


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Analyze append-only CodeLens tool usage telemetry and latest audit cache.",
    )
    parser.add_argument(
        "telemetry_path",
        nargs="?",
        default=str(DEFAULT_TELEMETRY_PATH),
        help="Path to tool_usage.jsonl (default: %(default)s)",
    )
    parser.add_argument(
        "--manifest",
        default=str(DEFAULT_MANIFEST_PATH),
        help="Path to docs/generated/surface-manifest.json",
    )
    parser.add_argument(
        "--telemetry-rs",
        default=str(DEFAULT_TELEMETRY_RS_PATH),
        help="Path to telemetry.rs for workflow-tool classification",
    )
    parser.add_argument(
        "--analysis-cache",
        default=str(DEFAULT_ANALYSIS_CACHE_DIR),
        help="Path to .codelens/analysis-cache for latest session_rows.json",
    )
    parser.add_argument(
        "--format",
        choices=("markdown", "json"),
        default="markdown",
        help="Output format",
    )
    parser.add_argument(
        "--output",
        help="Optional output file. Defaults to stdout.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run a built-in synthetic regression check and exit.",
    )
    return parser.parse_args()


def load_manifest(path: Path) -> dict[str, dict[str, str | None]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    tools = payload.get("tool_registry", {}).get("tools", [])
    manifest: dict[str, dict[str, str | None]] = {}
    for tool in tools:
        name = tool.get("name")
        if not isinstance(name, str):
            continue
        manifest[name] = {
            "preferred_executor": tool.get("preferred_executor"),
            "phase": tool.get("phase"),
            "namespace": tool.get("namespace"),
            "tier": tool.get("tier"),
        }
    return manifest


def load_workflow_tools(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    match = re.search(
        r"fn is_workflow_tool\(name: &str\) -> bool \{\s*matches!\(\s*name,\s*(.*?)\)\s*\}",
        text,
        re.DOTALL,
    )
    if not match:
        return set()
    return set(re.findall(r'"([^"]+)"', match.group(1)))


def load_latest_session_rows(analysis_cache_dir: Path) -> dict | None:
    if not analysis_cache_dir.exists():
        return None
    candidates = sorted(
        analysis_cache_dir.glob("analysis-*/session_rows.json"),
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    if not candidates:
        return None
    latest = candidates[0]
    payload = json.loads(latest.read_text(encoding="utf-8"))
    sessions = payload.get("sessions", [])
    role_status_counts: dict[str, Counter[str]] = defaultdict(Counter)
    finding_codes: Counter[str] = Counter()
    for session in sessions:
        if not isinstance(session, dict):
            continue
        role = session.get("role") or "unknown"
        status = session.get("status") or "unknown"
        role_status_counts[str(role)][str(status)] += 1
        for code in session.get("finding_codes", []) or []:
            if isinstance(code, str):
                finding_codes[code] += 1
    return {
        "path": str(latest),
        "session_count": len(sessions),
        "role_status_counts": {role: dict(counts) for role, counts in role_status_counts.items()},
        "top_finding_codes": top_items(finding_codes),
    }


def top_items(counter: Counter, limit: int = TOP_N) -> list[dict[str, object]]:
    return [{"name": name, "count": count} for name, count in counter.most_common(limit)]


def event_meta(event: dict, manifest: dict[str, dict[str, str | None]]) -> dict[str, str | None]:
    tool = str(event.get("tool", ""))
    manifest_meta = manifest.get(tool, {})
    return {
        "preferred_executor": manifest_meta.get("preferred_executor") or "any",
        "phase": manifest_meta.get("phase") or event.get("phase"),
    }


def is_workflow_tool(
    tool: str,
    event: dict,
    manifest: dict[str, dict[str, str | None]],
    workflow_tools: set[str],
) -> bool:
    if workflow_tools:
        return tool in workflow_tools
    meta = manifest.get(tool)
    if meta and meta.get("phase") is not None:
        return True
    return event.get("phase") is not None


def load_events(path: Path) -> tuple[list[dict], int]:
    events: list[dict] = []
    invalid_lines = 0
    if not path.exists():
        return events, invalid_lines
    with path.open(encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            try:
                payload = json.loads(line)
            except json.JSONDecodeError:
                invalid_lines += 1
                continue
            payload["_line_no"] = line_no
            events.append(payload)
    return events, invalid_lines


def analyze_events(
    events: list[dict],
    manifest: dict[str, dict[str, str | None]],
    workflow_tools: set[str],
) -> dict:
    sessions: dict[str, list[dict]] = defaultdict(list)
    for event in events:
        session_id = event.get("session_id") or "<none>"
        sessions[str(session_id)].append(event)

    tool_counts: Counter[str] = Counter()
    failed_tools: Counter[str] = Counter()
    surface_counts: Counter[str] = Counter()
    executor_counts: Counter[str] = Counter()
    phase_counts: Counter[str] = Counter()
    builder_entry_tools: Counter[str] = Counter()
    boundary_sources: Counter[str] = Counter()
    delegate_hint_triggers: Counter[str] = Counter()
    delegate_target_tools: Counter[str] = Counter()
    delegate_consumed_tools: Counter[str] = Counter()
    correlated_delegate_target_tools: Counter[str] = Counter()
    low_level_triplets: Counter[str] = Counter()
    builder_retry_triplets: Counter[str] = Counter()
    session_summaries: list[dict] = []

    total_truncated = 0
    total_failures = 0
    total_boundaries = 0
    total_proxy_accepts = 0
    total_delegate_hint_emitted = 0
    total_low_level_chains = 0
    truncated_followups = 0
    sessions_with_builder_calls = 0

    delegate_emissions_by_handoff: dict[str, dict[str, object]] = {}
    consumed_handoff_ids: set[str] = set()
    correlated_handoff_ids: set[str] = set()
    cross_session_handoff_ids: set[str] = set()

    sorted_events = sorted(
        events,
        key=lambda item: (item.get("timestamp_ms", 0), item["_line_no"]),
    )
    for event in sorted_events:
        session_id = str(event.get("session_id") or "<none>")
        tool = str(event.get("tool", ""))
        preferred = str(event_meta(event, manifest)["preferred_executor"] or "any")

        delegate_handoff_id = event.get("delegate_handoff_id")
        if isinstance(delegate_handoff_id, str) and delegate_handoff_id:
            delegate_emissions_by_handoff.setdefault(
                delegate_handoff_id,
                {
                    "session_id": session_id,
                    "target_tool": event.get("delegate_target_tool"),
                },
            )

        handoff_id = event.get("handoff_id")
        if isinstance(handoff_id, str) and handoff_id and preferred == "codex-builder":
            consumed_handoff_ids.add(handoff_id)
            delegate_consumed_tools[tool] += 1
            emission = delegate_emissions_by_handoff.get(handoff_id)
            if emission is not None:
                correlated_handoff_ids.add(handoff_id)
                if session_id != emission.get("session_id"):
                    cross_session_handoff_ids.add(handoff_id)
                target_tool = emission.get("target_tool")
                if isinstance(target_tool, str) and target_tool:
                    correlated_delegate_target_tools[target_tool] += 1

    for session_id, session_events in sessions.items():
        session_events.sort(key=lambda item: (item.get("timestamp_ms", 0), item["_line_no"]))
        session_builder_calls = 0
        session_boundaries = 0
        session_proxy_accepts = 0
        session_low_level_chains = 0
        previous = None

        for index, event in enumerate(session_events):
            tool = str(event.get("tool", ""))
            meta = event_meta(event, manifest)
            preferred = str(meta["preferred_executor"] or "any")
            phase = meta["phase"]
            workflow = is_workflow_tool(tool, event, manifest, workflow_tools)

            tool_counts[tool] += 1
            surface_counts[str(event.get("surface", "unknown"))] += 1
            executor_counts[preferred] += 1
            phase_counts[str(phase or "<none>")] += 1

            if event.get("truncated"):
                total_truncated += 1
            if not event.get("success", False):
                total_failures += 1
                failed_tools[tool] += 1
            delegate_trigger = event.get("delegate_hint_trigger")
            if isinstance(delegate_trigger, str) and delegate_trigger:
                total_delegate_hint_emitted += 1
                delegate_hint_triggers[delegate_trigger] += 1
            delegate_target_tool = event.get("delegate_target_tool")
            if isinstance(delegate_target_tool, str) and delegate_target_tool:
                delegate_target_tools[delegate_target_tool] += 1

            if preferred == "codex-builder":
                session_builder_calls += 1
                if previous is not None:
                    prev_meta = event_meta(previous, manifest)
                    prev_preferred = str(prev_meta["preferred_executor"] or "any")
                    prev_tool = str(previous.get("tool", ""))
                    if prev_preferred != "codex-builder":
                        total_boundaries += 1
                        session_boundaries += 1
                        boundary_sources[prev_preferred] += 1
                        builder_entry_tools[tool] += 1
                        prev_is_workflow = is_workflow_tool(
                            prev_tool,
                            previous,
                            manifest,
                            workflow_tools,
                        )
                        if prev_preferred == "claude" or prev_is_workflow:
                            total_proxy_accepts += 1
                            session_proxy_accepts += 1

            if previous is not None and previous.get("truncated") and tool != "get_tool_metrics":
                truncated_followups += 1

            if index >= 2:
                triplet = session_events[index - 2 : index + 1]
                triplet_tools = [str(item.get("tool", "")) for item in triplet]
                if all(
                    not is_workflow_tool(name, item, manifest, workflow_tools)
                    for name, item in zip(triplet_tools, triplet)
                ):
                    total_low_level_chains += 1
                    session_low_level_chains += 1
                    low_level_triplets[" -> ".join(triplet_tools)] += 1
                if (
                    triplet_tools[0] == triplet_tools[1] == triplet_tools[2]
                    and all(
                        str(event_meta(item, manifest)["preferred_executor"] or "any")
                        == "codex-builder"
                        for item in triplet
                    )
                ):
                    builder_retry_triplets[triplet_tools[0]] += 1

            previous = event

        if session_builder_calls > 0:
            sessions_with_builder_calls += 1

        session_summaries.append(
            {
                "session_id": session_id,
                "event_count": len(session_events),
                "builder_calls": session_builder_calls,
                "boundary_crossings": session_boundaries,
                "builder_followthrough_proxy": session_proxy_accepts,
                "low_level_chain_count": session_low_level_chains,
            }
        )

    session_summaries.sort(
        key=lambda item: (
            item["boundary_crossings"],
            item["low_level_chain_count"],
            item["event_count"],
        ),
        reverse=True,
    )

    return {
        "event_count": len(events),
        "session_count": len(sessions),
        "sessions_with_builder_calls": sessions_with_builder_calls,
        "failure_count": total_failures,
        "truncated_count": total_truncated,
        "truncated_followup_count": truncated_followups,
        "delegate_hint_emitted_count": total_delegate_hint_emitted,
        "delegate_handoff_emitted_count": len(delegate_emissions_by_handoff),
        "delegate_handoff_consumed_count": len(consumed_handoff_ids),
        "delegate_handoff_correlated_count": len(correlated_handoff_ids),
        "delegate_handoff_cross_session_count": len(cross_session_handoff_ids),
        "boundary_crossings_to_codex_builder": total_boundaries,
        "builder_followthrough_proxy_count": total_proxy_accepts,
        "repeated_low_level_chain_count": total_low_level_chains,
        "tool_counts": top_items(tool_counts),
        "failed_tools": top_items(failed_tools),
        "surface_counts": top_items(surface_counts),
        "executor_counts": top_items(executor_counts),
        "phase_counts": top_items(phase_counts),
        "builder_entry_tools": top_items(builder_entry_tools),
        "boundary_sources": top_items(boundary_sources),
        "delegate_hint_triggers": top_items(delegate_hint_triggers),
        "delegate_target_tools": top_items(delegate_target_tools),
        "delegate_consumed_tools": top_items(delegate_consumed_tools),
        "correlated_delegate_target_tools": top_items(correlated_delegate_target_tools),
        "low_level_triplets": top_items(low_level_triplets),
        "builder_retry_triplets": top_items(builder_retry_triplets),
        "hot_sessions": session_summaries[:TOP_N],
    }


def markdown_section(title: str, rows: list[dict[str, object]], name_key: str = "name") -> str:
    if not rows:
        return f"## {title}\n\n- none\n"
    lines = [f"## {title}", ""]
    for row in rows:
        lines.append(f"- `{row[name_key]}`: {row['count']}")
    lines.append("")
    return "\n".join(lines)


def render_markdown(report: dict) -> str:
    lines = [
        "# Tool Usage Telemetry Analysis",
        "",
        f"- Generated at: `{report['generated_at']}`",
        f"- Telemetry path: `{report['telemetry_path']}`",
        f"- Manifest path: `{report['manifest_path']}`",
        f"- Workflow source: `{report['telemetry_rs_path']}`",
    ]
    if report["telemetry_missing"]:
        lines.extend(
            [
                "",
                "## Status",
                "",
                "- telemetry log not found",
                "- enable with `CODELENS_TELEMETRY_ENABLED=1`",
            ]
        )
    else:
        lines.extend(
            [
                "",
                "## Summary",
                "",
                f"- Events: `{report['analysis']['event_count']}`",
                f"- Sessions: `{report['analysis']['session_count']}`",
                f"- Sessions with builder calls: `{report['analysis']['sessions_with_builder_calls']}`",
                f"- Literal delegate hints emitted: `{report['analysis']['delegate_hint_emitted_count']}`",
                f"- Unique delegate handoff IDs emitted: `{report['analysis']['delegate_handoff_emitted_count']}`",
                f"- Unique delegate handoff IDs consumed by builder tools: `{report['analysis']['delegate_handoff_consumed_count']}`",
                f"- Correlated delegate handoffs: `{report['analysis']['delegate_handoff_correlated_count']}`",
                f"- Cross-session correlated handoffs: `{report['analysis']['delegate_handoff_cross_session_count']}`",
                f"- Boundary crossings to `codex-builder`: `{report['analysis']['boundary_crossings_to_codex_builder']}`",
                f"- Builder follow-through proxy: `{report['analysis']['builder_followthrough_proxy_count']}`",
                f"- Repeated low-level chain count: `{report['analysis']['repeated_low_level_chain_count']}`",
                f"- Failures: `{report['analysis']['failure_count']}`",
                f"- Truncated responses: `{report['analysis']['truncated_count']}`",
                f"- Truncation follow-through: `{report['analysis']['truncated_followup_count']}`",
                f"- Invalid JSONL lines skipped: `{report['invalid_line_count']}`",
                "",
            ]
        )
        lines.append(
            markdown_section("Top Builder Entry Tools", report["analysis"]["builder_entry_tools"])
        )
        lines.append(
            markdown_section(
                "Delegate Hint Triggers",
                report["analysis"]["delegate_hint_triggers"],
            )
        )
        lines.append(
            markdown_section(
                "Delegate Target Tools",
                report["analysis"]["delegate_target_tools"],
            )
        )
        lines.append(
            markdown_section(
                "Builder Tools With Handoff IDs",
                report["analysis"]["delegate_consumed_tools"],
            )
        )
        lines.append(
            markdown_section(
                "Correlated Delegate Target Tools",
                report["analysis"]["correlated_delegate_target_tools"],
            )
        )
        lines.append(
            markdown_section("Boundary Sources", report["analysis"]["boundary_sources"])
        )
        lines.append(
            markdown_section("Repeated Low-Level Triplets", report["analysis"]["low_level_triplets"])
        )
        lines.append(markdown_section("Top Failed Tools", report["analysis"]["failed_tools"]))
        lines.append(markdown_section("Top Tools", report["analysis"]["tool_counts"]))
        lines.append(markdown_section("Executor Counts", report["analysis"]["executor_counts"]))
        lines.append("## Hot Sessions\n")
        if not report["analysis"]["hot_sessions"]:
            lines.append("- none\n")
        else:
            for session in report["analysis"]["hot_sessions"]:
                lines.append(
                    "- `{session_id}`: events={event_count}, boundaries={boundary_crossings}, "
                    "builder_proxy={builder_followthrough_proxy}, low_level_chains={low_level_chain_count}".format(
                        **session
                    )
                )
            lines.append("")

    audit = report.get("latest_session_rows")
    lines.append("## Latest Audit Cache\n")
    if not audit:
        lines.append("- no `session_rows.json` found under `.codelens/analysis-cache`\n")
    else:
        lines.append(f"- Source: `{audit['path']}`")
        lines.append(f"- Session rows: `{audit['session_count']}`")
        for role, counts in sorted(audit["role_status_counts"].items()):
            formatted = ", ".join(f"{status}={count}" for status, count in sorted(counts.items()))
            lines.append(f"- {role} status counts: {formatted}")
        if audit["top_finding_codes"]:
            lines.append("")
            lines.append("### Top Finding Codes")
            lines.append("")
            for row in audit["top_finding_codes"]:
                lines.append(f"- `{row['name']}`: {row['count']}")
        lines.append("")

    lines.extend(
        [
            "## Notes",
            "",
            "- `delegate hint emitted` is now a literal counter from JSONL event fields.",
            "- `correlated delegate handoffs` count shared `handoff_id` values between emitted scaffolds and later `codex-builder` tool calls, including cross-session cases.",
            "- `builder follow-through proxy` remains useful when hosts do not preserve the scaffold `handoff_id`, so it is still reported separately.",
            "- Planner and builder sessions can be separate logical sessions, so correlation should prefer `handoff_id` rather than assume one `session_id`.",
            "",
        ]
    )
    return "\n".join(lines).rstrip() + "\n"


def build_report(args: argparse.Namespace) -> dict:
    telemetry_path = Path(args.telemetry_path)
    manifest_path = Path(args.manifest)
    telemetry_rs_path = Path(args.telemetry_rs)
    analysis_cache_dir = Path(args.analysis_cache)

    manifest = load_manifest(manifest_path)
    workflow_tools = load_workflow_tools(telemetry_rs_path)
    events, invalid_lines = load_events(telemetry_path)
    latest_session_rows = load_latest_session_rows(analysis_cache_dir)

    return {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "telemetry_path": str(telemetry_path),
        "manifest_path": str(manifest_path),
        "telemetry_rs_path": str(telemetry_rs_path),
        "analysis_cache_dir": str(analysis_cache_dir),
        "telemetry_missing": not telemetry_path.exists(),
        "invalid_line_count": invalid_lines,
        "analysis": analyze_events(events, manifest, workflow_tools),
        "latest_session_rows": latest_session_rows,
    }


def run_self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        telemetry_path = Path(tmp) / "tool_usage.jsonl"
        events = [
            {
                "timestamp_ms": 100,
                "tool": "safe_rename_report",
                "surface": "refactor-full",
                "elapsed_ms": 10,
                "tokens": 100,
                "success": True,
                "truncated": False,
                "session_id": "planner-a",
                "phase": "review",
                "suggested_next_tools": ["delegate_to_codex_builder", "rename_symbol"],
                "delegate_hint_trigger": "preferred_executor_boundary",
                "delegate_target_tool": "rename_symbol",
                "delegate_handoff_id": "codelens-handoff-test-1",
            },
            {
                "timestamp_ms": 200,
                "tool": "rename_symbol",
                "surface": "refactor-full",
                "elapsed_ms": 30,
                "tokens": 140,
                "success": True,
                "truncated": False,
                "session_id": "builder-b",
                "phase": "build",
                "handoff_id": "codelens-handoff-test-1",
            },
        ]
        telemetry_path.write_text(
            "".join(json.dumps(event) + "\n" for event in events),
            encoding="utf-8",
        )
        manifest = load_manifest(DEFAULT_MANIFEST_PATH)
        workflow_tools = load_workflow_tools(DEFAULT_TELEMETRY_RS_PATH)
        loaded_events, invalid_lines = load_events(telemetry_path)
        assert invalid_lines == 0
        analysis = analyze_events(loaded_events, manifest, workflow_tools)
        assert analysis["delegate_hint_emitted_count"] == 1, analysis
        assert analysis["delegate_handoff_emitted_count"] == 1, analysis
        assert analysis["delegate_handoff_consumed_count"] == 1, analysis
        assert analysis["delegate_handoff_correlated_count"] == 1, analysis
        assert analysis["delegate_handoff_cross_session_count"] == 1, analysis
        assert analysis["delegate_consumed_tools"][0]["name"] == "rename_symbol", analysis
        assert (
            analysis["correlated_delegate_target_tools"][0]["name"] == "rename_symbol"
        ), analysis
    print("analyze-tool-usage: self-test ok")
    return 0


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()
    report = build_report(args)
    if args.format == "json":
        rendered = json.dumps(report, indent=2) + "\n"
    else:
        rendered = render_markdown(report)
    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
