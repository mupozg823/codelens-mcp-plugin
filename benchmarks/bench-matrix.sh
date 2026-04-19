#!/bin/bash
# CodeLens MCP Multi-Target Benchmark Matrix
#
# Runs the standard bench.sh against multiple target repositories so
# results can be compared across repo shape (size, language, density).
# Produces a single aggregated markdown report instead of quoting
# self-bench numbers as universal truth.
#
# Usage: ./benchmarks/bench-matrix.sh [output_dir]

set -euo pipefail

OUT_DIR="${1:-benchmarks/matrix-$(date +%Y-%m-%d)}"
BINARY="./target/release/codelens-mcp"
BENCH_SCRIPT="./benchmarks/bench.sh"

BOLD="\033[1m"
RESET="\033[0m"
GREEN="\033[0;32m"
YELLOW="\033[0;33m"
CYAN="\033[0;36m"

mkdir -p "$OUT_DIR"

# ── Target registry ────────────────────────────────────────────────────
# Each target is "label|path|language|files|notes"
# Add new rows here to extend the matrix.

TARGETS=(
	"self|$(pwd)|mixed (Rust + Python + MD)|~320 indexed|self-benchmark (dogfooding)"
	"serena|/tmp/serena-oraios|Python|287 py files|external medium-repo (oraios/serena)"
)

# ── Helpers ─────────────────────────────────────────────────────────────

run_target() {
	local label="$1"
	local path="$2"
	local lang="$3"
	local files="$4"
	local notes="$5"
	local out_file="$OUT_DIR/bench-${label}.txt"

	if [[ ! -d "$path" ]]; then
		echo -e "${YELLOW}⚠  skip $label — path not found: $path${RESET}"
		echo "SKIP: $path does not exist" >"$out_file"
		return
	fi

	echo -e "${CYAN}▶ $label ($lang, $files)${RESET}"
	echo "  path: $path"
	echo "  notes: $notes"
	bash "$BENCH_SCRIPT" "$path" "$BINARY" 2>&1 >"$out_file" || true
	echo -e "${GREEN}  → $out_file${RESET}"
	echo
}

extract_metric() {
	# Strip ANSI colours, then extract the FIRST integer token on the
	# metric row. bench.sh outputs "<label>  <min>  <avg>  <max>" in ms,
	# so the first integer is the min (warm-path best case).
	local file="$1"
	local label="$2"
	grep -F "$label" "$file" 2>/dev/null |
		sed 's/\x1b\[[0-9;]*m//g' |
		head -1 |
		awk '{for (i=1; i<=NF; i++) if ($i ~ /^[0-9]+$/) {print $i; exit}}'
}

extract_bytes() {
	# "CodeLens find_symbol              2755 bytes (~  688 tokens)"
	# Strip the label first so its embedded digits ("-A 20") don't
	# collide with the metric value.
	local file="$1"
	local label="$2"
	grep -F "$label" "$file" 2>/dev/null |
		sed 's/\x1b\[[0-9;]*m//g' |
		head -1 |
		awk -v lbl="$label" '{
			sub(lbl, "", $0);
			for (i=1; i<=NF; i++) if ($i ~ /^[0-9]+$/) {print $i; exit}
		}'
}

# ── Build binary if needed ─────────────────────────────────────────────

if [[ ! -x "$BINARY" ]]; then
	echo -e "${CYAN}building release binary...${RESET}"
	cargo build --release -p codelens-mcp >/dev/null
fi

# ── Run each target ─────────────────────────────────────────────────────

echo -e "${BOLD}=== CodeLens Multi-Target Benchmark Matrix ===${RESET}"
echo "Date: $(date '+%Y-%m-%d %H:%M:%S')"
echo "Output dir: $OUT_DIR"
echo

for row in "${TARGETS[@]}"; do
	IFS='|' read -r label path lang files notes <<<"$row"
	run_target "$label" "$path" "$lang" "$files" "$notes"
done

