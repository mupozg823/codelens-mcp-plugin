#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${CODELENS_OTEL_COMPOSE_FILE:-$ROOT_DIR/docker-compose.otel.yml}"
PROJECT_DIR="${CODELENS_PROJECT_DIR:-$ROOT_DIR}"
FEATURES="${CODELENS_FEATURES:-http,otel}"
PROFILE="${CODELENS_PROFILE:-planner-readonly}"
OTEL_ENDPOINT="${CODELENS_OTEL_ENDPOINT:-http://127.0.0.1:4317}"
JAEGER_API="${CODELENS_JAEGER_API:-http://127.0.0.1:16686/api/services}"
JAEGER_TRACE_API="${CODELENS_JAEGER_TRACE_API:-http://127.0.0.1:16686/api/traces?service=codelens-mcp&limit=5&lookback=1h}"
BINARY="$ROOT_DIR/target/debug/codelens-mcp"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

wait_for_http() {
  local url="$1"
  local retries="${2:-30}"
  local delay="${3:-1}"
  local attempt
  for attempt in $(seq 1 "$retries"); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$delay"
  done
  return 1
}

require_command cargo
require_command curl
require_command docker

echo "[otel] starting Jaeger via $COMPOSE_FILE"
docker compose -f "$COMPOSE_FILE" up -d jaeger

echo "[otel] waiting for Jaeger API"
if ! wait_for_http "$JAEGER_API" 45 1; then
  echo "Jaeger API did not become ready at $JAEGER_API" >&2
  exit 1
fi

echo "[otel] building codelens-mcp with features: $FEATURES"
cargo build -p codelens-mcp --features "$FEATURES"

echo "[otel] emitting a traced tool call"
CODELENS_OTEL_ENDPOINT="$OTEL_ENDPOINT" \
CODELENS_LOG="${CODELENS_LOG:-info}" \
"$BINARY" "$PROJECT_DIR" --cmd prepare_harness_session --args \
  "{\"profile\":\"$PROFILE\",\"_session_client_name\":\"codex\"}" \
  >/tmp/codelens-otel-prepare.json

services_json=""
for _ in $(seq 1 20); do
  services_json="$(curl -fsS "$JAEGER_API" || true)"
  if printf '%s' "$services_json" | grep -q 'codelens-mcp'; then
    break
  fi
  sleep 1
done

if ! printf '%s' "$services_json" | grep -q 'codelens-mcp'; then
  echo "Jaeger did not report the codelens-mcp service after the traced run" >&2
  exit 1
fi

traces_json="$(curl -fsS "$JAEGER_TRACE_API" || true)"
if ! printf '%s' "$traces_json" | grep -q 'traceID'; then
  echo "Jaeger service exists, but no traces were returned for codelens-mcp" >&2
  exit 1
fi

echo "[otel] verification passed"
echo "[otel] Jaeger UI: http://127.0.0.1:16686/search?service=codelens-mcp"
