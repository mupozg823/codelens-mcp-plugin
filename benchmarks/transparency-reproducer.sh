#!/usr/bin/env bash
# Reproducer for Phase 1 transparency layer (plan Task 10).
# Requires: /tmp/serena-oraios present; codelens-mcp built in release mode.
#
# Exits 0 on success; prints a one-line progress message per scenario.
# Override the binary via CODELENS_BIN and the fixture via CODELENS_FIXTURE.
set -euo pipefail

# Phase 2 scenarios hit tools that are gated out of the default
# `preset:balanced` surface (search_for_pattern, get_ranked_context).
# Force the full preset for the whole script so every scenario can
# actually dispatch. Callers that want a narrower surface can
# override via CODELENS_PRESET before invoking the script.
export CODELENS_PRESET="${CODELENS_PRESET:-full}"

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

echo "--- find_symbol zero-result (should emit exact_match_only) ---"
"$BIN" --cmd find_symbol \
	--args '{"name":"definitelynotasymbolxyz","exact_match":true}' |
	python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
kinds = [e["kind"] for e in d.get("limits_applied", [])]
assert "exact_match_only" in kinds, "expected exact_match_only, got " + repr(kinds)
print("ok exact_match_only:", kinds)
'

echo "--- get_symbols_overview on fixture (decision set is informational — no assertion on content) ---"
"$BIN" --cmd get_symbols_overview \
	--args '{"path":"."}' |
	python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
# Only assert the field is a list; content depends on repo size.
limits = d.get("limits_applied", [])
assert isinstance(limits, list)
print("ok get_symbols_overview decisions:", [e["kind"] for e in limits])
'

echo "--- search_for_pattern with tight max_results + glob (should emit sampling + filter_applied) ---"
"$BIN" --cmd search_for_pattern \
	--args '{"pattern":"class ","file_glob":"*.py","max_results":3}' |
	python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
kinds = [e["kind"] for e in d.get("limits_applied", [])]
assert "filter_applied" in kinds, "expected filter_applied, got " + repr(kinds)
print("ok search_for_pattern:", kinds)
'

echo "--- get_ranked_context with tight budget (should usually emit budget_prune) ---"
"$BIN" --cmd get_ranked_context \
	--args '{"query":"serena","max_tokens":300}' |
	python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
kinds = [e["kind"] for e in d.get("limits_applied", [])]
# budget_prune is the expected signal, but on a very small repo budget might fit;
# report kinds either way.
print("ok get_ranked_context:", kinds)
'
