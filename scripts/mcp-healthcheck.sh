#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${1:-.}"
CODEX_CONFIG="${CODEX_CONFIG:-$HOME/.codex/config.toml}"
EXPECTED_PORT="${CODELENS_HTTP_PORT:-7837}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SMOKE_SCRIPT="${SCRIPT_DIR}/mcp-smoke.sh"

echo "[health] checking codelens-mcp runtime + Codex MCP config"

if ps -ax -o pid=,command= | rg -q "codelens-mcp .*--preset"; then
  ps -ax -o pid=,rss=,etime=,command= | rg "codelens-mcp .*--preset" | head -5
else
  echo "[health] no codelens-mcp stdio process detected"
fi

if lsof -nP -iTCP:${EXPECTED_PORT} -sTCP:LISTEN >/dev/null 2>&1; then
  echo "[health] HTTP listener detected on port ${EXPECTED_PORT}"
else
  echo "[health] no HTTP listener on port ${EXPECTED_PORT}"
fi

if [[ -f "${CODEX_CONFIG}" ]]; then
  echo "[health] Codex config: ${CODEX_CONFIG}"
  if rg -q "mcp_servers\\.codelens" "${CODEX_CONFIG}"; then
    rg -n "mcp_servers\\.codelens|url\\s*=|command\\s*=|args\\s*=" "${CODEX_CONFIG}"
  else
    echo "[health] no codelens entry in Codex config"
  fi
else
  echo "[health] Codex config not found: ${CODEX_CONFIG}"
fi

echo "[health] live MCP smoke:"
if bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport auto --config "${CODEX_CONFIG}"; then
  echo "[health] live MCP smoke passed"
else
  echo "[health] live MCP smoke failed"
fi
