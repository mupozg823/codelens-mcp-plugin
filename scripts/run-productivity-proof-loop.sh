#!/bin/bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/run-productivity-proof-loop.sh [repo-root] [options]

Run one evidence collection loop for proving whether CodeLens improves agent
productivity. The loop collects local tool-usage telemetry, exports a live
daemon eval_session_audit snapshot, refreshes history/gate artifacts, and
writes a small artifact index.

Options:
  --mcp-url URL          MCP HTTP endpoint (default: http://127.0.0.1:7839/mcp)
  --telemetry-path PATH  tool_usage.jsonl to analyze. If omitted, the script
                        checks CODELENS_TELEMETRY_PATH, repo-root telemetry,
                        then crates/codelens-mcp telemetry.
  --output-dir DIR       Output root (default: .codelens/reports/productivity)
  --run-id ID            Stable run id for reproducible paths
  --history-limit N      Number of audit snapshots to include in trend/gate
                        artifacts (default: 14)
  --skip-audit           Skip the live daemon eval_session_audit export
  --print-plan           Print resolved paths and exit without writing artifacts
  -h, --help             Show this help

Examples:
  bash scripts/run-productivity-proof-loop.sh .
  bash scripts/run-productivity-proof-loop.sh . --mcp-url http://127.0.0.1:7839/mcp
  bash scripts/run-productivity-proof-loop.sh . --print-plan
EOF
}

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT=""
MCP_URL="${CODELENS_AUDIT_MCP_URL:-http://127.0.0.1:7839/mcp}"
TELEMETRY_PATH="${CODELENS_TELEMETRY_PATH:-}"
OUTPUT_DIR=""
RUN_ID="${CODELENS_PRODUCTIVITY_RUN_ID:-}"
HISTORY_LIMIT="${CODELENS_PRODUCTIVITY_HISTORY_LIMIT:-14}"
SKIP_AUDIT=0
PRINT_PLAN=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--mcp-url)
		MCP_URL="${2:-}"
		shift 2
		;;
	--telemetry-path)
		TELEMETRY_PATH="${2:-}"
		shift 2
		;;
	--output-dir)
		OUTPUT_DIR="${2:-}"
		shift 2
		;;
	--run-id)
		RUN_ID="${2:-}"
		shift 2
		;;
	--history-limit)
		HISTORY_LIMIT="${2:-}"
		shift 2
		;;
	--skip-audit)
		SKIP_AUDIT=1
		shift
		;;
	--print-plan)
		PRINT_PLAN=1
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
	REPO_ROOT="$DEFAULT_REPO_ROOT"
fi
REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd)"

if [[ -z "$OUTPUT_DIR" ]]; then
	OUTPUT_DIR="$REPO_ROOT/.codelens/reports/productivity"
