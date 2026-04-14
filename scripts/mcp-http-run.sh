#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${1:-.}"
PORT="${CODELENS_HTTP_PORT:-7837}"
BIN="${CODELENS_BIN:-${ROOT_DIR}/target/release/codelens-mcp}"

if [[ ! -x "$BIN" ]]; then
  echo "[http-run] release binary not found; building with http feature" >&2
  cargo build --release --features http
fi

if [[ ! -x "$BIN" ]]; then
  echo "[http-run] binary not found after build: ${BIN}" >&2
  exit 1
fi

echo "[http-run] starting on port ${PORT}"
"$BIN" "${ROOT_DIR}" --transport http --port "${PORT}"
