#!/usr/bin/env bash
# Generate a SCIP index for the current Rust workspace so CodeLens can
# wire its `scip-backend` feature into get_callers / get_callees and any
# follow-up type-aware tooling.
#
# Output: ./index.scip (auto-detected by ScipBackend::detect at session
# startup; gitignored as of L1 slice 1).
#
# Requires: rust-analyzer (>= 2024-08, supports `scip` subcommand).
#   brew install rust-analyzer       # macOS
#   rustup component add rust-analyzer
#
# Re-run this whenever Cargo.toml dependencies change or major refactors
# move/rename function symbols. The MCP server picks up the new index on
# its next restart (or first scip() access if not previously cached).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${ROOT}/index.scip"

if ! command -v rust-analyzer >/dev/null 2>&1; then
	echo "error: rust-analyzer not found in PATH" >&2
	echo "       install with: brew install rust-analyzer" >&2
	echo "       or:           rustup component add rust-analyzer" >&2
	exit 1
fi

if ! rust-analyzer --help 2>&1 | grep -q "^[[:space:]]*scip"; then
	echo "error: rust-analyzer is too old; missing the \`scip\` subcommand" >&2
	echo "       upgrade rust-analyzer (>= 2024-08 required)" >&2
	exit 1
fi

cd "${ROOT}"
echo "==> generating SCIP index at ${OUTPUT}"
echo "    (this can take 15-60s on a medium-sized workspace)"

start=$(date +%s)
rust-analyzer scip . --output "${OUTPUT}"
elapsed=$(($(date +%s) - start))

bytes=$(stat -f%z "${OUTPUT}" 2>/dev/null || stat -c%s "${OUTPUT}")
mb=$((bytes / 1024 / 1024))

echo
echo "==> done in ${elapsed}s — ${mb}MB at ${OUTPUT}"
echo
echo "next steps:"
echo "  1. Build the MCP server with the scip-backend feature:"
echo "       cargo build --release --features scip-backend --bin codelens-mcp"
echo "  2. Restart any running daemons (launchctl kickstart -k gui/\$(id -u)/dev.codelens.mcp-readonly etc.)."
echo "  3. The next call to get_callers / get_callees will surface SCIP-resolved entries"
echo "     (resolution: \"scip\") alongside the tree-sitter call graph."
