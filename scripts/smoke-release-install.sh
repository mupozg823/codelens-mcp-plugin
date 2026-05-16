#!/usr/bin/env bash
# Lightweight pre-release smoke for the default local binary shape.
#
# This is intentionally narrower than release artifact verification:
# it checks that a built binary can report its version, print a valid
# surface manifest, and execute one read-only one-shot tool against the
# current repository.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CODELENS_BIN:-"$ROOT/target/debug/codelens-mcp"}"

if [[ ! -x "$BIN" ]]; then
  cargo build -p codelens-mcp
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

"$BIN" --version >"$TMP_DIR/version.txt"
grep -q '^codelens-mcp ' "$TMP_DIR/version.txt"

"$BIN" --print-surface-manifest >"$TMP_DIR/surface-manifest.json"
python3 - "$TMP_DIR/surface-manifest.json" <<'PY'
import json
import sys

path = sys.argv[1]
manifest = json.load(open(path, encoding="utf-8"))
assert manifest["schema_version"] == "codelens-surface-manifest-v2"
tool_count = manifest["tool_registry"]["definition_count"]
schema_count = manifest["tool_registry"]["output_schema_count"]
assert tool_count >= 70, tool_count
assert schema_count >= 50, schema_count
assert manifest["workspace"]["member_count"] == 2
PY

"$BIN" "$ROOT" --preset minimal --cmd get_current_config --args '{}' \
  >"$TMP_DIR/current-config.json" \
  2>"$TMP_DIR/current-config.stderr"
python3 - "$TMP_DIR/current-config.json" "$ROOT" <<'PY'
import json
import sys

path, expected_root = sys.argv[1], sys.argv[2]
payload = json.load(open(path, encoding="utf-8"))
assert payload["success"] is True
data = payload["data"]
assert data["project_root"] == expected_root, data["project_root"]
assert data["surface"] == "preset:minimal", data["surface"]
assert data["tool_count"] >= 20, data["tool_count"]
PY

printf 'PASS smoke-release-install: %s\n' "$("$BIN" --version)"