elif [[ "$OUTPUT_DIR" != /* ]]; then
	OUTPUT_DIR="$REPO_ROOT/$OUTPUT_DIR"
fi

if [[ -z "$RUN_ID" ]]; then
	RUN_ID="$(date +%Y%m%d-%H%M%S)"
fi

if ! [[ "$HISTORY_LIMIT" =~ ^[0-9]+$ ]] || [[ "$HISTORY_LIMIT" == "0" ]]; then
	echo "--history-limit must be a positive integer" >&2
	exit 2
fi

ANALYZER="$REPO_ROOT/scripts/analyze-tool-usage.py"
EXPORT_AUDIT="$REPO_ROOT/scripts/export-eval-session-audit.sh"
SUMMARY_SCRIPT="$REPO_ROOT/scripts/summarize-eval-session-audit-history.sh"
GATE_SCRIPT="$REPO_ROOT/scripts/eval-session-audit-operator-gate.sh"
PRODUCTIVITY_SUMMARY_SCRIPT="$REPO_ROOT/scripts/summarize-productivity-proof-runs.py"

for required in "$ANALYZER" "$EXPORT_AUDIT" "$SUMMARY_SCRIPT" "$GATE_SCRIPT" "$PRODUCTIVITY_SUMMARY_SCRIPT"; do
	if [[ ! -f "$required" ]]; then
		echo "missing required script: $required" >&2
		exit 1
	fi
done

resolve_telemetry_path() {
	if [[ -n "$TELEMETRY_PATH" ]]; then
		printf '%s\n' "$TELEMETRY_PATH"
		return
	fi
	local candidate
	for candidate in \
		"$REPO_ROOT/.codelens/telemetry/tool_usage.jsonl" \
		"$REPO_ROOT/crates/codelens-mcp/.codelens/telemetry/tool_usage.jsonl"; do
		if [[ -f "$candidate" ]]; then
			printf '%s\n' "$candidate"
			return
		fi
	done
}

RESOLVED_TELEMETRY_PATH="$(resolve_telemetry_path)"
RUN_DIR="$OUTPUT_DIR/runs/$RUN_ID"
HISTORY_DIR="$OUTPUT_DIR/history"
TOOL_USAGE_JSON="$RUN_DIR/tool-usage.json"
TOOL_USAGE_TEXT="$RUN_DIR/tool-usage.txt"
AUDIT_JSON="$HISTORY_DIR/eval-session-audit-$RUN_ID.json"
SUMMARY_MD="$RUN_DIR/history-summary.md"
GATE_MD="$RUN_DIR/operator-gate.md"
PRODUCTIVITY_SUMMARY_MD="$RUN_DIR/productivity-trend-summary.md"
INDEX_MD="$RUN_DIR/productivity-proof-loop.md"

print_plan() {
	cat <<EOF
repo_root=$REPO_ROOT
mcp_url=$MCP_URL
telemetry_path=${RESOLVED_TELEMETRY_PATH:-<none>}
output_dir=$OUTPUT_DIR
run_id=$RUN_ID
run_dir=$RUN_DIR
history_dir=$HISTORY_DIR
tool_usage_json=$TOOL_USAGE_JSON
tool_usage_text=$TOOL_USAGE_TEXT
audit_json=$AUDIT_JSON
history_summary=$SUMMARY_MD
operator_gate=$GATE_MD
productivity_summary=$PRODUCTIVITY_SUMMARY_MD
index=$INDEX_MD
skip_audit=$SKIP_AUDIT
EOF
}

if [[ "$PRINT_PLAN" == "1" ]]; then
	print_plan
	exit 0
fi

mkdir -p "$RUN_DIR" "$HISTORY_DIR"

pushd "$REPO_ROOT" >/dev/null
if [[ -n "$RESOLVED_TELEMETRY_PATH" && -f "$RESOLVED_TELEMETRY_PATH" ]]; then
	python3 "$ANALYZER" \
		--telemetry-path "$RESOLVED_TELEMETRY_PATH" \
		--format json \
		--output "$TOOL_USAGE_JSON"
	python3 "$ANALYZER" \
		--telemetry-path "$RESOLVED_TELEMETRY_PATH" \
		> "$TOOL_USAGE_TEXT"
else
	python3 "$ANALYZER" --format json --output "$TOOL_USAGE_JSON"
	python3 "$ANALYZER" > "$TOOL_USAGE_TEXT"
fi

if [[ "$SKIP_AUDIT" == "0" ]]; then
	bash "$EXPORT_AUDIT" "$AUDIT_JSON" \
		--format json \
		--mcp-url "$MCP_URL" \
		--history-summary-path "$SUMMARY_MD" \
		--history-gate-path "$GATE_MD" \
		--history-summary-limit "$HISTORY_LIMIT" \
		>/dev/null
else
	if compgen -G "$HISTORY_DIR/eval-session-audit-*.json" >/dev/null; then
		bash "$SUMMARY_SCRIPT" "$SUMMARY_MD" \
			--input-dir "$HISTORY_DIR" \
			--limit "$HISTORY_LIMIT" \
			>/dev/null
		bash "$GATE_SCRIPT" "$GATE_MD" \
			--input-dir "$HISTORY_DIR" \
			--limit "$HISTORY_LIMIT" \
			>/dev/null || true
	fi
fi
popd >/dev/null

python3 "$PRODUCTIVITY_SUMMARY_SCRIPT" \
	--input-dir "$OUTPUT_DIR/runs" \
	--audit-history-dir "$HISTORY_DIR" \
	--output "$PRODUCTIVITY_SUMMARY_MD" \
	--limit "$HISTORY_LIMIT" \
	>/dev/null

{
	printf '# CodeLens productivity proof loop\n\n'
	printf -- '- Generated at: `%s`\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
	printf -- '- Repository: `%s`\n' "$REPO_ROOT"
	printf -- '- MCP URL: `%s`\n' "$MCP_URL"
	printf -- '- Telemetry path: `%s`\n' "${RESOLVED_TELEMETRY_PATH:-none}"
	printf -- '- Run ID: `%s`\n\n' "$RUN_ID"
	printf '## Artifacts\n\n'
	printf -- '- Tool usage JSON: `%s`\n' "$TOOL_USAGE_JSON"
	printf -- '- Tool usage text: `%s`\n' "$TOOL_USAGE_TEXT"
	if [[ "$SKIP_AUDIT" == "0" ]]; then
		printf -- '- Eval session audit JSON: `%s`\n' "$AUDIT_JSON"
	fi
	if [[ -f "$SUMMARY_MD" ]]; then
		printf -- '- Audit trend summary: `%s`\n' "$SUMMARY_MD"
	fi
	if [[ -f "$GATE_MD" ]]; then
		printf -- '- Operator gate: `%s`\n' "$GATE_MD"
	fi
	printf -- '- Productivity trend summary: `%s`\n' "$PRODUCTIVITY_SUMMARY_MD"
	printf '\n## Interpretation checklist\n\n'
	printf -- '- Tool usage proves call volume, follow-through, missed routes, builder handoff proxy, and hot/cold tools.\n'
	printf -- '- Eval session audit proves planner/builder session quality for the running daemon.\n'
	printf -- '- History summary proves drift across repeated loop runs.\n'
	printf -- '- Productivity trend summary compares latest tool-usage metrics against previous runs.\n'
	printf -- '- Operator gate turns the recent window into pass/warn/fail automation evidence.\n'
} > "$INDEX_MD"

printf '%s\n' "$INDEX_MD"
