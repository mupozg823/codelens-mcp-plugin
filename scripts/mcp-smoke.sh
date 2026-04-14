#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${1:-.}"
shift || true
TRANSPORT="auto"
CONFIG_PATH="${CODEX_CONFIG:-$HOME/.codex/config.toml}"
URL_OVERRIDE=""
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROBE_SCRIPT="${SCRIPT_DIR}/mcp_probe.py"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --transport)
      TRANSPORT="${2:-}"
      shift 2
      ;;
    --config)
      CONFIG_PATH="${2:-}"
      shift 2
      ;;
    --url)
      URL_OVERRIDE="${2:-}"
      shift 2
      ;;
    *)
      echo "[smoke] unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ ! -d "${ROOT_DIR}" ]]; then
  echo "[smoke] project dir not found: ${ROOT_DIR}" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "[smoke] python3 is required" >&2
  exit 1
fi

probe_cmd=(python3 "${PROBE_SCRIPT}" "${ROOT_DIR}" --transport "${TRANSPORT}" --config "${CONFIG_PATH}")
if [[ -n "${URL_OVERRIDE}" ]]; then
  probe_cmd+=(--url "${URL_OVERRIDE}")
fi

if probe_json="$("${probe_cmd[@]}")"; then
  PROBE_JSON="${probe_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["PROBE_JSON"])
prepare = data.get("prepare_harness_session", {})
endpoint = data.get("endpoint")
if isinstance(endpoint, list):
    endpoint = " ".join(endpoint)
print(f"[smoke] transport: {data.get('transport')}")
print(f"[smoke] endpoint: {endpoint}")
init = data.get("initialize", {})
print(
    f"[smoke] initialize: ok "
    f"(protocol={init.get('protocol_version')}, server={init.get('server')})"
)
print(
    f"[smoke] prepare_harness_session: ok "
    f"(surface={prepare.get('active_surface')}, visible_tools={prepare.get('visible_tool_count')}, warnings={prepare.get('warning_count')})"
)
stderr = data.get("stderr")
if stderr:
    print(f"[smoke] stderr: {stderr}")
PY
  exit 0
fi

echo "[smoke] failed"
echo "${probe_json}"
exit 2
