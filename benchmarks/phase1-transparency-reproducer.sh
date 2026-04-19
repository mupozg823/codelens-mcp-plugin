#!/usr/bin/env bash
# Reproducer for Phase 1 transparency layer (plan Task 10).
# Requires: /tmp/serena-oraios present; codelens-mcp built in release mode.
#
# Exits 0 on success; prints a one-line progress message per scenario.
# Override the binary via CODELENS_BIN and the fixture via CODELENS_FIXTURE.
set -euo pipefail

BIN="${CODELENS_BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/codelens-mcp}"
FIXTURE="${CODELENS_FIXTURE:-/tmp/serena-oraios}"

if [[ ! -d "$FIXTURE" ]]; then
	echo "Fixture $FIXTURE not found. Set CODELENS_FIXTURE to override." >&2
	exit 2
fi
if [[ ! -x "$BIN" ]]; then
	echo "Binary $BIN not executable. Run: cargo build --release -p codelens-mcp" >&2
	exit 2
fi

cd "$FIXTURE"

echo "--- default call (should be sampled) ---"
"$BIN" --cmd find_referencing_symbols \
	--args '{"symbol_name":"SerenaAgent","file_path":"src/serena/agent.py"}' |
	python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
limits = d.get("limits_applied", [])
sampled = d.get("sampled")
assert sampled is True, "expected sampled=true, got " + repr(sampled)
kinds = [e["kind"] for e in limits]
assert "sampling" in kinds, "expected sampling decision, got " + repr(kinds)
print("ok sampling:", kinds)
'

echo "--- full_results call (should NOT have a sampling decision) ---"
"$BIN" --cmd find_referencing_symbols \
	--args '{"symbol_name":"SerenaAgent","file_path":"src/serena/agent.py","full_results":true,"max_results":500}' |
	python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
limits = d.get("limits_applied", [])
sampled = d.get("sampled")
assert sampled is False, "expected sampled=false, got " + repr(sampled)
kinds = [e["kind"] for e in limits]
assert "sampling" not in kinds, "sampling decision must not appear when all results returned: " + repr(kinds)
print("ok full_results:", kinds)
'
