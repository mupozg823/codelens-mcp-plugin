#!/bin/bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/eval-session-audit-operator-gate.sh [output-path] [options]

Evaluate recent daemon-wide `eval_session_audit` history and classify it as
`pass`, `warn`, or `fail` for operator workflows. This script reuses
`summarize-eval-session-audit-history.sh` as its data source instead of
re-implementing snapshot parsing.

Options:
  --input-dir DIR               Directory containing `eval-session-audit-*.json`
                                snapshots (default: .codelens/reports/daily)
  --limit N                     Number of most recent snapshots to analyze
                                (default: 14)
  --format FMT                  Output format: markdown or json
                                (default: markdown)
  --min-builder-pass-rate VAL   Minimum latest builder pass rate before fail
                                (default: 0.5, use `off` to disable)
  --min-planner-pass-rate VAL   Minimum latest planner pass rate before fail
                                (default: 0.5, use `off` to disable)
  --max-no-applicable N         Maximum snapshots with zero applicable audited
                                sessions before fail (default: 0)
  --fail-on-warn                Exit non-zero on warn as well as fail
  -h, --help                    Show this help

Examples:
  bash scripts/eval-session-audit-operator-gate.sh
  bash scripts/eval-session-audit-operator-gate.sh --limit 7
  bash scripts/eval-session-audit-operator-gate.sh --fail-on-warn
  bash scripts/eval-session-audit-operator-gate.sh --format json
EOF
}

INPUT_DIR="${CODELENS_AUDIT_INPUT_DIR:-.codelens/reports/daily}"
LIMIT="${CODELENS_AUDIT_HISTORY_LIMIT:-14}"
OUTPUT_FORMAT="${CODELENS_AUDIT_GATE_FORMAT:-markdown}"
MIN_BUILDER_PASS_RATE="${CODELENS_AUDIT_GATE_MIN_BUILDER_PASS_RATE:-0.5}"
MIN_PLANNER_PASS_RATE="${CODELENS_AUDIT_GATE_MIN_PLANNER_PASS_RATE:-0.5}"
MAX_NO_APPLICABLE="${CODELENS_AUDIT_GATE_MAX_NO_APPLICABLE:-0}"
FAIL_ON_WARN=0
OUTPUT_PATH=""
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUMMARY_SCRIPT="$SCRIPT_DIR/summarize-eval-session-audit-history.sh"

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
	--min-builder-pass-rate)
		MIN_BUILDER_PASS_RATE="${2:-}"
		shift 2
		;;
	--min-planner-pass-rate)
		MIN_PLANNER_PASS_RATE="${2:-}"
		shift 2
		;;
	--max-no-applicable)
		MAX_NO_APPLICABLE="${2:-}"
		shift 2
		;;
	--fail-on-warn)
		FAIL_ON_WARN=1
		shift
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

if ! [[ "$MAX_NO_APPLICABLE" =~ ^[0-9]+$ ]]; then
	echo "--max-no-applicable must be a non-negative integer" >&2
	exit 2
fi

if [[ ! -x "$SUMMARY_SCRIPT" ]]; then
	echo "missing executable summary script: $SUMMARY_SCRIPT" >&2
	exit 1
fi

SUMMARY_JSON="$(
	bash "$SUMMARY_SCRIPT" \
		--input-dir "$INPUT_DIR" \
		--limit "$LIMIT" \
		--format json
)"

SUMMARY_JSON_FILE="$(mktemp)"
trap 'rm -f "$SUMMARY_JSON_FILE"' EXIT
printf '%s\n' "$SUMMARY_JSON" >"$SUMMARY_JSON_FILE"

python3 - "$SUMMARY_JSON_FILE" "$OUTPUT_FORMAT" "$OUTPUT_PATH" "$FAIL_ON_WARN" "$MIN_BUILDER_PASS_RATE" "$MIN_PLANNER_PASS_RATE" "$MAX_NO_APPLICABLE" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    summary_json_path,
    output_format,
    output_path,
    fail_on_warn_raw,
    min_builder_raw,
    min_planner_raw,
    max_no_applicable_raw,
) = sys.argv[1:]
summary = json.loads(Path(summary_json_path).read_text(encoding="utf-8"))
fail_on_warn = fail_on_warn_raw == "1"
max_no_applicable = int(max_no_applicable_raw)


def parse_threshold(value: str):
    if value == "off":
        return None
    try:
        parsed = float(value)
    except ValueError as exc:
        raise SystemExit(f"invalid threshold `{value}`") from exc
    if parsed < 0 or parsed > 1:
        raise SystemExit(f"threshold `{value}` must be between 0 and 1 inclusive")
    return parsed


def fmt_pct(value):
    if value is None:
        return "n/a"
    return f"{value * 100:.1f}%"


min_builder = parse_threshold(min_builder_raw)
min_planner = parse_threshold(min_planner_raw)

fail_reasons = []
warn_reasons = []
coverage = summary["coverage"]
builder = summary["builder"]
planner = summary["planner"]
drift_flags = summary.get("drift_flags", [])
recurring = summary.get("recurring_failed_checks", [])

