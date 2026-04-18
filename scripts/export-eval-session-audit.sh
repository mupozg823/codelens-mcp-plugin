#!/bin/bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/export-eval-session-audit.sh [output-path] [options]

Export a daemon-wide `eval_session_audit` snapshot from a running CodeLens
HTTP daemon. The snapshot is intentionally aggregate/runtime-scoped, not a
per-session Stop-hook artifact.

Options:
  --format json|markdown   Output format (default: json)
  --history-summary-path PATH
                         Refresh a historical trend summary after writing a
                         JSON snapshot
  --history-gate-path PATH
                         Refresh an operator gate artifact after writing a
                         JSON snapshot
  --history-summary-limit N
                         Number of recent JSON snapshots to include when
                         refreshing the historical summary and gate
                         artifacts (default: 14)
  --mcp-url URL            MCP HTTP endpoint (default: http://127.0.0.1:7837/mcp)
  --timeout-secs N         RPC timeout in seconds (default: 10)
  --poll-interval-secs N   Poll interval in seconds (default: 0.5)
  --max-polls N            Maximum job polls before timing out (default: 20)
  -h, --help               Show this help

Examples:
  bash scripts/export-eval-session-audit.sh
  bash scripts/export-eval-session-audit.sh --format markdown
  bash scripts/export-eval-session-audit.sh --history-summary-path .codelens/reports/daily/latest-summary.md
  bash scripts/export-eval-session-audit.sh --history-gate-path .codelens/reports/daily/latest-gate.md
  bash scripts/export-eval-session-audit.sh .codelens/reports/daily/latest.md --format markdown
EOF
}

MCP_URL="${CODELENS_AUDIT_MCP_URL:-http://127.0.0.1:7837/mcp}"
TIMEOUT_SECS="${CODELENS_AUDIT_TIMEOUT_SECS:-10}"
POLL_INTERVAL_SECS="${CODELENS_AUDIT_POLL_INTERVAL_SECS:-0.5}"
MAX_POLLS="${CODELENS_AUDIT_MAX_POLLS:-20}"
OUTPUT_FORMAT="${CODELENS_AUDIT_OUTPUT_FORMAT:-json}"
HISTORY_SUMMARY_PATH="${CODELENS_AUDIT_HISTORY_SUMMARY_PATH:-}"
HISTORY_GATE_PATH="${CODELENS_AUDIT_HISTORY_GATE_PATH:-}"
HISTORY_SUMMARY_LIMIT="${CODELENS_AUDIT_HISTORY_SUMMARY_LIMIT:-14}"
DEFAULT_OUTPUT_DIR="${CODELENS_AUDIT_OUTPUT_DIR:-.codelens/reports}"
OUTPUT_PATH=""
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUMMARY_SCRIPT="$SCRIPT_DIR/summarize-eval-session-audit-history.sh"
GATE_SCRIPT="$SCRIPT_DIR/eval-session-audit-operator-gate.sh"

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--format)
		OUTPUT_FORMAT="${2:-}"
		shift 2
		;;
	--history-summary-path)
		HISTORY_SUMMARY_PATH="${2:-}"
		shift 2
		;;
	--history-gate-path)
		HISTORY_GATE_PATH="${2:-}"
		shift 2
		;;
	--history-summary-limit)
		HISTORY_SUMMARY_LIMIT="${2:-}"
		shift 2
		;;
	--mcp-url)
		MCP_URL="${2:-}"
		shift 2
		;;
	--timeout-secs)
		TIMEOUT_SECS="${2:-}"
		shift 2
		;;
	--poll-interval-secs)
		POLL_INTERVAL_SECS="${2:-}"
		shift 2
		;;
	--max-polls)
		MAX_POLLS="${2:-}"
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
json | markdown) ;;
*)
	echo "--format must be one of: json, markdown" >&2
	exit 2
	;;
esac

if ! [[ "$HISTORY_SUMMARY_LIMIT" =~ ^[0-9]+$ ]] || [[ "$HISTORY_SUMMARY_LIMIT" == "0" ]]; then
	echo "--history-summary-limit must be a positive integer" >&2
	exit 2
fi

if [[ -n "$HISTORY_SUMMARY_PATH" && "$OUTPUT_FORMAT" != "json" ]]; then
	echo "--history-summary-path requires --format json because the history summarizer reads JSON snapshots" >&2
	exit 2
fi
if [[ -n "$HISTORY_GATE_PATH" && "$OUTPUT_FORMAT" != "json" ]]; then
	echo "--history-gate-path requires --format json because the operator gate reads JSON snapshots" >&2
	exit 2
