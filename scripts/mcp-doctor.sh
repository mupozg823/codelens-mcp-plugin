#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="."
CODEX_CONFIG="${CODEX_CONFIG:-$HOME/.codex/config.toml}"
CLAUDE_CONFIG="${CLAUDE_CONFIG:-$HOME/.claude.json}"
EXPECTED_PORT="${CODELENS_HTTP_PORT:-7837}"
STRICT=0
ALLOW_MISSING_CONFIG=0
EXPECT_TRANSPORT="auto"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SMOKE_SCRIPT="${SCRIPT_DIR}/mcp-smoke.sh"
PROBE_SCRIPT="${SCRIPT_DIR}/mcp_probe.py"

status=0
stdio_smoke_failed=0

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
    --claude-config)
      CLAUDE_CONFIG="${2:-}"
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

inspect_config() {
  python3 "${PROBE_SCRIPT}" "${ROOT_DIR}" --config "$1" --print-config
}

json_field() {
  local json_input="$1"
  local field_path="$2"
  JSON_INPUT="${json_input}" python3 - "$field_path" <<'PY'
import json
import os
import sys

data = json.loads(os.environ["JSON_INPUT"])
value = data
for part in sys.argv[1].split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if value is None:
    print("")
elif isinstance(value, bool):
    print("true" if value else "false")
elif isinstance(value, (list, dict)):
    print(json.dumps(value))
else:
    print(value)
PY
}

summarize_config() {
  local label="$1"
  local json_input="$2"
  local transport target format parse_error
  format="$(json_field "${json_input}" "config_format")"
  transport="$(json_field "${json_input}" "transport")"
  parse_error="$(json_field "${json_input}" "parse_error")"
  target="$(json_field "${json_input}" "url")"
  if [[ -z "${target}" ]]; then
    target="$(json_field "${json_input}" "command")"
  fi
  say "[doctor] ${label} config: ${transport}${target:+ (${target})}${format:+ [${format}]}"
  if [[ -n "${parse_error}" ]]; then
    warn "[doctor] ${label} config parse error: ${parse_error}"
  fi
}

codex_config_json="$(inspect_config "${CODEX_CONFIG}")"
claude_config_json="$(inspect_config "${CLAUDE_CONFIG}")"
config_mode="$(json_field "${codex_config_json}" "transport")"
config_target="$(json_field "${codex_config_json}" "url")"
if [[ -z "${config_target}" ]]; then
  config_target="$(json_field "${codex_config_json}" "command")"
fi

say "[doctor] project: ${ROOT_DIR}"
say "[doctor] codex config path: ${CODEX_CONFIG}"
say "[doctor] claude config path: ${CLAUDE_CONFIG}"
summarize_config "codex" "${codex_config_json}"
summarize_config "claude" "${claude_config_json}"

claude_mode="$(json_field "${claude_config_json}" "transport")"
claude_target="$(json_field "${claude_config_json}" "url")"
if [[ -z "${claude_target}" ]]; then
  claude_target="$(json_field "${claude_config_json}" "command")"
fi

if [[ "${config_mode}" != "missing" && "${claude_mode}" != "missing" ]]; then
  if [[ "${config_mode}" != "${claude_mode}" || "${config_target}" != "${claude_target}" ]]; then
    warn "[doctor] Codex and Claude codelens configs drift: codex=${config_mode}${config_target:+:${config_target}} claude=${claude_mode}${claude_target:+:${claude_target}}"
  fi
fi

if ! bin_path="$(find_binary)"; then
  warn "[doctor] codelens-mcp binary not found"
  exit "${status}"
fi

say "[doctor] binary: ${bin_path}"

if [[ "${config_mode}" == "stdio" && -n "${config_target}" && "${config_target}" == */* ]]; then
  if [[ -L "${config_target}" && ! -e "${config_target}" ]]; then
    warn "[doctor] configured stdio command is a broken symlink: ${config_target}"
  elif [[ ! -e "${config_target}" ]]; then
    warn "[doctor] configured stdio command path does not exist: ${config_target}"
  fi
fi

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
    stdio_smoke_failed=1
    warn "[doctor] stdio smoke failed"
  fi

  if probe_json="$(python3 "${PROBE_SCRIPT}" "${ROOT_DIR}" --transport stdio --config "${CODEX_CONFIG}" 2>/dev/null)"; then
    invocation_source="$(json_field "${probe_json}" "invocation_source")"
    endpoint="$(json_field "${probe_json}" "endpoint")"
    if [[ "${invocation_source}" != "config" ]]; then
      warn "[doctor] configured stdio command was not used directly (invocation_source=${invocation_source}); active endpoint=${endpoint}"
    fi
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

  if probe_json="$(python3 "${PROBE_SCRIPT}" "${ROOT_DIR}" --transport http --config "${CODEX_CONFIG}" 2>/dev/null)"; then
    endpoint="$(json_field "${probe_json}" "endpoint")"
    if [[ -n "${config_target}" && "${endpoint}" != "${config_target}" ]]; then
      warn "[doctor] configured HTTP URL drift: config=${config_target} active=${endpoint}"
    fi
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

if [[ "${config_mode}" == "stdio" && "${stdio_smoke_failed}" -eq 1 ]]; then
  local_probe_config="$(mktemp)"
  rm -f "${local_probe_config}"
  say "[doctor] retrying stdio smoke against the local repo binary for comparison"
  if bash "${SMOKE_SCRIPT}" "${ROOT_DIR}" --transport stdio --config "${local_probe_config}"; then
    if [[ -n "${config_target}" && "${config_target}" == */* ]]; then
      if [[ -L "${config_target}" && ! -e "${config_target}" ]]; then
        warn "[doctor] configured stdio command failed, but the local repo binary passed. The configured command path is a broken symlink: ${config_target}. Update the config or relink that path to this workspace build."
      elif [[ ! -e "${config_target}" ]]; then
        warn "[doctor] configured stdio command failed, but the local repo binary passed. The configured command path does not exist: ${config_target}. Update the config or point it at this workspace build."
      else
        warn "[doctor] configured stdio command failed, but the local repo binary passed. The configured command path differs from the working workspace binary: ${config_target}. Verify that Claude/Codex is pointing at the expected build."
      fi
    else
      warn "[doctor] configured stdio command failed, but the local repo binary passed. The installed \`codelens-mcp\` on your PATH is likely stale or not the same build as this workspace. Run \`bash scripts/sync-local-bin.sh ${ROOT_DIR}\` to relink ~/.local/bin/codelens-mcp to this repo's release build."
    fi
  fi
fi

exit "${status}"
