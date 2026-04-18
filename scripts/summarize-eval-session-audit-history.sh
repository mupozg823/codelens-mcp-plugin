#!/bin/bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/summarize-eval-session-audit-history.sh [output-path] [options]

Summarize daemon-wide `eval_session_audit` JSON snapshots over time so an
operator can see drift and trend signals across recent days.

Options:
  --input-dir DIR     Directory containing `eval-session-audit-*.json` snapshots
                      (default: .codelens/reports/daily)
  --limit N           Number of most recent snapshots to analyze (default: 14)
  --format FMT        Output format: markdown or json (default: markdown)
  -h, --help          Show this help

Examples:
  bash scripts/summarize-eval-session-audit-history.sh
  bash scripts/summarize-eval-session-audit-history.sh --limit 7
  bash scripts/summarize-eval-session-audit-history.sh .codelens/reports/daily/latest-summary.md
  bash scripts/summarize-eval-session-audit-history.sh --format json
EOF
}

INPUT_DIR="${CODELENS_AUDIT_INPUT_DIR:-.codelens/reports/daily}"
LIMIT="${CODELENS_AUDIT_HISTORY_LIMIT:-14}"
OUTPUT_FORMAT="${CODELENS_AUDIT_HISTORY_FORMAT:-markdown}"
OUTPUT_PATH=""

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--input-dir)
		INPUT_DIR="${2:-}"
		shift 2
		;;
	--limit)
		LIMIT="${2:-}"
		shift 2
		;;
	--format)
		OUTPUT_FORMAT="${2:-}"
		shift 2
		;;
	-*)
		echo "unknown option: $1" >&2
		usage >&2
		exit 2
		;;
	*)
		if [[ -n "$OUTPUT_PATH" ]]; then
			echo "multiple output paths provided" >&2
			usage >&2
			exit 2
		fi
		OUTPUT_PATH="$1"
		shift
		;;
	esac
done

case "$OUTPUT_FORMAT" in
markdown | json) ;;
*)
	echo "--format must be one of: markdown, json" >&2
	exit 2
	;;
esac

if ! [[ "$LIMIT" =~ ^[0-9]+$ ]] || [[ "$LIMIT" == "0" ]]; then
	echo "--limit must be a positive integer" >&2
	exit 2
fi

python3 - "$INPUT_DIR" "$LIMIT" "$OUTPUT_FORMAT" "$OUTPUT_PATH" <<'PY'
import json
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

input_dir, limit_raw, output_format, output_path = sys.argv[1:]
limit = int(limit_raw)


def parse_iso(value: str | None) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        if value.endswith("Z"):
            return datetime.fromisoformat(value.replace("Z", "+00:00"))
        return datetime.fromisoformat(value)
    except ValueError:
        return None


def fmt_pct(value):
    if value is None:
        return "n/a"
    return f"{value * 100:.1f}%"


def fmt_delta(value):
    if value is None:
        return "n/a"
    sign = "+" if value >= 0 else ""
    return f"{sign}{value * 100:.1f}pp"


def collect_rate_series(snapshots, key):
    values = []
    for snapshot in snapshots:
        value = snapshot["audit_pass_rate"].get(key)
        values.append(value if isinstance(value, (int, float)) else None)
    return values


def rate_summary(snapshots, key):
    series = collect_rate_series(snapshots, key)
    observed = [value for value in series if value is not None]
    latest = series[-1] if series else None
    previous = None
    for value in reversed(series[:-1]):
        if value is not None:
            previous = value
            break
    delta = None if latest is None or previous is None else latest - previous
    return {
        "latest": latest,
        "previous": previous,
        "delta_vs_previous": delta,
        "average": statistics.mean(observed) if observed else None,
        "minimum": min(observed) if observed else None,
        "maximum": max(observed) if observed else None,
        "observed_snapshot_count": len(observed),
        "snapshot_count": len(series),
    }


def latest_flagged_sessions(snapshot):
    flagged = []
    for session in snapshot.get("session_rows", {}).get("sessions", []):
        status = session.get("status")
        if status in {"warn", "fail"}:
            flagged.append(
                {
                    "session_id": session.get("session_id"),
                    "role": session.get("role"),
                    "status": status,
                    "surface": session.get("surface"),
                    "transport": session.get("transport"),
                    "finding_codes": session.get("finding_codes") or [],
                }
            )
    return flagged


