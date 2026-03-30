#!/usr/bin/env bash
# CodeLens MCP — Session Telemetry Collector
#
# Usage (two modes):
#   1. Pipe JSON from get_tool_metrics:
#      echo '{"data":{...}}' | ./benchmarks/collect-session.sh session-name
#
#   2. Read from file:
#      ./benchmarks/collect-session.sh session-name metrics.json
#
# The JSON is the raw output from the get_tool_metrics MCP tool.
set -euo pipefail

NAME="${1:-session}"
JSON_FILE="${2:-}"
DATE=$(date +%Y-%m-%d)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
OUTDIR="$(dirname "$0")/results"
OUTFILE="${OUTDIR}/${DATE}-${NAME}.md"

mkdir -p "$OUTDIR"

# Read JSON from file or stdin
if [ -n "$JSON_FILE" ] && [ -f "$JSON_FILE" ]; then
	RAW=$(cat "$JSON_FILE")
else
	RAW=$(cat -)
fi

python3 -c "
import json, sys

raw = json.loads(sys.argv[1])
data = raw.get('data', {})
session = data.get('session', {})
tools = sorted(data.get('tools', []), key=lambda t: t['calls'], reverse=True)
total_calls = max(session.get('total_calls', 1), 1)
total_tokens = session.get('total_tokens', 0)
count = data.get('count', 0)

lines = []
a = lines.append

a('---')
a('date: $DATE')
a('phase: $NAME (session telemetry)')
a('commit: $COMMIT')
a('---')
a('')
a('# Session Telemetry: $DATE — $NAME')
a('')
a('## Summary')
a('')
a('| Metric | Value |')
a('|---|---|')
a(f'| Total calls | {total_calls} |')
a(f'| Total time | {session.get(\"total_ms\", 0):,}ms |')
a(f'| Avg per call | {session.get(\"avg_ms_per_call\", 0)}ms |')
a(f'| Total tokens | {total_tokens:,} |')
a(f'| Errors | {session.get(\"error_count\", 0)} |')
a(f'| Unique tools | {count} |')
a('')
a('## Tool Usage')
a('')
a('| Tool | Calls | Total(ms) | Avg(ms) | Max(ms) | Err |')
a('|---|---|---|---|---|---|')
for t in tools:
    c = t['calls']
    avg = round(t['total_ms'] / c, 1) if c > 0 else 0
    a(f'| {t[\"tool\"]} | {c} | {t[\"total_ms\"]:,} | {avg} | {t[\"max_ms\"]:,} | {t[\"errors\"]} |')

a('')
a('## Distribution')
a('')
a('\`\`\`')
for t in tools[:5]:
    pct = round(t['calls'] / total_calls * 100, 1)
    bar = '#' * int(pct / 2)
    a(f'  {t[\"tool\"]:30} {t[\"calls\"]:3} ({pct:5.1f}%) {bar}')
a('\`\`\`')
a('')
a(f'Unused: {39 - count}/39 BALANCED tools')
a('')
a('## Token Efficiency')
a('')
a(f'| Tokens/call | {total_tokens // total_calls:,} |')
a(f'|---|---|')

print('\n'.join(lines))
" "$RAW" >"$OUTFILE"

echo "Saved: $OUTFILE"
echo ""
cat "$OUTFILE"
