#!/bin/bash
# CodeLens MCP Benchmark
# Usage: ./benchmarks/bench.sh [project_path] [binary_path]

set -euo pipefail

PROJECT_PATH="${1:-$(pwd)}"
BINARY="${2:-./target/release/codelens-mcp}"

RUNS=3

# Colors
BOLD="\033[1m"
RESET="\033[0m"
GREEN="\033[0;32m"
CYAN="\033[0;36m"
YELLOW="\033[0;33m"

# ─── helpers ────────────────────────────────────────────────────────────────

die() {
	echo "ERROR: $*" >&2
	exit 1
}

# Returns elapsed milliseconds for a single run.
# Uses bash $TIMEFORMAT on macOS/Linux; falls back to date-delta if unavailable.
time_ms() {
	local start end
	if command -v python3 &>/dev/null; then
		start=$(python3 -c "import time; print(int(time.time()*1000))")
		"$@" &>/dev/null
		end=$(python3 -c "import time; print(int(time.time()*1000))")
		echo $((end - start))
	else
		# macOS gdate or GNU date
		if command -v gdate &>/dev/null; then
			start=$(gdate +%s%3N)
			"$@" &>/dev/null
			end=$(gdate +%s%3N)
		else
			start=$(date +%s%3N 2>/dev/null || date +%s)
			"$@" &>/dev/null
			end=$(date +%s%3N 2>/dev/null || date +%s)
		fi
		echo $((end - start))
	fi
}

# Run a benchmark N times, print min/avg/max in milliseconds.
bench() {
	local label="$1"
	shift
	local -a times=()
	local sum=0 min=999999999 max=0

	for i in $(seq 1 "$RUNS"); do
		local t
		t=$(time_ms "$@")
		times+=("$t")
		sum=$((sum + t))
		((t < min)) && min=$t
		((t > max)) && max=$t
	done

	local avg=$((sum / RUNS))

	printf "  %-42s  %6d ms  %6d ms  %6d ms\n" "$label" "$min" "$avg" "$max"
}

run_cmd() {
	local cmd="$1"
	shift
	"$BINARY" "$PROJECT_PATH" --cmd "$cmd" "$@"
}

# ─── build ───────────────────────────────────────────────────────────────────

echo
echo -e "${BOLD}=== CodeLens MCP Benchmark ===${RESET}"
echo -e "Project : $PROJECT_PATH"
echo -e "Binary  : $BINARY"
echo

echo -e "${CYAN}[1/2] Building release binary...${RESET}"
cargo build --release --manifest-path "$(dirname "$(dirname "$BINARY")")/Cargo.toml" 2>&1 ||
	cargo build --release 2>&1 ||
	die "cargo build --release failed"

[[ -x "$BINARY" ]] || die "Binary not found: $BINARY"
echo -e "${GREEN}Build OK${RESET}"
echo

# ─── benchmarks ──────────────────────────────────────────────────────────────

echo -e "${BOLD}[2/2] Running benchmarks (${RUNS} runs each)...${RESET}"
echo
printf "  %-42s  %8s  %8s  %8s\n" "Metric" "Min" "Avg" "Max"
printf "  %-42s  %8s  %8s  %8s\n" \
	"------------------------------------------" "--------" "--------" "--------"

# 1. Cold start + config (wipe index before each run)
INDEX_DIR="$PROJECT_PATH/.codelens/index"
cold_start() {
	rm -rf "$INDEX_DIR"
	run_cmd get_current_config
}
bench "Cold start + get_current_config" cold_start

# 2. Symbol indexing
bench "Symbol indexing (refresh_symbol_index)" \
	run_cmd refresh_symbol_index

# 3. Symbol overview
bench "get_symbols_overview path=src" \
	run_cmd get_symbols_overview --args '{"path":"src"}'

# 4. Find symbol
bench "find_symbol name=main" \
	run_cmd find_symbol --args '{"name":"main"}'

# 5. Impact analysis
bench "get_impact_analysis src/main.rs" \
	run_cmd get_impact_analysis --args '{"file_path":"src/main.rs"}'

echo
printf "  %-42s  %8s  %8s  %8s\n" \
	"------------------------------------------" "--------" "--------" "--------"
echo

# ─── CodeLens vs grep comparison ─────────────────────────────────────────────

