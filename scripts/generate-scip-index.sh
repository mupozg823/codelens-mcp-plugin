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
summary_counts_tsv=$(python3 - "${LOG_PATH}" "${SUMMARY_PATH}" <<'PY'
import collections
import json
import re
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
log_text = log_path.read_text(encoding="utf-8", errors="replace")

duplicate_files = collections.Counter()
previous_source = None
for line in log_text.splitlines():
    source_match = re.match(r"([^\s].*\.rs):\d+:\d+-\d+:\d+$", line)
    if source_match:
        previous_source = source_match.group(1)
        continue
    if "Duplicate symbol:" in line and previous_source:
        duplicate_files[previous_source] += 1
        previous_source = None

missing_files = collections.Counter(
    re.findall(
        r"Bug: definition at ([^:]+):\d+:\d+-\d+:\d+ should have been in an SCIP document",
        log_text,
    )
)
unnamed_enclosing_definition_count = log_text.count(
    "Encountered enclosing definition with no name"
)
duplicate_symbol_count = sum(duplicate_files.values())
missing_document_definition_count = sum(missing_files.values())
precision_risk_warning_count = duplicate_symbol_count + missing_document_definition_count
known_generator_noise_count = unnamed_enclosing_definition_count
warning_count = precision_risk_warning_count + known_generator_noise_count

all_files = sorted(
    set(duplicate_files) | set(missing_files),
    key=lambda path: (
        -(duplicate_files[path] + missing_files[path]),
        -duplicate_files[path],
        path,
    ),
)
file_limit = 25
precision_risk_files = [
    {
        "file_path": path,
        "duplicate_symbol_count": duplicate_files[path],
        "missing_document_definition_count": missing_files[path],
        "total_count": duplicate_files[path] + missing_files[path],
    }
    for path in all_files[:file_limit]
]

summary = {
    "schema_version": 3,
    "generator": "rust-analyzer scip",
    "log_path": ".codelens/scip-generation.log",
    "warning_count": warning_count,
    "precision_risk_warning_count": precision_risk_warning_count,
    "known_generator_noise_count": known_generator_noise_count,
    "precision_risk_file_count": len(all_files),
    "precision_risk_files_truncated": len(all_files) > file_limit,
    "precision_risk_files": precision_risk_files,
    "duplicate_symbol_count": duplicate_symbol_count,
    "missing_document_definition_count": missing_document_definition_count,
    "unnamed_enclosing_definition_count": unnamed_enclosing_definition_count,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=False) + "\n", encoding="utf-8")
print(
    "\t".join(
        str(value)
        for value in (
            warning_count,
            precision_risk_warning_count,
            known_generator_noise_count,
            len(all_files),
            duplicate_symbol_count,
            missing_document_definition_count,
            unnamed_enclosing_definition_count,
        )
    )
)
PY
)
IFS=$'\t' read -r \
	warning_count \
	precision_risk_warning_count \
	known_generator_noise_count \
	precision_risk_file_count \
	duplicate_symbol_count \
	missing_document_definition_count \
	unnamed_enclosing_definition_count <<<"${summary_counts_tsv}"

echo
echo "==> done in ${elapsed}s — ${mb}MB at ${OUTPUT}"
if (( warning_count > 0 )); then
	echo "==> generator warnings: ${warning_count} total (${duplicate_symbol_count} duplicate symbols, ${missing_document_definition_count} missing-document definitions, ${unnamed_enclosing_definition_count} unnamed enclosing definitions)"
	echo "    precision-risk warnings: ${precision_risk_warning_count} across ${precision_risk_file_count} files; known generator noise: ${known_generator_noise_count}"
	echo "    details: ${LOG_PATH}"
fi
echo
echo "next steps:"
echo "  1. Build the MCP server with the scip-backend feature:"
echo "       cargo build --release --features scip-backend --bin codelens-mcp"
echo "  2. Restart any running daemons (launchctl kickstart -k gui/\$(id -u)/dev.codelens.mcp-readonly etc.)."
echo "  3. The next call to get_callers / get_callees will surface SCIP-resolved entries"
echo "     (resolution: \"scip\") alongside the tree-sitter call graph."
