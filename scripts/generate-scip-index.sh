#!/usr/bin/env bash
# Generate SCIP index(es) for the current workspace so CodeLens can
# wire its `scip-backend` feature into get_callers / get_callees and any
# follow-up type-aware tooling.
#
# Languages are detected by source-file count; every detected language
# with an installed indexer is run:
#   rust                  rust-analyzer scip   (>= 2024-08, `scip` subcommand)
#   python                scip-python          (npm install -g @sourcegraph/scip-python)
#   typescript/javascript scip-typescript      (npm install -g @sourcegraph/scip-typescript)
# Detected languages whose indexer is missing are skipped with an install
# hint; the script fails only when no indexer is available at all.
#
# Output contract:
#   ./index.scip — the PRIMARY language (most files among installed
#   indexers). ScipBackend::detect loads only the first of index.scip,
#   .scip/index.scip, .codelens/index.scip — so the primary artifact must
#   land here. Secondary languages are indexed to
#   .codelens/index-<lang>.scip as separate artifacts; SCIP tooling has no
#   official multi-index merge, so they are NOT merged and the engine's
#   precise tier stays primary-language-only until it grows multi-index
#   support. (Auto-detected at session startup; gitignored as of L1 slice 1.)
#
# Re-run this whenever dependencies change or major refactors move/rename
# function symbols. The MCP server picks up the new index on its next
# restart (or first scip() access if not previously cached).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${ROOT}/index.scip"
SUMMARY_DIR="${ROOT}/.codelens"
LOG_PATH="${SUMMARY_DIR}/scip-generation.log"
SUMMARY_PATH="${SUMMARY_DIR}/scip-generation-summary.json"

generator_name_for() {
	case "$1" in
	rust) echo "rust-analyzer scip" ;;
	python) echo "scip-python" ;;
	typescript) echo "scip-typescript" ;;
	esac
}

# Per-generator stderr/stdout log. Only ${LOG_PATH} (rust) is consumed by
# the warning parser below — its format is rust-analyzer-specific and the
# other generators' logs must never be fed through it.
log_path_for() {
	case "$1" in
	rust) echo "${LOG_PATH}" ;;
	python) echo "${SUMMARY_DIR}/scip-generation-python.log" ;;
	typescript) echo "${SUMMARY_DIR}/scip-generation-typescript.log" ;;
	esac
}

# ok | missing | too_old
indexer_status_for() {
	case "$1" in
	rust)
		if ! command -v rust-analyzer >/dev/null 2>&1; then
			echo "missing"
		elif ! rust-analyzer --help 2>&1 | grep -Eq "^[[:space:]]*(rust-analyzer[[:space:]]+)?scip([[:space:]]|$)"; then
			echo "too_old"
		else
			echo "ok"
		fi
		;;
	python)
		if command -v scip-python >/dev/null 2>&1; then echo "ok"; else echo "missing"; fi
		;;
	typescript)
		if command -v scip-typescript >/dev/null 2>&1; then echo "ok"; else echo "missing"; fi
		;;
	esac
}

print_install_hint() {
	case "$1" in
	rust)
		if [ "$2" = "too_old" ]; then
			echo "       rust-analyzer is too old; missing the \`scip\` subcommand" >&2
			echo "       upgrade rust-analyzer (>= 2024-08 required)" >&2
		else
			echo "       install with: brew install rust-analyzer" >&2
			echo "       or:           rustup component add rust-analyzer" >&2
		fi
		;;
	python)
		echo "       install with: npm install -g @sourcegraph/scip-python" >&2
		;;
	typescript)
		echo "       install with: npm install -g @sourcegraph/scip-typescript" >&2
		;;
	esac
}

list_source_files() {
	if git -C "${ROOT}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
		git -C "${ROOT}" ls-files --cached --others --exclude-standard
	else
		find "${ROOT}" \
			\( -name .git -o -name node_modules -o -name target -o -name dist \
			-o -name build -o -name .venv -o -name __pycache__ -o -name .codelens \) -prune \
			-o -type f -print
	fi
}

