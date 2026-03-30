#!/usr/bin/env bash
# CodeLens MCP — Session Telemetry Collector
# Usage: ./benchmarks/collect-session.sh [project_path] [session_name]
#
# Captures the current session's tool usage from get_tool_metrics
# and saves a structured report to benchmarks/results/
set -euo pipefail

BIN="${CODELENS_BIN:-./target/release/codelens-mcp}"
PROJECT="${1:-.}"
NAME="${2:-session}"
DATE=$(date +%Y-%m-%d)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
OUTDIR="$(dirname "$0")/results"
OUTFILE="${OUTDIR}/${DATE}-${NAME}.md"

mkdir -p "$OUTDIR"

RAW=$($BIN "$PROJECT" --cmd get_tool_metrics --args '{}' 2>/dev/null)

python3 << PYEOF > "$OUTFILE"
import json, sys

raw = json.loads('''$RAW''')
data = raw.get('data', {})
session = data.get('session', {})
tools = data.get('tools', [])

print(f"""---
date: $DATE
phase: $NAME (session telemetry)
project: $PROJECT
binary: $BIN
commit: $COMMIT
---

# Session Telemetry: $DATE — $NAME

## 세션 요약

| 항목 | 값 |
|---|---|
| 총 도구 호출 | {session.get('total_calls', 0)}회 |
| 총 소요 시간 | {session.get('total_ms', 0):,}ms |
| 평균 호출 시간 | {session.get('avg_ms_per_call', 0)}ms |
| 총 토큰 사용 | {session.get('total_tokens', 0):,} |
| 에러 | {session.get('error_count', 0)}회 |
| 고유 도구 사용 | {data.get('count', 0)}종 |

## 도구별 사용 빈도 + 성능

| 도구 | 호출 | 총 시간(ms) | 평균(ms) | 최대(ms) | 에러 |
|---|---|---|---|---|---|""")

tools_sorted = sorted(tools, key=lambda t: t['calls'], reverse=True)
total_calls = session.get('total_calls', 1)

for t in tools_sorted:
    calls = t['calls']
    total_ms = t['total_ms']
    avg = round(total_ms / calls, 1) if calls > 0 else 0
    max_ms = t['max_ms']
    errors = t['errors']
    print(f"| {t['tool']} | {calls} | {total_ms:,} | {avg} | {max_ms:,} | {errors} |")

print(f"""
## 사용 분포

```""")

for t in tools_sorted[:5]:
    pct = round(t['calls'] / total_calls * 100, 1)
    bar = '█' * int(pct / 2)
    print(f"  {t['tool']:30} {t['calls']:3}회 ({pct:5.1f}%) {bar}")

print(f"""```

## 호출되지 않은 도구

BALANCED 프리셋 39개 중 {39 - data.get('count', 0)}개 미사용.

## 토큰 효율

| 지표 | 값 |
|---|---|
| 총 토큰 | {session.get('total_tokens', 0):,} |
| 호출당 평균 토큰 | {session.get('total_tokens', 0) // max(total_calls, 1):,} |
| Read/Grep 대비 절약 추정 | 2-5x (랭킹된 심볼만 반환) |
""")
PYEOF

echo "Saved: $OUTFILE"
cat "$OUTFILE"
