#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="."
CODEX_CONFIG="${CODEX_CONFIG:-$HOME/.codex/config.toml}"
EXPECTED_PORT="${CODELENS_HTTP_PORT:-7837}"
STRICT=0
ALLOW_MISSING_CONFIG=0
EXPECT_TRANSPORT="auto"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SMOKE_SCRIPT="${SCRIPT_DIR}/mcp-smoke.sh"

status=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --strict)
      STRICT=1
      shift
      ;;
    --allow-missing-config)
      ALLOW_MISSING_CONFIG=1
      shift
      ;;
    --expect-transport)
      EXPECT_TRANSPORT="${2:-}"
      shift 2
      ;;
    --config)
      CODEX_CONFIG="${2:-}"
      shift 2
      ;;
    *)
      if [[ "${ROOT_DIR}" == "." ]]; then
        ROOT_DIR="$1"
        shift
      else
        printf '%s\n' "[doctor] unknown argument: $1" >&2
        exit 2
      fi
      ;;
  esac
done

say() {
  printf '%s\n' "$1"
}

warn() {
  printf '%s\n' "$1" >&2
  status=1
}

if [[ "${EXPECT_TRANSPORT}" != "auto" && "${EXPECT_TRANSPORT}" != "stdio" && "${EXPECT_TRANSPORT}" != "http" ]]; then
  printf '%s\n' "[doctor] invalid --expect-transport: ${EXPECT_TRANSPORT}" >&2
  exit 2
fi

find_binary() {
  local candidate
  for candidate in \
    "${CODELENS_BIN:-}" \
    "${ROOT_DIR}/target/release/codelens-mcp" \
    "${ROOT_DIR}/target/debug/codelens-mcp"
  do
    if [[ -n "${candidate}" && -x "${candidate}" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done

  if command -v codelens-mcp >/dev/null 2>&1; then
    command -v codelens-mcp
    return 0
  fi

  return 1
}

config_mode="missing"
config_target=""

if [[ -f "${CODEX_CONFIG}" ]]; then
  config_block="$(awk '
    /^\[mcp_servers\.codelens\]/ { in_block=1; print; next }
    /^\[/ && in_block { exit }
    in_block { print }
  ' "${CODEX_CONFIG}")"

  if [[ -n "${config_block}" ]]; then
    if printf '%s\n' "${config_block}" | rg -q '^[[:space:]]*url[[:space:]]*='; then
      config_mode="http"
      config_target="$(printf '%s\n' "${config_block}" | rg '^[[:space:]]*url[[:space:]]*=' | head -1 | sed -E 's/^[^=]+= *"([^"]+)".*/\1/')"
    elif printf '%s\n' "${config_block}" | rg -q '^[[:space:]]*command[[:space:]]*='; then
      config_mode="stdio"
      config_target="$(printf '%s\n' "${config_block}" | rg '^[[:space:]]*command[[:space:]]*=' | head -1 | sed -E 's/^[^=]+= *"([^"]+)".*/\1/')"
    else
      config_mode="unknown"
    fi
  fi
fi

say "[doctor] project: ${ROOT_DIR}"
say "[doctor] config: ${CODEX_CONFIG}"
say "[doctor] configured mode: ${config_mode}${config_target:+ (${config_target})}"

if ! bin_path="$(find_binary)"; then
  warn "[doctor] codelens-mcp binary not found"
  exit "${status}"
fi

say "[doctor] binary: ${bin_path}"

http_running=0
if lsof -nP -iTCP:"${EXPECTED_PORT}" -sTCP:LISTEN >/dev/null 2>&1; then
  http_running=1
fi

if [[ "${EXPECT_TRANSPORT}" != "auto" ]]; then
  if [[ "${config_mode}" == "missing" && "${ALLOW_MISSING_CONFIG}" -eq 1 ]]; then
    say "[doctor] config missing; continuing because --allow-missing-config was set"
    config_mode="${EXPECT_TRANSPORT}"
  elif [[ "${config_mode}" != "${EXPECT_TRANSPORT}" ]]; then
    warn "[doctor] expected transport ${EXPECT_TRANSPORT}, but config resolves to ${config_mode}"
  fi
fi

if [[ "${config_mode}" == "stdio" ]]; then
  say "[doctor] stdio mode will be tested via framed MCP stdio handshake"
  if bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport stdio --config "${CODEX_CONFIG}"; then
    say "[doctor] stdio smoke passed"
  else
    warn "[doctor] stdio smoke failed"
  fi

  if [[ "${http_running}" -eq 1 ]]; then
    if [[ "${STRICT}" -eq 1 ]]; then
      warn "[doctor] stdio is configured, but an HTTP daemon is also listening on port ${EXPECTED_PORT}"
    else
      say "[doctor] note: http daemon is also running, but Codex is configured for stdio"
    fi
  else
    say "[doctor] no resident http daemon detected; this is fine for stdio"
  fi
elif [[ "${config_mode}" == "http" ]]; then
  if [[ "${http_running}" -eq 1 ]]; then
    say "[doctor] http listener detected on port ${EXPECTED_PORT}"
  else
    warn "[doctor] http mode configured but no listener on port ${EXPECTED_PORT}"
  fi

  if bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport http --config "${CODEX_CONFIG}"; then
    say "[doctor] http smoke passed"
  else
    warn "[doctor] http smoke failed"
  fi
elif [[ "${config_mode}" == "missing" && "${ALLOW_MISSING_CONFIG}" -eq 1 ]]; then
  say "[doctor] no config entry found; running local stdio MCP smoke only"
  if bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport "${EXPECT_TRANSPORT}"; then
    say "[doctor] local smoke passed"
  else
    warn "[doctor] local smoke failed"
  fi
else
  warn "[doctor] no usable codelens entry found in Codex config"
fi

if [[ "${config_mode}" == "stdio" && "${status}" -ne 0 ]]; then
  local_probe_config="$(mktemp)"
  rm -f "${local_probe_config}"
  say "[doctor] retrying stdio smoke against the local repo binary for comparison"
  if bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport stdio --config "${local_probe_config}"; then
    warn "[doctor] configured stdio command failed, but the local repo binary passed. The installed \`codelens-mcp\` on your PATH is likely stale or not the same build as this workspace. Run \`bash scripts/sync-local-bin.sh ${ROOT_DIR}\` to relink ~/.local/bin/codelens-mcp to this repo's release build."
  fi
fi

exit "${status}"
