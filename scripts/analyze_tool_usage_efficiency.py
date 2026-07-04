from __future__ import annotations

from collections import defaultdict

from analyze_tool_usage_branches import BRANCHES


def int_metric(value) -> int:
    return value if isinstance(value, int) else 0


def estimate_tokens(chars: int) -> int:
    return (chars + 3) // 4 if chars > 0 else 0


def efficiency_band(total_chars: int, overflow_count: int) -> str:
    if overflow_count > 0:
        return "overflow"
    if total_chars <= 1_200:
        return "compact"
    if total_chars <= 6_000:
        return "moderate"
    return "expensive"


def external_transfer(event: dict) -> dict:
    call_chars = int_metric(event.get("next_external_call_chars"))
    result_chars = int_metric(event.get("next_external_result_chars"))
    overflow_count = int_metric(event.get("next_external_overflow_count"))
    tool_count = int_metric(event.get("next_external_tool_count"))
    if tool_count == 0:
        tool_count = len(event.get("next_external_tools", []))
    total_chars = call_chars + result_chars
    return {
        "tool_count": tool_count,
        "call_chars": call_chars,
        "result_chars": result_chars,
        "total_chars": total_chars,
        "estimated_tokens": estimate_tokens(total_chars),
        "overflow_count": overflow_count,
        "efficiency_band": efficiency_band(total_chars, overflow_count),
    }


def branch_transfer(event: dict, branch: str) -> dict:
    if branch not in BRANCHES:
        return {
            "tool_count": 0,
            "call_chars": 0,
            "result_chars": 0,
            "total_chars": 0,
            "estimated_tokens": 0,
            "overflow_count": 0,
            "efficiency_band": "compact",
        }
    call_chars = int_metric(event.get(f"next_{branch}_call_chars"))
    result_chars = int_metric(event.get(f"next_{branch}_result_chars"))
    overflow_count = int_metric(event.get(f"next_{branch}_overflow_count"))
    tool_count = int_metric(event.get(f"next_{branch}_tool_count"))
    total_chars = call_chars + result_chars
    return {
        "tool_count": tool_count,
        "call_chars": call_chars,
        "result_chars": result_chars,
        "total_chars": total_chars,
        "estimated_tokens": estimate_tokens(total_chars),
        "overflow_count": overflow_count,
        "efficiency_band": efficiency_band(total_chars, overflow_count),
    }


def branch_transfers(event: dict) -> dict[str, dict]:
    return {branch: branch_transfer(event, branch) for branch in BRANCHES}


def summarize_transfers(rows: list[dict], group_key: str = "route_label") -> list[dict]:
    totals: dict[str, dict] = defaultdict(
        lambda: {
            "count": 0,
            "total_chars": 0,
            "estimated_tokens": 0,
            "overflow_count": 0,
            "tool_count": 0,
            "expensive_count": 0,
            "max_estimated_tokens": 0,
        }
    )
    for row in rows:
        label = row[group_key]
        transfer = row["external_transfer"]
        bucket = totals[label]
        bucket["count"] += 1
        bucket["total_chars"] += transfer["total_chars"]
        bucket["estimated_tokens"] += transfer["estimated_tokens"]
        bucket["overflow_count"] += transfer["overflow_count"]
        bucket["tool_count"] += transfer["tool_count"]
        if transfer["efficiency_band"] in {"expensive", "overflow"}:
            bucket["expensive_count"] += 1
        bucket["max_estimated_tokens"] = max(
            bucket["max_estimated_tokens"],
            transfer["estimated_tokens"],
        )
    return [
        {
            group_key: label,
            "count": values["count"],
            "avg_total_chars": values["total_chars"] // values["count"],
            "avg_estimated_tokens": values["estimated_tokens"] // values["count"],
            "max_estimated_tokens": values["max_estimated_tokens"],
            "overflow_count": values["overflow_count"],
            "expensive_count": values["expensive_count"],
            "avg_tool_count": values["tool_count"] / values["count"],
        }
        for label, values in sorted(
            totals.items(),
            key=lambda item: (-item[1]["count"], item[0]),
        )
    ]


def top_transfer_rows(rows: list[dict]) -> list[dict]:
    return sorted(
        rows,
        key=lambda row: row["external_transfer"]["estimated_tokens"],
        reverse=True,
    )[:5]
