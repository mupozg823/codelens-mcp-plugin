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
SUMMARY_DIR="${ROOT}/.codelens"
LOG_PATH="${SUMMARY_DIR}/scip-generation.log"
SUMMARY_PATH="${SUMMARY_DIR}/scip-generation-summary.json"

if ! command -v rust-analyzer >/dev/null 2>&1; then
	echo "error: rust-analyzer not found in PATH" >&2
	echo "       install with: brew install rust-analyzer" >&2
	echo "       or:           rustup component add rust-analyzer" >&2
	exit 1
fi

if ! rust-analyzer --help 2>&1 | grep -Eq "^[[:space:]]*(rust-analyzer[[:space:]]+)?scip([[:space:]]|$)"; then
	echo "error: rust-analyzer is too old; missing the \`scip\` subcommand" >&2
	echo "       upgrade rust-analyzer (>= 2024-08 required)" >&2
	exit 1
fi

cd "${ROOT}"
mkdir -p "${SUMMARY_DIR}"
rm -f "${LOG_PATH}" "${SUMMARY_PATH}"

echo "==> generating SCIP index at ${OUTPUT}"
echo "    (this can take 15-60s on a medium-sized workspace)"

start=$(date +%s)
rust-analyzer scip . --output "${OUTPUT}" 2> >(tee "${LOG_PATH}" >&2)
elapsed=$(($(date +%s) - start))

bytes=$(stat -f%z "${OUTPUT}" 2>/dev/null || stat -c%s "${OUTPUT}")
mb=$((bytes / 1024 / 1024))
duplicate_symbol_count=$(grep -c "Duplicate symbol:" "${LOG_PATH}" || true)
missing_document_definition_count=$(grep -c "should have been in an SCIP document" "${LOG_PATH}" || true)
unnamed_enclosing_definition_count=$(grep -c "Encountered enclosing definition with no name" "${LOG_PATH}" || true)
warning_count=$((duplicate_symbol_count + missing_document_definition_count + unnamed_enclosing_definition_count))
precision_risk_warning_count=$((duplicate_symbol_count + missing_document_definition_count))
known_generator_noise_count=${unnamed_enclosing_definition_count}

cat > "${SUMMARY_PATH}" <<JSON
{
  "schema_version": 2,
  "generator": "rust-analyzer scip",
  "log_path": ".codelens/scip-generation.log",
  "warning_count": ${warning_count},
  "precision_risk_warning_count": ${precision_risk_warning_count},
  "known_generator_noise_count": ${known_generator_noise_count},
  "duplicate_symbol_count": ${duplicate_symbol_count},
  "missing_document_definition_count": ${missing_document_definition_count},
  "unnamed_enclosing_definition_count": ${unnamed_enclosing_definition_count}
}
JSON

echo
echo "==> done in ${elapsed}s — ${mb}MB at ${OUTPUT}"
if (( warning_count > 0 )); then
	echo "==> generator warnings: ${warning_count} total (${duplicate_symbol_count} duplicate symbols, ${missing_document_definition_count} missing-document definitions, ${unnamed_enclosing_definition_count} unnamed enclosing definitions)"
	echo "    precision-risk warnings: ${precision_risk_warning_count}; known generator noise: ${known_generator_noise_count}"
	echo "    details: ${LOG_PATH}"
fi
echo
echo "next steps:"
echo "  1. Build the MCP server with the scip-backend feature:"
echo "       cargo build --release --features scip-backend --bin codelens-mcp"
echo "  2. Restart any running daemons (launchctl kickstart -k gui/\$(id -u)/dev.codelens.mcp-readonly etc.)."
echo "  3. The next call to get_callers / get_callees will surface SCIP-resolved entries"
echo "     (resolution: \"scip\") alongside the tree-sitter call graph."