run_indexer() {
	local lang="$1" out="$2" log="$3"
	case "${lang}" in
	rust)
		rust-analyzer scip . --output "${out}" 2> >(tee "${log}" >&2)
		;;
	python)
		scip-python index . --project-name "$(basename "${ROOT}")" --output "${out}" 2>&1 | tee "${log}"
		;;
	typescript)
		if [ -f "${ROOT}/tsconfig.json" ]; then
			scip-typescript index --output "${out}" 2>&1 | tee "${log}"
		else
			scip-typescript index --infer-tsconfig --output "${out}" 2>&1 | tee "${log}"
		fi
		;;
	esac
}

cd "${ROOT}"
mkdir -p "${SUMMARY_DIR}"
rm -f "${LOG_PATH}" "${SUMMARY_PATH}" \
	"${SUMMARY_DIR}/scip-generation-python.log" \
	"${SUMMARY_DIR}/scip-generation-typescript.log" \
	"${SUMMARY_DIR}/index-rust.scip" \
	"${SUMMARY_DIR}/index-python.scip" \
	"${SUMMARY_DIR}/index-typescript.scip"

FILES_TMP="$(mktemp)"
GEN_MANIFEST="$(mktemp)"
trap 'rm -f "${FILES_TMP}" "${GEN_MANIFEST}"' EXIT
list_source_files >"${FILES_TMP}"

rust_count="$(grep -cE '\.rs$' "${FILES_TMP}" || true)"
python_count="$(grep -cE '\.py$' "${FILES_TMP}" || true)"
typescript_count="$(grep -cE '\.(ts|tsx|js|jsx)$' "${FILES_TMP}" || true)"

file_count_for() {
	case "$1" in
	rust) echo "${rust_count}" ;;
	python) echo "${python_count}" ;;
	typescript) echo "${typescript_count}" ;;
	esac
}

# language \t generator \t status \t output(rel, "-" if none) \t elapsed \t file_count
record_generator() {
	printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "$6" >>"${GEN_MANIFEST}"
}

# Detected languages ordered by file count desc; ties break on the fixed
# rust > python > typescript order so the primary choice is deterministic.
DETECTED="$(
	{
		[ "${rust_count}" -gt 0 ] && echo "${rust_count} 0 rust"
		[ "${python_count}" -gt 0 ] && echo "${python_count} 1 python"
		[ "${typescript_count}" -gt 0 ] && echo "${typescript_count} 2 typescript"
		true
	} | sort -k1,1nr -k2,2n | awk '{print $3}'
)"

if [ -z "${DETECTED}" ]; then
	echo "error: no supported source files detected (rust/python/typescript)" >&2
	exit 1
fi

echo "==> detected languages (by file count):"
while IFS= read -r lang; do
	echo "    ${lang}: $(file_count_for "${lang}") files"
done <<<"${DETECTED}"

AVAILABLE=""
while IFS= read -r lang; do
	status="$(indexer_status_for "${lang}")"
	if [ "${status}" = "ok" ]; then
		if [ -z "${AVAILABLE}" ]; then
			AVAILABLE="${lang}"
		else
			AVAILABLE="${AVAILABLE}
${lang}"
		fi
	else
		echo "==> skipping ${lang} ($(file_count_for "${lang}") files): $(generator_name_for "${lang}") not available (${status})" >&2
		print_install_hint "${lang}" "${status}"
		record_generator "${lang}" "$(generator_name_for "${lang}")" "skipped_missing_indexer" "-" 0 "$(file_count_for "${lang}")"
	fi
done <<<"${DETECTED}"

if [ -z "${AVAILABLE}" ]; then
	echo "error: no SCIP indexer available for the detected language(s)" >&2
	exit 1
fi

PRIMARY_LANG="$(head -n1 <<<"${AVAILABLE}")"
SECONDARY_LANGS="$(tail -n +2 <<<"${AVAILABLE}")"
PRIMARY_GENERATOR="$(generator_name_for "${PRIMARY_LANG}")"

