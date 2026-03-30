#!/usr/bin/env bash
# CodeLens MCP — Reproducible Benchmark Suite
# Usage: ./benchmarks/run-benchmark.sh [project_path] [output_name]
#
# Results saved to: benchmarks/results/<date>-<name>.md
# Compare with: ./benchmarks/compare.sh <old.md> <new.md>
set -euo pipefail

BIN="${CODELENS_BIN:-./target/release/codelens-mcp}"
PROJECT="${1:-.}"
NAME="${2:-manual}"
DATE=$(date +%Y-%m-%d)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
OUTDIR="$(dirname "$0")/results"
OUTFILE="${OUTDIR}/${DATE}-${NAME}.md"

mkdir -p "$OUTDIR"

if [ ! -f "$BIN" ]; then
	echo "Binary not found: $BIN"
	echo "Run: cargo build --release"
	exit 1
fi

# ── Helpers ──────────────────────────────────────────────────────────
measure() {
	local label="$1"
	shift
	local total=0
	local runs=3
	for i in $(seq 1 $runs); do
		local t=$({
			TIMEFORMAT='%3R'
			time "$@" >/dev/null 2>&1
		} 2>&1)
		# Convert to ms
		local ms=$(echo "$t * 1000" | bc 2>/dev/null | cut -d. -f1)
		total=$((total + ms))
	done
	local avg=$((total / runs))
	echo "| $label | $avg |"
}

measure_once() {
	local label="$1"
	shift
	local t=$({
		TIMEFORMAT='%3R'
		time "$@" >/dev/null 2>&1
	} 2>&1)
	local ms=$(echo "$t * 1000" | bc 2>/dev/null | cut -d. -f1)
	echo "| $label | $ms |"
}

# ── Collect project info ─────────────────────────────────────────────
FILE_COUNT=$(find "$PROJECT" -name "*.rs" -o -name "*.py" -o -name "*.ts" -o -name "*.tsx" -o -name "*.go" -o -name "*.java" -o -name "*.kt" -o -name "*.c" -o -name "*.cpp" -o -name "*.js" | grep -v target | grep -v node_modules | wc -l | xargs)
LOC=$(find "$PROJECT" -name "*.rs" -o -name "*.py" -o -name "*.ts" -o -name "*.go" -o -name "*.java" | grep -v target | grep -v node_modules | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}')

# ── Run benchmarks ───────────────────────────────────────────────────
echo "Running benchmarks on $PROJECT ($FILE_COUNT files, ~$LOC LOC)..."

{
	cat <<HEADER
---
date: $DATE
phase: $NAME
project: $PROJECT
binary: $BIN ($(uname -m)-$(uname -s | tr A-Z a-z))
commit: $COMMIT
---

# Benchmark: $DATE — $NAME

## 환경

| 항목 | 값 |
|---|---|
| OS | $(uname -srm) |
| Rust | $(rustc --version 2>/dev/null | awk '{print $2}' || echo "?") |
| 프로젝트 | $PROJECT |
| LOC | $LOC |
| 파일 수 | $FILE_COUNT |
| commit | $COMMIT |

## 핵심 도구 성능 (warm, 3회 평균, ms)

| 도구 | ms |
|---|---|
HEADER

	# Warm up (ensure index exists)
	$BIN "$PROJECT" --cmd refresh_symbol_index --args '{}' >/dev/null 2>&1

	measure "find_symbol" $BIN "$PROJECT" --cmd find_symbol --args '{"name":"main","include_body":true}'
	measure "get_symbols_overview" $BIN "$PROJECT" --cmd get_symbols_overview --args '{"path":"."}'
	measure "get_ranked_context" $BIN "$PROJECT" --cmd get_ranked_context --args '{"query":"main entry point","max_tokens":4000}'
	measure "get_impact_analysis" $BIN "$PROJECT" --cmd get_impact_analysis --args '{"file_path":"."}'
	measure "find_referencing_symbols" $BIN "$PROJECT" --cmd find_referencing_symbols --args '{"file_path":".","symbol_name":"main","max_results":50}'
	measure "refresh_symbol_index" $BIN "$PROJECT" --cmd refresh_symbol_index --args '{}'

	cat <<ZERO

## 제로 프로젝트 (auto-index, ms)

| 시나리오 | ms |
|---|---|
ZERO

	# Zero-start test: small
	TMPDIR_SMALL=$(mktemp -d)
	cd "$TMPDIR_SMALL" && git init -q
	echo "class Foo:\n    def bar(self): pass\ndef baz(): pass" >app.py
	measure_once "Python 3함수 (첫 호출)" $BIN "$TMPDIR_SMALL" --cmd find_symbol --args '{"name":"Foo"}'
	rm -rf "$TMPDIR_SMALL"

	# Zero-start test: medium
	TMPDIR_MED=$(mktemp -d)
	cd "$TMPDIR_MED" && git init -q
	for i in $(seq 1 100); do mkdir -p "m$i" && echo "def f$i(): return $i" >"m$i/m.py"; done
	measure_once "100파일 (첫 호출)" $BIN "$TMPDIR_MED" --cmd find_symbol --args '{"name":"f50"}'
	rm -rf "$TMPDIR_MED"

	cat <<GREP

## grep 대비

| 작업 | CodeLens(ms) | grep(ms) |
|---|---|---|
GREP

	# grep baseline
	CL_T=$({
		TIMEFORMAT='%3R'
		time $BIN "$PROJECT" --cmd find_symbol --args '{"name":"main"}' >/dev/null 2>&1
	} 2>&1)
	CL_MS=$(echo "$CL_T * 1000" | bc | cut -d. -f1)
	GR_T=$({
		TIMEFORMAT='%3R'
		time grep -rn "\bmain\b" "$PROJECT" --include="*.rs" --include="*.py" >/dev/null 2>&1
	} 2>&1)
	GR_MS=$(echo "$GR_T * 1000" | bc | cut -d. -f1)
	echo "| find_symbol vs grep | $CL_MS | $GR_MS |"

} >"$OUTFILE"

cd "$PROJECT" 2>/dev/null || true
echo ""
echo "Saved: $OUTFILE"
echo ""
head -50 "$OUTFILE"
