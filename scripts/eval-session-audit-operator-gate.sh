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
  --policy PATH                 Load repo-local operator gate policy from JSON
                                (default: .codelens/eval-session-audit-gate.json
                                when present)
  --no-policy                   Ignore repo-local and env-provided policy files
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
  bash scripts/eval-session-audit-operator-gate.sh --policy .codelens/eval-session-audit-gate.json
  bash scripts/eval-session-audit-operator-gate.sh --limit 7
  bash scripts/eval-session-audit-operator-gate.sh --fail-on-warn
  bash scripts/eval-session-audit-operator-gate.sh --format json
EOF
}

DEFAULT_INPUT_DIR=".codelens/reports/daily"
DEFAULT_LIMIT="14"
DEFAULT_OUTPUT_FORMAT="markdown"
DEFAULT_MIN_BUILDER_PASS_RATE="0.5"
DEFAULT_MIN_PLANNER_PASS_RATE="0.5"
DEFAULT_MAX_NO_APPLICABLE="0"
ENV_POLICY_PATH="${CODELENS_AUDIT_GATE_POLICY_PATH:-}"
CLI_INPUT_DIR=""
CLI_POLICY_PATH=""
CLI_LIMIT=""
CLI_OUTPUT_FORMAT=""
CLI_MIN_BUILDER_PASS_RATE=""
CLI_MIN_PLANNER_PASS_RATE=""
CLI_MAX_NO_APPLICABLE=""
CLI_FAIL_ON_WARN=""
DISABLE_POLICY=0
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
		CLI_INPUT_DIR="${2:-}"
		shift 2
		;;
	--policy)
		CLI_POLICY_PATH="${2:-}"
		shift 2
		;;
	--no-policy)
		DISABLE_POLICY=1
		shift
		;;
	--limit)
		CLI_LIMIT="${2:-}"
		shift 2
		;;
	--format)
		CLI_OUTPUT_FORMAT="${2:-}"
		shift 2
		;;
	--min-builder-pass-rate)
		CLI_MIN_BUILDER_PASS_RATE="${2:-}"
		shift 2
		;;
	--min-planner-pass-rate)
		CLI_MIN_PLANNER_PASS_RATE="${2:-}"
		shift 2
		;;
	--max-no-applicable)
		CLI_MAX_NO_APPLICABLE="${2:-}"
		shift 2
		;;
	--fail-on-warn)
		CLI_FAIL_ON_WARN="1"
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

normalize_boolish() {
	local raw="$1"
	local lowered
	lowered="$(printf '%s' "$raw" | tr '[:upper:]' '[:lower:]')"
	case "$lowered" in
	1 | true | yes | on)
		printf '1\n'
		;;
	0 | false | no | off)
		printf '0\n'
		;;
	*)
		echo "invalid boolean value: $raw" >&2
		return 1
		;;
	esac
}

current_repo_root() {
	if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
		git rev-parse --show-toplevel
	else
		pwd -P
	fi
}

REPO_ROOT="$(current_repo_root)"
DEFAULT_POLICY_PATH="$REPO_ROOT/.codelens/eval-session-audit-gate.json"
POLICY_PATH=""
POLICY_SOURCE="none"

if [[ "$DISABLE_POLICY" == "1" ]]; then
	POLICY_SOURCE="disabled"
elif [[ -n "$CLI_POLICY_PATH" ]]; then
	POLICY_PATH="$CLI_POLICY_PATH"
	POLICY_SOURCE="cli"
elif [[ -n "$ENV_POLICY_PATH" ]]; then
	POLICY_PATH="$ENV_POLICY_PATH"
	POLICY_SOURCE="env"
elif [[ -f "$DEFAULT_POLICY_PATH" ]]; then
	POLICY_PATH="$DEFAULT_POLICY_PATH"
	POLICY_SOURCE="repo_default"
fi

POLICY_LIMIT=""
POLICY_MIN_BUILDER_PASS_RATE=""
POLICY_MIN_PLANNER_PASS_RATE=""
POLICY_MAX_NO_APPLICABLE=""
POLICY_FAIL_ON_WARN=""

if [[ -n "$POLICY_PATH" ]]; then
	if [[ ! -r "$POLICY_PATH" ]]; then
		echo "policy file is not readable: $POLICY_PATH" >&2
		exit 1
	fi
	eval "$(
		python3 - "$POLICY_PATH" <<'PY'
import json
import shlex
import sys
from pathlib import Path

policy_path = Path(sys.argv[1])
try:
    data = json.loads(policy_path.read_text(encoding="utf-8"))
except FileNotFoundError as exc:
    raise SystemExit(f"missing policy file: {policy_path}") from exc
except json.JSONDecodeError as exc:
    raise SystemExit(f"invalid JSON in policy file {policy_path}: {exc}") from exc

if not isinstance(data, dict):
    raise SystemExit(f"policy file {policy_path} must contain a JSON object")