echo -e "${BOLD}[3/3] CodeLens vs grep comparison (${RUNS} runs each)...${RESET}"
echo
printf "  %-42s  %8s  %8s  %8s\n" "Comparison" "Min" "Avg" "Max"
printf "  %-42s  %8s  %8s  %8s\n" \
	"------------------------------------------" "--------" "--------" "--------"

# Pick a function name that exists in the project
SEARCH_NAME="dispatch_tool"
SEARCH_FILE="src/main.rs"

# 1. Find function: CodeLens vs grep
bench "CodeLens: find_symbol \"$SEARCH_NAME\"" \
	run_cmd find_symbol --args "{\"name\":\"$SEARCH_NAME\"}"

grep_find() {
	grep -rn "fn $SEARCH_NAME\|def $SEARCH_NAME\|function $SEARCH_NAME" \
		"$PROJECT_PATH" --include="*.rs" --include="*.py" --include="*.ts" --include="*.js"
}
bench "grep: fn $SEARCH_NAME" grep_find

# 2. File structure: CodeLens vs wc+grep
bench "CodeLens: get_symbols_overview" \
	run_cmd get_symbols_overview --args '{"path":"crates/codelens-mcp/src/dispatch.rs"}'

grep_structure() {
	grep -n "^pub\|^fn\|^struct\|^enum\|^impl\|^trait\|^mod" \
		"$PROJECT_PATH/crates/codelens-mcp/src/dispatch.rs"
}
bench "grep: pub/fn/struct patterns" grep_structure

# 3. Callers/references: CodeLens vs grep
bench "CodeLens: find_referencing_symbols" \
	run_cmd find_referencing_symbols --args "{\"symbol_name\":\"$SEARCH_NAME\"}"

grep_refs() {
	grep -rn "$SEARCH_NAME" "$PROJECT_PATH" \
		--include="*.rs" --include="*.py" --include="*.ts"
}
bench "grep: references to $SEARCH_NAME" grep_refs

# 4. Output size comparison (single run)
echo
echo -e "${BOLD}Output size comparison (tokens ≈ bytes/4):${RESET}"
CL_OUT=$("$BINARY" "$PROJECT_PATH" --cmd find_symbol --args "{\"name\":\"$SEARCH_NAME\",\"include_body\":true}" 2>/dev/null)
CL_BYTES=$(echo -n "$CL_OUT" | wc -c | tr -d ' ')
CL_TOKENS=$((CL_BYTES / 4))

GREP_OUT=$(grep -rn "fn $SEARCH_NAME" "$PROJECT_PATH" --include="*.rs" -A 20 2>/dev/null || true)
GREP_BYTES=$(echo -n "$GREP_OUT" | wc -c | tr -d ' ')
GREP_TOKENS=$((GREP_BYTES / 4))

printf "  %-30s  %6d bytes  (~%5d tokens)\n" "CodeLens find_symbol" "$CL_BYTES" "$CL_TOKENS"
printf "  %-30s  %6d bytes  (~%5d tokens)\n" "grep -A 20" "$GREP_BYTES" "$GREP_TOKENS"

if ((GREP_TOKENS > 0 && CL_TOKENS > 0)); then
	RATIO=$(awk "BEGIN{printf \"%.1fx\", $GREP_TOKENS/$CL_TOKENS}")
	echo -e "  ${GREEN}CodeLens is ${RATIO} more token-efficient${RESET}"
fi

echo
printf "  %-42s  %8s  %8s  %8s\n" \
	"------------------------------------------" "--------" "--------" "--------"
echo

# ─── metadata ────────────────────────────────────────────────────────────────

BINARY_BYTES=$(wc -c <"$BINARY" | tr -d ' ')
if ((BINARY_BYTES >= 1048576)); then
	BINARY_SIZE=$(awk "BEGIN{printf \"%.1f MB\", $BINARY_BYTES/1048576}")
else
	BINARY_SIZE=$(awk "BEGIN{printf \"%.1f KB\", $BINARY_BYTES/1024}")
fi

TOOL_COUNT=$("$BINARY" "$PROJECT_PATH" --cmd list_tools 2>/dev/null |
	python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('tools', d.get('result', []))))" 2>/dev/null ||
	echo "N/A")

echo -e "${BOLD}Binary info${RESET}"
echo "  Size       : $BINARY_SIZE"
echo "  Tool count : $TOOL_COUNT"
echo

echo -e "${YELLOW}Done.${RESET}"