if coverage["latest_applicable_session_count"] == 0:
    fail_reasons.append("latest snapshot has zero applicable audited sessions")
if coverage["no_applicable_snapshots"] > max_no_applicable:
    fail_reasons.append(
        f"no-applicable snapshots {coverage['no_applicable_snapshots']} exceed threshold {max_no_applicable}"
    )
if (
    min_builder is not None
    and builder["latest"] is not None
    and builder["latest"] < min_builder
):
    fail_reasons.append(
        f"latest builder pass rate {fmt_pct(builder['latest'])} is below threshold {fmt_pct(min_builder)}"
    )
if (
    min_planner is not None
    and planner["latest"] is not None
    and planner["latest"] < min_planner
):
    fail_reasons.append(
        f"latest planner pass rate {fmt_pct(planner['latest'])} is below threshold {fmt_pct(min_planner)}"
    )

if coverage["no_builder_coverage_snapshots"] > 0:
    warn_reasons.append(
        f"builder coverage missing in {coverage['no_builder_coverage_snapshots']}/{coverage['snapshot_count']} snapshots"
    )
if coverage["no_planner_coverage_snapshots"] > 0:
    warn_reasons.append(
        f"planner coverage missing in {coverage['no_planner_coverage_snapshots']}/{coverage['snapshot_count']} snapshots"
    )
for flag in drift_flags:
    if flag != "no material drift detected in the selected snapshot window":
        warn_reasons.append(flag)
if recurring:
    hottest = recurring[0]
    if hottest["snapshot_count"] >= 2:
        warn_reasons.append(
            f"recurring failed check `{hottest['code']}` spans {hottest['snapshot_count']} snapshots"
        )

if fail_reasons:
    status = "fail"
elif warn_reasons:
    status = "warn"
else:
    status = "pass"

payload = {
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "status": status,
    "fail_reasons": fail_reasons,
    "warn_reasons": warn_reasons,
    "thresholds": {
        "min_builder_pass_rate": min_builder,
        "min_planner_pass_rate": min_planner,
        "max_no_applicable_snapshots": max_no_applicable,
        "fail_on_warn": fail_on_warn,
    },
    "summary": summary,
}


def render_markdown(data):
    lines = [
        "# CodeLens eval_session_audit operator gate",
        "",
        f"- Generated at: `{data['generated_at']}`",
        f"- Status: `{data['status']}`",
        f"- Snapshot window: `{data['summary']['window_start']}` -> `{data['summary']['window_end']}`",
        f"- Latest snapshot: `{data['summary']['latest_snapshot_path']}`",
        "",
        "## Thresholds",
        "",
        f"- min_builder_pass_rate: `{fmt_pct(data['thresholds']['min_builder_pass_rate']) if data['thresholds']['min_builder_pass_rate'] is not None else 'off'}`",
        f"- min_planner_pass_rate: `{fmt_pct(data['thresholds']['min_planner_pass_rate']) if data['thresholds']['min_planner_pass_rate'] is not None else 'off'}`",
        f"- max_no_applicable_snapshots: `{data['thresholds']['max_no_applicable_snapshots']}`",
        f"- fail_on_warn: `{str(data['thresholds']['fail_on_warn']).lower()}`",
        "",
        "## Reasons",
        "",
    ]
    if data["fail_reasons"]:
        for reason in data["fail_reasons"]:
            lines.append(f"- fail: {reason}")
    if data["warn_reasons"]:
        for reason in data["warn_reasons"]:
            lines.append(f"- warn: {reason}")
    if not data["fail_reasons"] and not data["warn_reasons"]:
        lines.append("- pass: no gate violations detected")
    lines.extend(
        [
            "",
            "## Latest Metrics",
            "",
            f"- Builder latest/avg: `{fmt_pct(data['summary']['builder']['latest'])}` / `{fmt_pct(data['summary']['builder']['average'])}`",
            f"- Planner latest/avg: `{fmt_pct(data['summary']['planner']['latest'])}` / `{fmt_pct(data['summary']['planner']['average'])}`",
            f"- Latest tracked/applicable/skipped: `{data['summary']['coverage']['latest_tracked_session_count']}` / `{data['summary']['coverage']['latest_applicable_session_count']}` / `{data['summary']['coverage']['latest_skipped_session_count']}`",
            "",
            "## Drift Flags",
            "",
        ]
    )
    flags = data["summary"].get("drift_flags", [])
    if flags:
        for flag in flags:
            lines.append(f"- {flag}")
    else:
        lines.append("- _none_")
    return "\n".join(lines).rstrip() + "\n"


rendered = (
    json.dumps(payload, indent=2) + "\n"
    if output_format == "json"
    else render_markdown(payload)
)

if output_path:
    output_file = Path(output_path)
    output_file.parent.mkdir(parents=True, exist_ok=True)
    output_file.write_text(rendered, encoding="utf-8")
    print(str(output_file))
else:
    sys.stdout.write(rendered)

if status == "fail" or (status == "warn" and fail_on_warn):
    raise SystemExit(1)
PY
