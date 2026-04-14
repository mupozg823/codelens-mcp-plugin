#!/usr/bin/env bash
# Profile-Guided Optimization (PGO) build for CodeLens MCP.
#
# 3-step process:
# 1. Build instrumented binary (collects profile data)
# 2. Run benchmark workload to generate profile
# 3. Rebuild with profile data for optimized binary
#
# Requires: llvm-profdata (brew install llvm)
# Output: target/release/codelens-mcp (PGO-optimized)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LLVM_PROFDATA="${LLVM_PROFDATA:-/opt/homebrew/Cellar/llvm/21.1.8/bin/llvm-profdata}"
PROFILE_DIR="$ROOT/target/pgo-profiles"
MERGED_PROF="$PROFILE_DIR/merged.profdata"

if [ ! -f "$LLVM_PROFDATA" ]; then
	echo "llvm-profdata not found at $LLVM_PROFDATA"
	echo "Install: brew install llvm"
	echo "Or set LLVM_PROFDATA=/path/to/llvm-profdata"
	exit 1
fi

echo "=== Step 1: Instrumented build ==="
rm -rf "$PROFILE_DIR"
mkdir -p "$PROFILE_DIR"

RUSTFLAGS="-Cprofile-generate=$PROFILE_DIR" \
	cargo build --release --target aarch64-apple-darwin 2>&1 | tail -3

BIN="$ROOT/target/aarch64-apple-darwin/release/codelens-mcp"
if [ ! -f "$BIN" ]; then
	echo "Instrumented binary not found"
	exit 1
fi

echo ""
echo "=== Step 2: Profile workload ==="

# Run the benchmark suite to generate profile data
echo "  Running embedding-quality benchmark..."
python3 benchmarks/embedding-quality.py . \
	--binary "$BIN" \
	--dataset benchmarks/embedding-quality-dataset-self.json \
	--output /dev/null 2>&1 | tail -1

echo "  Running embedding-runtime benchmark..."
python3 benchmarks/embedding-runtime.py . --binary "$BIN" --output /dev/null 2>&1 | tail -1

echo "  Running tool calls..."
for cmd in get_capabilities get_project_structure "find_symbol --args '{\"name\":\"AppState\"}'"; do
	tool=$(echo "$cmd" | awk '{print $1}')
	args=$(echo "$cmd" | sed 's/[^ ]* //' | sed 's/--args //')
	[ "$args" = "$tool" ] && args='{}'
	"$BIN" . --preset balanced --cmd "$tool" --args "$args" >/dev/null 2>&1 || true
done

echo ""
echo "=== Step 3: Merge profiles ==="
"$LLVM_PROFDATA" merge -o "$MERGED_PROF" "$PROFILE_DIR"/*.profraw 2>/dev/null ||
	"$LLVM_PROFDATA" merge -o "$MERGED_PROF" "$PROFILE_DIR" 2>/dev/null

if [ ! -f "$MERGED_PROF" ]; then
	echo "No profile data generated. Aborting."
	exit 1
fi
echo "  Merged profile: $(du -h "$MERGED_PROF" | awk '{print $1}')"

echo ""
echo "=== Step 4: PGO-optimized build ==="
RUSTFLAGS="-Cprofile-use=$MERGED_PROF -Cllvm-args=-pgo-warn-missing-function" \
	cargo build --release --target aarch64-apple-darwin 2>&1 | tail -3

FINAL="$ROOT/target/aarch64-apple-darwin/release/codelens-mcp"
echo ""
echo "=== Done ==="
echo "PGO binary: $FINAL"
echo "Size: $(du -h "$FINAL" | awk '{print $1}')"
echo ""
echo "Benchmark: python3 benchmarks/embedding-runtime.py . --binary $FINAL"
