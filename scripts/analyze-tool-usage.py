#!/usr/bin/env python3
"""
Analyze get_tool_metrics JSON to identify zero-call (dead) tools and usage patterns.

Usage:
    python3 scripts/analyze-tool-usage.py < metrics.json
    python3 scripts/analyze-tool-usage.py metrics.json

Outputs:
    - Zero-call tools (candidates for removal)
    - Call coverage ratio
    - Hot vs cold tool distribution
    - Recommendations for surface diet
"""

import json
import sys
from collections import Counter


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
        print(f"\n  ⚠️  ZERO-CALL TOOLS (candidates for removal):")
        for name in zero_call:
            print(f"      • {name}")

    if tools:
        sorted_tools = sorted(tools, key=lambda t: t.get("calls", 0), reverse=True)
        total_calls = sum(t.get("calls", 0) for t in tools)

        print(f"\n  🔥 TOP 5 HOT TOOLS:")
        for t in sorted_tools[:5]:
            pct = (t["calls"] / total_calls * 100) if total_calls else 0
            print(
                f"      {t['tool']:30} {t['calls']:4} calls ({pct:5.1f}%)"
            )

        print(f"\n  🥶 TOP 5 COLD TOOLS (called but barely):")
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

        print(f"\n  📊 CALL DISTRIBUTION:")
        for label in ["zero", "1-5", "6-20", "21-100", "100+"]:
            if label in buckets:
                print(f"      {label:8}: {buckets[label]:3} tools")

    # Recommendations
    print(f"\n  💡 RECOMMENDATIONS:")
    if coverage < 0.5:
        print("      • Coverage < 50%: Strong candidate for aggressive tool surface diet.")
    elif coverage < 0.7:
        print("      • Coverage < 70%: Consider removing zero-call tools or moving them to opt-in feature gates.")
    else:
        print("      • Coverage >= 70%: Surface is reasonably utilized. Monitor cold tools for deprecation.")

    if len(zero_call) > 10:
        print(f"      • {len(zero_call)} zero-call tools: Review if they serve niche use-cases or are stale.")

    print(f"{'=' * 60}\n")


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] not in ("-h", "--help"):
        with open(sys.argv[1]) as f:
            data = json.load(f)
    else:
        data = json.load(sys.stdin)

    # Handle nested JSON-RPC result wrapper if present
    if "result" in data and isinstance(data["result"], dict):
        data = data["result"]
    if "content" in data and isinstance(data["content"], list):
        # MCP resource/textContent wrapper
        for item in data["content"]:
            if item.get("type") == "text":
                data = json.loads(item["text"])
                break

    analyze(data)


if __name__ == "__main__":
    main()