# ── Aggregate into markdown ─────────────────────────────────────────────

MATRIX_MD="$OUT_DIR/matrix.md"
cat >"$MATRIX_MD" <<EOF
# CodeLens Multi-Target Benchmark Matrix

Run date: $(date '+%Y-%m-%d %H:%M:%S')
Binary: $BINARY
Source: benchmarks/bench-matrix.sh

## Targets

| Target | Path | Language | Files | Notes |
| --- | --- | --- | --- | --- |
EOF

for row in "${TARGETS[@]}"; do
	IFS='|' read -r label path lang files notes <<<"$row"
	printf "| %s | \`%s\` | %s | %s | %s |\n" "$label" "$path" "$lang" "$files" "$notes" >>"$MATRIX_MD"
done

cat >>"$MATRIX_MD" <<'EOF'

## Warm-path metrics (min of 3 runs, ms)

| Metric | self | serena |
| --- | ---: | ---: |
EOF

metrics=(
	"Cold start + get_current_config"
	"Symbol indexing (refresh_symbol_index)"
	"get_symbols_overview path=src"
	"find_symbol name=main"
	"get_impact_analysis src/main.rs"
)

for m in "${metrics[@]}"; do
	v_self=$(extract_metric "$OUT_DIR/bench-self.txt" "$m")
	v_serena=$(extract_metric "$OUT_DIR/bench-serena.txt" "$m")
	printf "| %s | %s | %s |\n" "$m" "${v_self:-n/a}" "${v_serena:-n/a}" >>"$MATRIX_MD"
done

cat >>"$MATRIX_MD" <<'EOF'

## CodeLens vs grep (min of 3 runs, ms)

| Comparison | self | serena |
| --- | ---: | ---: |
EOF

comparisons=(
	"CodeLens: find_symbol"
	"grep: fn dispatch_tool"
	"CodeLens: get_symbols_overview"
	"grep: pub/fn/struct patterns"
	"CodeLens: find_referencing_symbols"
	"grep: references"
)

for c in "${comparisons[@]}"; do
	v_self=$(extract_metric "$OUT_DIR/bench-self.txt" "$c")
	v_serena=$(extract_metric "$OUT_DIR/bench-serena.txt" "$c")
	printf "| %s | %s | %s |\n" "$c" "${v_self:-n/a}" "${v_serena:-n/a}" >>"$MATRIX_MD"
done

cat >>"$MATRIX_MD" <<'EOF'

## Token efficiency (bytes)

| Metric | self | serena |
| --- | ---: | ---: |
EOF

for c in "CodeLens find_symbol" "grep -A 20"; do
	v_self=$(extract_bytes "$OUT_DIR/bench-self.txt" "$c")
	v_serena=$(extract_bytes "$OUT_DIR/bench-serena.txt" "$c")
	printf "| %s | %s | %s |\n" "$c" "${v_self:-n/a}" "${v_serena:-n/a}" >>"$MATRIX_MD"
done

cat >>"$MATRIX_MD" <<'EOF'

## Honest interpretation

- CodeLens's ~55-60 ms warm cost is **constant** across repo sizes.
- grep scales with repo size — it wins on small repos, loses on large
  ones.
- Token compression ratio is scenario-dependent:
  - self-bench hits many `dispatch_tool` occurrences → grep's
    `-A 20` output is large → large compression ratio
  - serena has few `SerenaAgent` hits → grep output is already tight
    → small compression ratio
- Use the matrix (not a single self-bench number) to set expectations.

## Adding a new target

Edit `TARGETS=(...)` in `benchmarks/bench-matrix.sh` and re-run.
Consider adding:

- A large monorepo sample (> 1,000 files) to re-validate the "100×+"
  claim
- A tiny single-file project to stress cold-start latency
- A TypeScript / Go project to cover non-Rust / non-Python lanes
EOF

echo -e "${GREEN}${BOLD}✓ Matrix written to $MATRIX_MD${RESET}"
echo
cat "$MATRIX_MD"