def recurring_failed_checks(snapshots):
    rollup = defaultdict(lambda: {"total_count": 0, "snapshot_count": 0})
    for snapshot in snapshots:
        seen = set()
        for item in snapshot.get("audit_pass_rate", {}).get("top_failed_checks", []):
            code = item.get("code")
            count = item.get("count")
            if not isinstance(code, str) or not isinstance(count, int):
                continue
            rollup[code]["total_count"] += count
            if code not in seen:
                rollup[code]["snapshot_count"] += 1
                seen.add(code)
    return [
        {
            "code": code,
            "total_count": values["total_count"],
            "snapshot_count": values["snapshot_count"],
        }
        for code, values in sorted(
            rollup.items(),
            key=lambda item: (-item[1]["total_count"], -item[1]["snapshot_count"], item[0]),
        )
    ]


def coverage_summary(snapshots):
    no_applicable = 0
    no_builder = 0
    no_planner = 0
    for snapshot in snapshots:
        audit = snapshot["audit_pass_rate"]
        if int(audit.get("session_count") or 0) == 0:
            no_applicable += 1
        if int(audit.get("builder_session_count") or 0) == 0:
            no_builder += 1
        if int(audit.get("planner_session_count") or 0) == 0:
            no_planner += 1
    latest = snapshots[-1]["audit_pass_rate"]
    return {
        "snapshot_count": len(snapshots),
        "no_applicable_snapshots": no_applicable,
        "no_builder_coverage_snapshots": no_builder,
        "no_planner_coverage_snapshots": no_planner,
        "latest_tracked_session_count": int(latest.get("tracked_session_count") or 0),
        "latest_applicable_session_count": int(latest.get("session_count") or 0),
        "latest_skipped_session_count": int(latest.get("skipped_session_count") or 0),
        "latest_builder_session_count": int(latest.get("builder_session_count") or 0),
        "latest_planner_session_count": int(latest.get("planner_session_count") or 0),
    }


def detect_drift_flags(snapshots, builder, planner, recurring):
    flags = []
    latest = snapshots[-1]["audit_pass_rate"]
    if int(latest.get("session_count") or 0) == 0:
        flags.append("latest snapshot has no applicable audited sessions")
    if builder["delta_vs_previous"] is not None and builder["delta_vs_previous"] <= -0.20:
        flags.append(
            f"builder pass rate dropped {fmt_delta(builder['delta_vs_previous'])} versus previous observed snapshot"
        )
    if planner["delta_vs_previous"] is not None and planner["delta_vs_previous"] <= -0.20:
        flags.append(
            f"planner pass rate dropped {fmt_delta(planner['delta_vs_previous'])} versus previous observed snapshot"
        )

    latest_codes = {
        item.get("code")
        for item in latest.get("top_failed_checks", [])
        if isinstance(item.get("code"), str)
    }
    prior_codes = set()
    for snapshot in snapshots[:-1]:
        prior_codes.update(
            item.get("code")
            for item in snapshot.get("audit_pass_rate", {}).get("top_failed_checks", [])
            if isinstance(item.get("code"), str)
        )
    new_codes = sorted(code for code in latest_codes if code not in prior_codes)
    if new_codes:
        flags.append(
            "new failed checks in latest snapshot: "
            + ", ".join(f"`{code}`" for code in new_codes)
        )
    if recurring:
        hottest = recurring[0]
        if hottest["snapshot_count"] >= 2:
            flags.append(
                f"recurring failed check `{hottest['code']}` appears in {hottest['snapshot_count']} snapshots"
            )
    if not flags:
        flags.append("no material drift detected in the selected snapshot window")
    return flags