# Secondary generators run first so the primary artifact at ${OUTPUT} is
# written last — scip_health gates on summary mtime >= index mtime, and the
# summary below must be the final write.
if [ -n "${SECONDARY_LANGS}" ]; then
	while IFS= read -r lang; do
		# NOTE: never name these index.scip — .codelens/index.scip is a
		# ScipBackend::detect fallback candidate and must stay reserved.
		secondary_out="${SUMMARY_DIR}/index-${lang}.scip"
		secondary_log="$(log_path_for "${lang}")"
		echo "==> generating secondary SCIP index (${lang}) at ${secondary_out}"
		secondary_start=$(date +%s)
		if run_indexer "${lang}" "${secondary_out}" "${secondary_log}"; then
			secondary_elapsed=$(($(date +%s) - secondary_start))
			record_generator "${lang}" "$(generator_name_for "${lang}")" "ok" ".codelens/index-${lang}.scip" "${secondary_elapsed}" "$(file_count_for "${lang}")"
			echo "==> secondary ${lang} index done in ${secondary_elapsed}s"
		else
			secondary_elapsed=$(($(date +%s) - secondary_start))
			record_generator "${lang}" "$(generator_name_for "${lang}")" "failed" "-" "${secondary_elapsed}" "$(file_count_for "${lang}")"
			echo "warning: ${lang} indexer failed (see ${secondary_log}); continuing — secondary indexes are optional" >&2
		fi
	done <<<"${SECONDARY_LANGS}"
fi

echo "==> generating SCIP index (${PRIMARY_LANG}, ${PRIMARY_GENERATOR}) at ${OUTPUT}"
echo "    (this can take 15-60s on a medium-sized workspace)"

start=$(date +%s)
run_indexer "${PRIMARY_LANG}" "${OUTPUT}" "$(log_path_for "${PRIMARY_LANG}")"
elapsed=$(($(date +%s) - start))
record_generator "${PRIMARY_LANG}" "${PRIMARY_GENERATOR}" "ok" "index.scip" "${elapsed}" "$(file_count_for "${PRIMARY_LANG}")"

