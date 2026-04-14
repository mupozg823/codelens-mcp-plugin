#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${1:-.}"
shift || true
INSTALL_DIR="${CODELENS_INSTALL_DIR:-$HOME/.local/bin}"
NO_BUILD=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir)
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --no-build)
      NO_BUILD=1
      shift
      ;;
    *)
      echo "[sync] unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ ! -d "${ROOT_DIR}" ]]; then
  echo "[sync] project dir not found: ${ROOT_DIR}" >&2
  exit 1
fi

ROOT_DIR="$(cd "${ROOT_DIR}" && pwd)"
BIN_PATH="${ROOT_DIR}/target/release/codelens-mcp"
LINK_PATH="${INSTALL_DIR}/codelens-mcp"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SMOKE_SCRIPT="${SCRIPT_DIR}/mcp-smoke.sh"

if [[ "${NO_BUILD}" -eq 0 ]]; then
  echo "[sync] building release binary"
  cargo build --release -p codelens-mcp --manifest-path "${ROOT_DIR}/Cargo.toml"
fi

if [[ ! -x "${BIN_PATH}" ]]; then
  echo "[sync] release binary not found: ${BIN_PATH}" >&2
  exit 1
fi

mkdir -p "${INSTALL_DIR}"
ln -sf "${BIN_PATH}" "${LINK_PATH}"

echo "[sync] linked ${LINK_PATH} -> ${BIN_PATH}"
if command -v codelens-mcp >/dev/null 2>&1; then
  echo "[sync] PATH command: $(command -v codelens-mcp)"
else
  echo "[sync] note: ${INSTALL_DIR} is not currently on PATH"
fi

tmp_config="$(mktemp)"
rm -f "${tmp_config}"
echo "[sync] verifying linked binary with framed stdio MCP smoke"
CODELENS_BIN="${LINK_PATH}" bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport stdio --config "${tmp_config}"
