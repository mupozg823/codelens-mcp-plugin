#!/usr/bin/env python3
"""
Analyze CodeLens tool usage from get_tool_metrics JSON, telemetry JSONL, or Codex rollout JSONL.

Usage:
    python3 scripts/analyze-tool-usage.py < metrics.json
    python3 scripts/analyze-tool-usage.py metrics.json
    python3 scripts/analyze-tool-usage.py .codelens/telemetry/tool_usage.jsonl
    python3 scripts/analyze-tool-usage.py --telemetry-path .codelens/telemetry/tool_usage.jsonl
    python3 scripts/analyze-tool-usage.py --codex-rollout-path ~/.codex/memories/rollout_summaries
    python3 scripts/analyze-tool-usage.py --format json --output /tmp/codelens-telemetry.json

Outputs:
    - Zero-call tools (candidates for removal)
    - Call coverage ratio
    - Hot vs cold tool distribution
    - Suggested-next-tool follow-through and missed-route labels for telemetry/rollouts
    - Recommendations for surface diet
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

from analyze_tool_usage_lib import (
    DEFAULT_MANIFEST_PATH,
    DEFAULT_TELEMETRY_PATH,
    analyze_telemetry,
    load_telemetry,
    str_value,
)
from analyze_tool_usage_render import render_telemetry_report
from codex_rollout_usage import load_codex_rollout_events, rollout_files


def analyze(data: dict) -> None:
    tools = data.get("tools", data.get("per_tool", []))
    zero_call = data.get("zero_call_tools", [])
    registered = data.get("registered_count", 0)
    coverage = data.get("call_coverage", 0.0)

    print(f"\n{'=' * 60}")
    print("  CodeLens Tool Surface Usage Analysis")
    print(f"{'=' * 60}")
    print(f"  Registered tools : {registered}")
    print(f"  Called tools     : {len(tools)}")
    print(f"  Zero-call tools  : {len(zero_call)}")
    print(f"  Call coverage    : {coverage * 100:.1f}%")

    if zero_call:
        print("\n  ⚠️  ZERO-CALL TOOLS (candidates for removal):")
        for name in zero_call:
            print(f"      • {name}")

    if tools:
        sorted_tools = sorted(tools, key=lambda t: t.get("calls", 0), reverse=True)
        total_calls = sum(t.get("calls", 0) for t in tools)

        print("\n  🔥 TOP 5 HOT TOOLS:")
        for t in sorted_tools[:5]:
            pct = (t["calls"] / total_calls * 100) if total_calls else 0
            print(
                f"      {t['tool']:30} {t['calls']:4} calls ({pct:5.1f}%)"
            )

        print("\n  🥶 TOP 5 COLD TOOLS (called but barely):")
        for t in sorted_tools[-5:]:
            pct = (t["calls"] / total_calls * 100) if total_calls else 0
            print(
                f"      {t['tool']:30} {t['calls']:4} calls ({pct:5.1f}%)"
            )

        # Distribution buckets
        buckets = Counter()
        # Zero-call tools come from a separate field, not from per_tool list
        buckets["zero"] = len(zero_call)
        for t in tools:
            c = t.get("calls", 0)
            if c <= 5:
                buckets["1-5"] += 1
            elif c <= 20:
                buckets["6-20"] += 1
            elif c <= 100:
                buckets["21-100"] += 1
            else:
                buckets["100+"] += 1

        print("\n  📊 CALL DISTRIBUTION:")
        for label in ["zero", "1-5", "6-20", "21-100", "100+"]:
            if label in buckets:
                print(f"      {label:8}: {buckets[label]:3} tools")

    # Recommendations
    print("\n  💡 RECOMMENDATIONS:")
    if coverage < 0.5:
        print("      • Coverage < 50%: Strong candidate for aggressive tool surface diet.")
    elif coverage < 0.7:
        print("      • Coverage < 70%: Consider removing zero-call tools or moving them to opt-in feature gates.")
    else:
        print("      • Coverage >= 70%: Surface is reasonably utilized. Monitor cold tools for deprecation.")

    if len(zero_call) > 10:
        print(f"      • {len(zero_call)} zero-call tools: Review if they serve niche use-cases or are stale.")

    print(f"{'=' * 60}\n")


def unwrap_mcp_data(data: dict) -> dict:
    if "result" in data and isinstance(data["result"], dict):
        data = data["result"]
    if "content" in data and isinstance(data["content"], list):
        for item in data["content"]:
            if isinstance(item, dict) and item.get("type") == "text":
                text = str_value(item.get("text"))
                if text is not None:
                    parsed = json.loads(text)
                    if isinstance(parsed, dict):
                        data = parsed
                break
    return data


def read_metrics_json(path: Path | None) -> dict | None:
    if path is not None:
        with path.open(encoding="utf-8") as handle:
            parsed = json.load(handle)
    else:
        raw_input = sys.stdin.read()
        if not raw_input.strip():
            return None
        parsed = json.loads(raw_input)
    if not isinstance(parsed, dict):
        raise SystemExit("metrics input must be a JSON object")
    return unwrap_mcp_data(parsed)


def write_or_print(payload: str, output: Path | None) -> None:
    if output is not None:
        output.write_text(payload, encoding="utf-8")
    else:
        print(payload)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", nargs="?")
    parser.add_argument("--telemetry-path", type=Path)
    parser.add_argument("--session-id")
    parser.add_argument("--codex-rollout-path", type=Path, action="append")
    parser.add_argument("--manifest-path", type=Path, default=DEFAULT_MANIFEST_PATH)
    parser.add_argument("--format", choices=["text", "json"], default="text")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    if args.codex_rollout_path:
        rollout_paths: list[Path] = []
        for path in args.codex_rollout_path:
            rollout_paths.extend(rollout_files(path))
        report = analyze_telemetry(
            load_codex_rollout_events(rollout_paths),
            args.manifest_path,
        )
        if args.format == "json":
            write_or_print(json.dumps(report, indent=2, sort_keys=True), args.output)
        else:
            render_telemetry_report(report)
        return

    telemetry_path = args.telemetry_path
    input_path = Path(args.input) if args.input else None
    if telemetry_path is None and input_path is not None and input_path.suffix == ".jsonl":
        telemetry_path = input_path
        input_path = None
    if telemetry_path is None and input_path is None and DEFAULT_TELEMETRY_PATH.exists():
        telemetry_path = DEFAULT_TELEMETRY_PATH

    if telemetry_path is not None:
        events = load_telemetry(telemetry_path)
        if args.session_id is not None:
            events = [event for event in events if event.get("session_id") == args.session_id]
        report = analyze_telemetry(events, args.manifest_path)
        if args.format == "json":
            write_or_print(json.dumps(report, indent=2, sort_keys=True), args.output)
        else:
            render_telemetry_report(report)
        return

    data = read_metrics_json(input_path)
    if data is None:
        report = analyze_telemetry([], args.manifest_path)
        if args.format == "json":
            write_or_print(json.dumps(report, indent=2, sort_keys=True), args.output)
        else:
            render_telemetry_report(report)
        return
    if args.format == "json":
        write_or_print(json.dumps(data, indent=2, sort_keys=True), args.output)
    else:
        analyze(data)


if __name__ == "__main__":
    main()