def render_markdown(summary):
    lines = [
        "# CodeLens eval_session_audit trend summary",
        "",
        f"- Generated at: `{summary['generated_at']}`",
        f"- Input dir: `{summary['input_dir']}`",
        f"- Snapshots analyzed: `{summary['snapshot_count']}`",
        f"- Window: `{summary['window_start']}` -> `{summary['window_end']}`",
        f"- Latest snapshot: `{summary['latest_snapshot_path']}`",
    ]
    if summary["skipped_files"]:
        lines.append(
            "- Skipped files: "
            + ", ".join(f"`{item}`" for item in summary["skipped_files"])
        )
    lines.extend(
        [
            "",
            "## Pass-Rate Trends",
            "",
            f"- Builder: latest `{fmt_pct(summary['builder']['latest'])}`, delta `{fmt_delta(summary['builder']['delta_vs_previous'])}`, avg `{fmt_pct(summary['builder']['average'])}`, min `{fmt_pct(summary['builder']['minimum'])}`, max `{fmt_pct(summary['builder']['maximum'])}`, coverage `{summary['builder']['observed_snapshot_count']}/{summary['builder']['snapshot_count']}`",
            f"- Planner: latest `{fmt_pct(summary['planner']['latest'])}`, delta `{fmt_delta(summary['planner']['delta_vs_previous'])}`, avg `{fmt_pct(summary['planner']['average'])}`, min `{fmt_pct(summary['planner']['minimum'])}`, max `{fmt_pct(summary['planner']['maximum'])}`, coverage `{summary['planner']['observed_snapshot_count']}/{summary['planner']['snapshot_count']}`",
            "",
            "## Coverage Drift",
            "",
            f"- Latest tracked/applicable/skipped sessions: `{summary['coverage']['latest_tracked_session_count']}` / `{summary['coverage']['latest_applicable_session_count']}` / `{summary['coverage']['latest_skipped_session_count']}`",
            f"- Latest builder/planner applicable sessions: `{summary['coverage']['latest_builder_session_count']}` / `{summary['coverage']['latest_planner_session_count']}`",
            f"- Snapshots with no applicable audited sessions: `{summary['coverage']['no_applicable_snapshots']}/{summary['coverage']['snapshot_count']}`",
            f"- Snapshots with no builder coverage: `{summary['coverage']['no_builder_coverage_snapshots']}/{summary['coverage']['snapshot_count']}`",
            f"- Snapshots with no planner coverage: `{summary['coverage']['no_planner_coverage_snapshots']}/{summary['coverage']['snapshot_count']}`",
            "",
            "## Recurring Failed Checks",
            "",
        ]
    )
    recurring = summary["recurring_failed_checks"]
    if recurring:
        for item in recurring[:8]:
            lines.append(
                f"- `{item['code']}`: total=`{item['total_count']}`, snapshots=`{item['snapshot_count']}/{summary['snapshot_count']}`"
            )
    else:
        lines.append("- _none_")

    lines.extend(["", "## Latest Flagged Sessions", ""])
    flagged = summary["latest_flagged_sessions"]
    if flagged:
        for item in flagged:
            codes = item["finding_codes"] or []
            lines.append(
                f"- `{item['session_id']}` role=`{item['role']}` status=`{item['status']}` surface=`{item['surface']}` transport=`{item['transport']}` findings="
                + (", ".join(f"`{code}`" for code in codes) if codes else "_none_")
            )
    else:
        lines.append("- _none_")

    lines.extend(["", "## Drift Flags", ""])
    for flag in summary["drift_flags"]:
        lines.append(f"- {flag}")
    return "\n".join(lines).rstrip() + "\n"


input_path = Path(input_dir)
paths = sorted(input_path.glob("eval-session-audit-*.json"))
if limit:
    paths = paths[-limit:]
if not paths:
    raise SystemExit(f"no eval-session-audit JSON snapshots found under {input_dir}")

snapshots = []
skipped_files = []
for path in paths:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        skipped_files.append(str(path))
        continue
    audit = payload.get("audit_pass_rate")
    rows = payload.get("session_rows")
    if not isinstance(audit, dict) or not isinstance(rows, dict):
        skipped_files.append(str(path))
        continue
    generated_at = payload.get("generated_at")
    generated_dt = parse_iso(generated_at)
    snapshots.append(
        {
            "path": str(path),
            "generated_at": generated_at,
            "generated_dt": generated_dt,
            "audit_pass_rate": audit,
            "session_rows": rows,
        }
    )

if not snapshots:
    raise SystemExit("no valid eval_session_audit snapshots were readable")

snapshots.sort(key=lambda item: (item["generated_dt"] or datetime.min.replace(tzinfo=timezone.utc), item["path"]))
builder = rate_summary(snapshots, "builder_pass_rate")
planner = rate_summary(snapshots, "planner_pass_rate")
recurring = recurring_failed_checks(snapshots)
coverage = coverage_summary(snapshots)
summary = {
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "input_dir": str(input_path),
    "snapshot_count": len(snapshots),
    "window_start": snapshots[0]["generated_at"] or snapshots[0]["path"],
    "window_end": snapshots[-1]["generated_at"] or snapshots[-1]["path"],
    "latest_snapshot_path": snapshots[-1]["path"],
    "skipped_files": skipped_files,
    "builder": builder,
    "planner": planner,
    "coverage": coverage,
    "recurring_failed_checks": recurring,
    "latest_flagged_sessions": latest_flagged_sessions(snapshots[-1]),
}
summary["drift_flags"] = detect_drift_flags(snapshots, builder, planner, recurring)

if output_format == "json":
    rendered = json.dumps(summary, indent=2) + "\n"
else:
    rendered = render_markdown(summary)

if output_path:
    output_file = Path(output_path)
    output_file.parent.mkdir(parents=True, exist_ok=True)
    output_file.write_text(rendered, encoding="utf-8")
    print(str(output_file))
else:
    sys.stdout.write(rendered)
PY