fi

if [[ -z "$OUTPUT_PATH" ]]; then
	mkdir -p "$DEFAULT_OUTPUT_DIR"
	if [[ "$OUTPUT_FORMAT" == "markdown" ]]; then
		OUTPUT_PATH="$DEFAULT_OUTPUT_DIR/eval-session-audit-$(date +%Y%m%d-%H%M%S).md"
	else
		OUTPUT_PATH="$DEFAULT_OUTPUT_DIR/eval-session-audit-$(date +%Y%m%d-%H%M%S).json"
	fi
else
	mkdir -p "$(dirname "$OUTPUT_PATH")"
fi

WRITTEN_PATH="$(
	python3 - "$OUTPUT_PATH" "$OUTPUT_FORMAT" "$MCP_URL" "$TIMEOUT_SECS" "$POLL_INTERVAL_SECS" "$MAX_POLLS" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

output_path, output_format, mcp_url, timeout_secs, poll_interval_secs, max_polls = sys.argv[1:]
timeout_secs = float(timeout_secs)
poll_interval_secs = float(poll_interval_secs)
max_polls = int(max_polls)
request_id = 0


def nested_get(node, *path):
    current = node
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def first_present(node, *paths):
    for path in paths:
        value = nested_get(node, *path)
        if value is not None:
            return value
    return None


def rpc(method, params):
    global request_id
    request_id += 1
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        mcp_url,
        data=body,
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout_secs) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError) as exc:
        raise SystemExit(f"RPC {method} failed: {exc}") from exc
    if "error" in payload:
        raise SystemExit(f"RPC {method} returned error: {payload['error']}")
    return payload


def tool_call(name, arguments):
    return rpc("tools/call", {"name": name, "arguments": arguments})


def section_content(payload):
    content = first_present(
        payload,
        ("result", "structuredContent", "data", "content"),
        ("result", "structuredContent", "content"),
    )
    if content is None:
        raise SystemExit(f"Missing section content in payload: {json.dumps(payload)}")
    return content


def render_markdown(payload):
    audit = payload["audit_pass_rate"]
    rows = payload["session_rows"]

    def percent(value):
        if value is None:
            return "n/a"
        return f"{value * 100:.1f}%"

    def counts_line(label, counts):
        return (
            f"- {label}: pass={counts.get('pass', 0)}, "
            f"warn={counts.get('warn', 0)}, fail={counts.get('fail', 0)}"
        )

    def format_list(items):
        if not items:
            return "_none_"
        return ", ".join(f"`{item}`" for item in items)

    lines = [
        "# CodeLens eval_session_audit",
        "",
        f"- Generated at: `{payload['generated_at']}`",
        f"- MCP URL: `{payload['mcp_url']}`",
        f"- Job ID: `{payload['job_id']}`",
        f"- Analysis ID: `{payload['analysis_id']}`",
        "",
        "## Audit Pass Rate",
        "",
        f"- Tracked runtime sessions: `{audit.get('tracked_session_count', 0)}`",
        f"- Applicable audited sessions: `{audit.get('session_count', 0)}`",
        f"- Skipped runtime sessions: `{audit.get('skipped_session_count', 0)}`",
        f"- Builder pass rate: `{percent(audit.get('builder_pass_rate'))}` across `{audit.get('builder_session_count', 0)}` session(s)",
        f"- Planner pass rate: `{percent(audit.get('planner_pass_rate'))}` across `{audit.get('planner_session_count', 0)}` session(s)",
        "",
        "### Status Counts",
        "",
        counts_line("Builder", audit.get("builder_status_counts", {})),
        counts_line("Planner", audit.get("planner_status_counts", {})),
        "",
        "### Top Failed Checks",
        "",
    ]

    top_failed = audit.get("top_failed_checks") or []
    if top_failed:
        for item in top_failed:
            code = item.get("code", "unknown")
            count = item.get("count", 0)
            lines.append(f"- `{code}` in `{count}` session(s)")
    else:
        lines.append("- _none_")

    lines.extend(["", "## Session Rows", ""])
    sessions = rows.get("sessions") or []
    if not sessions:
        lines.append("_none_")
    else:
        for session in sessions:
            session_id = session.get("session_id") or "unknown-session"
            title = session_id[:8] if isinstance(session_id, str) else "unknown"
            lines.extend(
                [
                    f"### `{title}`",
                    "",
                    f"- session_id: `{session_id}`",
                    f"- role: `{session.get('role', 'unknown')}`",
                    f"- status: `{session.get('status', 'unknown')}`",
                    f"- score: `{session.get('score', 'n/a')}`",
                    f"- surface: `{session.get('surface', 'unknown')}`",
                    f"- transport: `{session.get('transport', 'unknown')}`",
                    f"- finding_codes: {format_list(session.get('finding_codes') or [])}",
                    f"- recent_tools: {format_list(session.get('recent_tools') or [])}",
                    f"- recommended_next_tools: {format_list(session.get('recommended_next_tools') or [])}",
                    "",
                ]
            )

    return "\n".join(lines).rstrip() + "\n"