bytes=$(stat -f%z "${OUTPUT}" 2>/dev/null || stat -c%s "${OUTPUT}")
mb=$((bytes / 1024 / 1024))
summary_counts_tsv=$(python3 - "${LOG_PATH}" "${SUMMARY_PATH}" "${GEN_MANIFEST}" "${PRIMARY_GENERATOR}" <<'PY'
import collections
import json
import re
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
manifest_path = Path(sys.argv[3])
primary_generator = sys.argv[4]
# The rust-analyzer log only exists when the rust generator ran; every
# warning pattern below is rust-analyzer-log-format-specific, so other
# generators' logs must never be parsed here.
log_text = (
    log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
)

generators = []
if manifest_path.exists():
    for line in manifest_path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        language, name, status, output, elapsed_seconds, file_count = line.split("\t")
        generators.append(
            {
                "language": language,
                "name": name,
                "status": status,
                "output": None if output == "-" else output,
                "elapsed_seconds": int(elapsed_seconds),
                "file_count": int(file_count),
            }
        )

duplicate_files = collections.Counter()
known_duplicate_files = collections.Counter()
previous_source = None
root = Path.cwd()
for line in log_text.splitlines():
    source_match = re.match(r"([^\s].*\.rs):\d+:\d+-\d+:\d+$", line)
    if source_match:
        previous_source = source_match.group(1)
        continue
    if "Duplicate symbol:" in line and previous_source:
        # rust-analyzer emits duplicate SCIP symbols for crate/test/bench
        # entrypoints that do not map to user-authored call targets. Keep
        # counting them, but do not mix them with file/symbol precision risk.
        known_entrypoint_duplicate = (
            line.rstrip().endswith(" crate/")
            or line.rstrip().endswith(" main().")
            or line.rstrip().endswith(" benches().")
            or "/tests/" in previous_source
            or "/benches/" in previous_source
        )
        if known_entrypoint_duplicate:
            known_duplicate_files[previous_source] += 1
        else:
            duplicate_files[previous_source] += 1
        previous_source = None

missing_definition_pattern = re.compile(
    r"Bug: definition at ([^:]+):(\d+):(\d+)-(\d+):(\d+) should have been in an SCIP document"
)

def in_derive_attribute(file_path: str, zero_based_line: int) -> bool:
    try:
        lines = (root / file_path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return False
    # rust-analyzer SCIP log locations are zero-based. Probe adjacent rows too
    # so the classifier remains stable if upstream flips this convention.
    for candidate in (zero_based_line, zero_based_line - 1, zero_based_line + 1):
        if not 0 <= candidate < len(lines):
            continue
        start = candidate
        while start >= 0 and start >= candidate - 6:
            if re.match(r"\s*#\s*\[\s*derive\s*\(", lines[start]):
                end = start
                while end < len(lines) and end <= start + 8:
                    if candidate <= end and "]" in lines[end]:
                        return True
                    if "]" in lines[end]:
                        break
                    end += 1
                break
            if lines[start].strip() and not lines[start].lstrip().startswith("#"):
                break
            start -= 1
    return False

missing_files = collections.Counter()
known_missing_files = collections.Counter()
for match in missing_definition_pattern.finditer(log_text):
    file_path = match.group(1)
    zero_based_line = int(match.group(2))
    if in_derive_attribute(file_path, zero_based_line):
        known_missing_files[file_path] += 1
    else:
        missing_files[file_path] += 1
unnamed_enclosing_definition_count = log_text.count(
    "Encountered enclosing definition with no name"
)
precision_risk_duplicate_symbol_count = sum(duplicate_files.values())
known_duplicate_symbol_count = sum(known_duplicate_files.values())
duplicate_symbol_count = precision_risk_duplicate_symbol_count + known_duplicate_symbol_count
precision_risk_missing_document_definition_count = sum(missing_files.values())
known_missing_document_definition_count = sum(known_missing_files.values())
missing_document_definition_count = (
    precision_risk_missing_document_definition_count
    + known_missing_document_definition_count
)
precision_risk_warning_count = (
    precision_risk_duplicate_symbol_count
    + precision_risk_missing_document_definition_count
)
known_generator_noise_count = (
    unnamed_enclosing_definition_count
    + known_duplicate_symbol_count
    + known_missing_document_definition_count
)
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
    "schema_version": 5,
    "generator": primary_generator,
    "generators": generators,
    "log_path": ".codelens/scip-generation.log",
    "warning_count": warning_count,
    "precision_risk_warning_count": precision_risk_warning_count,
    "known_generator_noise_count": known_generator_noise_count,
    "precision_risk_file_count": len(all_files),
    "precision_risk_files_truncated": len(all_files) > file_limit,
    "precision_risk_files": precision_risk_files,
    "duplicate_symbol_count": duplicate_symbol_count,
    "precision_risk_duplicate_symbol_count": precision_risk_duplicate_symbol_count,
    "known_duplicate_symbol_count": known_duplicate_symbol_count,
    "missing_document_definition_count": missing_document_definition_count,
    "precision_risk_missing_document_definition_count": precision_risk_missing_document_definition_count,
    "known_missing_document_definition_count": known_missing_document_definition_count,
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
if [ -n "${SECONDARY_LANGS}" ]; then
	echo
	echo "note: the engine loads only ${OUTPUT} (ScipBackend::detect contract); secondary"
	echo "      language indexes under .codelens/ are separate artifacts and are NOT merged"
	echo "      — precise-tier navigation stays ${PRIMARY_LANG}-only for now."
fi
echo
echo "next steps:"
echo "  1. Build the MCP server with the scip-backend feature:"
echo "       cargo build --release --features scip-backend --bin codelens-mcp"
echo "  2. Restart any running daemons (launchctl kickstart -k gui/\$(id -u)/dev.codelens.mcp-readonly etc.)."
echo "  3. The next call to get_callers / get_callees will surface SCIP-resolved entries"
echo "     (resolution: \"scip\") alongside the tree-sitter call graph."
