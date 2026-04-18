#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/install-eval-session-audit-launchd.sh [repo-root] [options]

Write a launchd agent that exports a daemon-wide eval_session_audit snapshot
once per day against a running CodeLens HTTP daemon.

Options:
  --hour N          Hour in local time (default: 23)
  --minute N        Minute in local time (default: 55)
  --mcp-url URL     MCP HTTP endpoint (default: http://127.0.0.1:7837/mcp)
  --output-dir DIR  Snapshot output dir (default: <repo>/.codelens/reports/daily)
  --label LABEL     launchd label (default: dev.codelens.eval-session-audit.<repo>)
  --plist-path PATH Write plist here instead of ~/Library/LaunchAgents/<label>.plist
  --run-at-load     Also run the job immediately when the agent is loaded
  --load            Bootstrap the written plist into launchd after writing it
  --print-only      Print the plist to stdout instead of writing it
  -h, --help        Show this help

Examples:
  bash scripts/install-eval-session-audit-launchd.sh .
  bash scripts/install-eval-session-audit-launchd.sh . --hour 2 --minute 30 --load
  bash scripts/install-eval-session-audit-launchd.sh . --print-only
EOF
}

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT=""
HOUR=23
MINUTE=55
MCP_URL="${CODELENS_AUDIT_MCP_URL:-http://127.0.0.1:7837/mcp}"
OUTPUT_DIR=""
LABEL=""
PLIST_PATH=""
RUN_AT_LOAD=0
LOAD_AFTER_WRITE=0
PRINT_ONLY=0

is_int_in_range() {
	local value="$1"
	local min="$2"
	local max="$3"
	[[ "$value" =~ ^[0-9]+$ ]] || return 1
	((value >= min && value <= max))
}

sanitize_label_component() {
	printf '%s' "$1" | tr -c '[:alnum:].-' '-'
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--hour)
		HOUR="${2:-}"
		shift 2
		;;
	--minute)
		MINUTE="${2:-}"
		shift 2
		;;
	--mcp-url)
		MCP_URL="${2:-}"
		shift 2
		;;
	--output-dir)
		OUTPUT_DIR="${2:-}"
		shift 2
		;;
	--label)
		LABEL="${2:-}"
		shift 2
		;;
	--plist-path)
		PLIST_PATH="${2:-}"
		shift 2
		;;
	--run-at-load)
		RUN_AT_LOAD=1
		shift
		;;
	--load)
		LOAD_AFTER_WRITE=1
		shift
		;;
	--print-only)
		PRINT_ONLY=1
		shift
		;;
	-*)
		echo "unknown option: $1" >&2
		usage >&2
		exit 2
		;;
	*)
		if [[ -n "$REPO_ROOT" ]]; then
			echo "multiple repo roots provided" >&2
			usage >&2
			exit 2
		fi
		REPO_ROOT="$1"
		shift
		;;
	esac
done

if [[ -z "$REPO_ROOT" ]]; then
	REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
else
	REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd)"
fi

if [[ ! -f "$REPO_ROOT/scripts/export-eval-session-audit.sh" ]]; then
	echo "missing export script under repo root: $REPO_ROOT/scripts/export-eval-session-audit.sh" >&2
	exit 1
fi

if ! is_int_in_range "$HOUR" 0 23; then
	echo "--hour must be an integer in [0, 23]" >&2
	exit 2
fi
if ! is_int_in_range "$MINUTE" 0 59; then
	echo "--minute must be an integer in [0, 59]" >&2
	exit 2
fi

if [[ -z "$OUTPUT_DIR" ]]; then
	OUTPUT_DIR="$REPO_ROOT/.codelens/reports/daily"
fi

if [[ -z "$LABEL" ]]; then
	LABEL="dev.codelens.eval-session-audit.$(sanitize_label_component "$(basename "$REPO_ROOT")")"
fi

if [[ -z "$PLIST_PATH" ]]; then
	PLIST_PATH="$HOME/Library/LaunchAgents/$LABEL.plist"
fi

LOG_DIR="$REPO_ROOT/.codelens/reports/launchd"
STDOUT_PATH="$LOG_DIR/$LABEL.out.log"
STDERR_PATH="$LOG_DIR/$LABEL.err.log"

mkdir -p "$OUTPUT_DIR" "$LOG_DIR"

quote_for_bash() {
	printf '%q' "$1"
}

xml_escape() {
	python3 - "$1" <<'PY'
import html
import sys

print(html.escape(sys.argv[1], quote=True))
PY
}

EXPORT_SCRIPT="$REPO_ROOT/scripts/export-eval-session-audit.sh"
LAUNCH_COMMAND="cd $(quote_for_bash "$REPO_ROOT") && CODELENS_AUDIT_MCP_URL=$(quote_for_bash "$MCP_URL") CODELENS_AUDIT_OUTPUT_DIR=$(quote_for_bash "$OUTPUT_DIR") bash $(quote_for_bash "$EXPORT_SCRIPT")"
LABEL_XML="$(xml_escape "$LABEL")"
LAUNCH_COMMAND_XML="$(xml_escape "$LAUNCH_COMMAND")"
REPO_ROOT_XML="$(xml_escape "$REPO_ROOT")"
STDOUT_PATH_XML="$(xml_escape "$STDOUT_PATH")"
STDERR_PATH_XML="$(xml_escape "$STDERR_PATH")"

RUN_AT_LOAD_XML=""
if [[ "$RUN_AT_LOAD" == "1" ]]; then
	RUN_AT_LOAD_XML=$'  <key>RunAtLoad</key>\n  <true/>\n'
fi

PLIST_CONTENT="$(cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL_XML}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>-lc</string>
    <string>${LAUNCH_COMMAND_XML}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${REPO_ROOT_XML}</string>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Hour</key>
    <integer>${HOUR}</integer>
    <key>Minute</key>
    <integer>${MINUTE}</integer>
  </dict>
${RUN_AT_LOAD_XML}  <key>StandardOutPath</key>
  <string>${STDOUT_PATH_XML}</string>
  <key>StandardErrorPath</key>
  <string>${STDERR_PATH_XML}</string>
</dict>
</plist>
EOF
)"

if [[ "$PRINT_ONLY" == "1" ]]; then
	printf '%s\n' "$PLIST_CONTENT"
	exit 0
fi

mkdir -p "$(dirname "$PLIST_PATH")"
printf '%s\n' "$PLIST_CONTENT" >"$PLIST_PATH"

if command -v plutil >/dev/null 2>&1; then
	plutil -lint "$PLIST_PATH" >/dev/null
fi

echo "==> Wrote $PLIST_PATH"
echo "==> Schedule: daily $(printf '%02d:%02d' "$HOUR" "$MINUTE")"
echo "==> MCP URL: $MCP_URL"
echo "==> Output dir: $OUTPUT_DIR"
echo "==> stdout log: $STDOUT_PATH"
echo "==> stderr log: $STDERR_PATH"

if [[ "$LOAD_AFTER_WRITE" == "1" ]]; then
	USER_DOMAIN="gui/$(id -u)"
	launchctl bootout "$USER_DOMAIN" "$PLIST_PATH" >/dev/null 2>&1 || true
	launchctl bootstrap "$USER_DOMAIN" "$PLIST_PATH"
	echo "==> Loaded with launchctl bootstrap $USER_DOMAIN $PLIST_PATH"
else
	echo "==> Next step: launchctl bootstrap gui/$(id -u) $PLIST_PATH"
fi