start_payload = tool_call(
    "start_analysis_job",
    {"kind": "eval_session_audit", "profile_hint": "ci-audit"},
)
job_id = first_present(
    start_payload,
    ("result", "structuredContent", "data", "job_id"),
    ("result", "structuredContent", "job_id"),
)
if not isinstance(job_id, str) or not job_id:
    raise SystemExit(f"Missing job_id from start_analysis_job: {json.dumps(start_payload)}")

analysis_id = None
status = None
error_detail = None
for _ in range(max_polls):
    job_payload = tool_call("get_analysis_job", {"job_id": job_id})
    status = first_present(
        job_payload,
        ("result", "structuredContent", "data", "status"),
        ("result", "structuredContent", "status"),
    )
    error_detail = first_present(
        job_payload,
        ("result", "structuredContent", "data", "error"),
        ("result", "structuredContent", "error"),
    )
    if status == "completed":
        analysis_id = first_present(
            job_payload,
            ("result", "structuredContent", "data", "analysis_id"),
            ("result", "structuredContent", "analysis_id"),
        )
        break
    if status in {"failed", "cancelled", "error"}:
        suffix = f": {error_detail}" if isinstance(error_detail, str) and error_detail else ""
        raise SystemExit(f"eval_session_audit job {job_id} ended with status={status}{suffix}")
    time.sleep(poll_interval_secs)

if not isinstance(analysis_id, str) or not analysis_id:
    suffix = f", error={error_detail}" if isinstance(error_detail, str) and error_detail else ""
    raise SystemExit(
        f"Timed out waiting for eval_session_audit completion (job_id={job_id}, status={status or 'unknown'}{suffix})"
    )

audit_pass_rate = section_content(
    tool_call(
        "get_analysis_section",
        {"analysis_id": analysis_id, "section": "audit_pass_rate"},
    )
)
session_rows = section_content(
    tool_call(
        "get_analysis_section",
        {"analysis_id": analysis_id, "section": "session_rows"},
    )
)

payload = {
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "mcp_url": mcp_url,
    "job_id": job_id,
    "analysis_id": analysis_id,
    "audit_pass_rate": audit_pass_rate,
    "session_rows": session_rows,
}
if output_format == "markdown":
    rendered = render_markdown(payload)
else:
    rendered = json.dumps(payload, indent=2) + "\n"
Path(output_path).write_text(rendered, encoding="utf-8")
print(output_path)
PY
)"

if [[ -n "$HISTORY_SUMMARY_PATH" ]]; then
	if [[ ! -x "$SUMMARY_SCRIPT" ]]; then
		echo "warning: history summary refresh skipped because $SUMMARY_SCRIPT is not executable" >&2
	else
		SNAPSHOT_DIR="$(dirname "$WRITTEN_PATH")"
		if ! bash "$SUMMARY_SCRIPT" \
			--input-dir "$SNAPSHOT_DIR" \
			--limit "$HISTORY_SUMMARY_LIMIT" \
			"$HISTORY_SUMMARY_PATH" >/dev/null; then
			echo "warning: failed to refresh history summary at $HISTORY_SUMMARY_PATH" >&2
		fi
	fi
fi

if [[ -n "$HISTORY_GATE_PATH" ]]; then
	if [[ ! -x "$GATE_SCRIPT" ]]; then
		echo "warning: history gate refresh skipped because $GATE_SCRIPT is not executable" >&2
	else
		SNAPSHOT_DIR="${SNAPSHOT_DIR:-$(dirname "$WRITTEN_PATH")}"
		if ! bash "$GATE_SCRIPT" \
			--input-dir "$SNAPSHOT_DIR" \
			--limit "$HISTORY_SUMMARY_LIMIT" \
			"$HISTORY_GATE_PATH" >/dev/null 2>&1; then
			if [[ ! -f "$HISTORY_GATE_PATH" ]]; then
				echo "warning: failed to refresh history gate at $HISTORY_GATE_PATH" >&2
			fi
		fi
	fi
fi

printf '%s\n' "$WRITTEN_PATH"