version = data.get("version")
if version is not None and version != 1:
    raise SystemExit(f"unsupported policy version in {policy_path}: {version!r}")

thresholds = data.get("thresholds", {})
if thresholds is None:
    thresholds = {}
if not isinstance(thresholds, dict):
    raise SystemExit(f"`thresholds` in {policy_path} must be a JSON object")


def value(key):
    if key in thresholds:
        return thresholds[key]
    return data.get(key)


def emit(name, raw):
    if raw is None:
        return
    print(f"{name}={shlex.quote(str(raw))}")


def normalize_rate(name, raw):
    if raw is None:
        return None
    if raw == "off":
        return "off"
    if isinstance(raw, bool) or not isinstance(raw, (int, float)):
        raise SystemExit(f"`{name}` in {policy_path} must be a number in [0, 1] or `off`")
    if raw < 0 or raw > 1:
        raise SystemExit(f"`{name}` in {policy_path} must be between 0 and 1 inclusive")
    return raw


limit = data.get("limit")
if limit is not None:
    if isinstance(limit, bool) or not isinstance(limit, int) or limit <= 0:
        raise SystemExit(f"`limit` in {policy_path} must be a positive integer")
    emit("POLICY_LIMIT", limit)

emit("POLICY_MIN_BUILDER_PASS_RATE", normalize_rate("min_builder_pass_rate", value("min_builder_pass_rate")))
emit("POLICY_MIN_PLANNER_PASS_RATE", normalize_rate("min_planner_pass_rate", value("min_planner_pass_rate")))

max_no_applicable = value("max_no_applicable_snapshots")
if max_no_applicable is not None:
    if isinstance(max_no_applicable, bool) or not isinstance(max_no_applicable, int) or max_no_applicable < 0:
        raise SystemExit(f"`max_no_applicable_snapshots` in {policy_path} must be a non-negative integer")
    emit("POLICY_MAX_NO_APPLICABLE", max_no_applicable)

fail_on_warn = value("fail_on_warn")
if fail_on_warn is not None:
    if not isinstance(fail_on_warn, bool):
        raise SystemExit(f"`fail_on_warn` in {policy_path} must be a boolean")
    emit("POLICY_FAIL_ON_WARN", "1" if fail_on_warn else "0")
PY
	)"
fi

INPUT_DIR="${CLI_INPUT_DIR:-${CODELENS_AUDIT_INPUT_DIR:-$DEFAULT_INPUT_DIR}}"
LIMIT="${CLI_LIMIT:-${CODELENS_AUDIT_HISTORY_LIMIT:-${POLICY_LIMIT:-$DEFAULT_LIMIT}}}"
OUTPUT_FORMAT="${CLI_OUTPUT_FORMAT:-${CODELENS_AUDIT_GATE_FORMAT:-$DEFAULT_OUTPUT_FORMAT}}"
MIN_BUILDER_PASS_RATE="${CLI_MIN_BUILDER_PASS_RATE:-${CODELENS_AUDIT_GATE_MIN_BUILDER_PASS_RATE:-${POLICY_MIN_BUILDER_PASS_RATE:-$DEFAULT_MIN_BUILDER_PASS_RATE}}}"
MIN_PLANNER_PASS_RATE="${CLI_MIN_PLANNER_PASS_RATE:-${CODELENS_AUDIT_GATE_MIN_PLANNER_PASS_RATE:-${POLICY_MIN_PLANNER_PASS_RATE:-$DEFAULT_MIN_PLANNER_PASS_RATE}}}"
MAX_NO_APPLICABLE="${CLI_MAX_NO_APPLICABLE:-${CODELENS_AUDIT_GATE_MAX_NO_APPLICABLE:-${POLICY_MAX_NO_APPLICABLE:-$DEFAULT_MAX_NO_APPLICABLE}}}"
if [[ -n "$CLI_FAIL_ON_WARN" ]]; then
	FAIL_ON_WARN="$CLI_FAIL_ON_WARN"
elif [[ -n "${CODELENS_AUDIT_GATE_FAIL_ON_WARN:-}" ]]; then
	FAIL_ON_WARN="$(normalize_boolish "${CODELENS_AUDIT_GATE_FAIL_ON_WARN}")"
elif [[ -n "$POLICY_FAIL_ON_WARN" ]]; then
	FAIL_ON_WARN="$POLICY_FAIL_ON_WARN"
else
	FAIL_ON_WARN="0"
fi

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

python3 - "$SUMMARY_JSON_FILE" "$OUTPUT_FORMAT" "$OUTPUT_PATH" "$FAIL_ON_WARN" "$MIN_BUILDER_PASS_RATE" "$MIN_PLANNER_PASS_RATE" "$MAX_NO_APPLICABLE" "$POLICY_PATH" "$POLICY_SOURCE" <<'PY'
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
    policy_path,
    policy_source,
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
    "policy": {
        "path": policy_path or None,
        "source": policy_source,
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
        f"- Policy: `{data['policy']['path'] or 'none'}` ({data['policy']['source']})",
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
