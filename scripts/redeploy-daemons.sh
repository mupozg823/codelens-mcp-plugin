#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/redeploy-daemons.sh [repo-root] [options]

Redeploy the repo-local CodeLens HTTP daemons after a fresh build. Encodes
the friction discovered during the 2026-05-18 self-dogfood session:

  1. cp target/release/codelens-mcp → .codelens/bin/codelens-mcp-http
  2. xattr -dr com.apple.provenance ${TARGET}     # macOS gatekeeper
  3. codesign --force --sign - ${TARGET}          # ad-hoc resign
  4. launchctl kickstart -k gui/$UID/${LABEL}     # both daemons
  5. wait for LISTEN on the read-only + mutation ports
  6. (optional) tools/list health probe

Without steps 2-3, every `cargo build` + `cp` cycle ends with
`OS_REASON_CODESIGNING SIGKILL` and KeepAlive loops, leaving the daemons
unable to bind.

Options:
  --label-prefix PREFIX       launchd label prefix (default: dev.codelens.mcp)
  --readonly-port N           read-only daemon port (default: 7839)
  --mutation-port N           mutation daemon port (default: 7838)
  --source PATH               built binary (default: <repo>/target/release/codelens-mcp)
  --target PATH               daemon binary (default: <repo>/.codelens/bin/codelens-mcp-http)
  --skip-readonly             do not kick the read-only daemon
  --skip-mutation             do not kick the mutation daemon
  --build                     also run `cargo build --release --features http,semantic,coreml` first
  --probe                     after kickstart, run a tools/list health probe
  --wait-secs N               LISTEN wait timeout in seconds (default: 12)
  --help                      print this help

Examples:
  bash scripts/redeploy-daemons.sh                    # quick post-build redeploy
  bash scripts/redeploy-daemons.sh --build --probe    # build + redeploy + health probe
  bash scripts/redeploy-daemons.sh --skip-mutation    # only readonly (Cursor untouched)
EOF
}

REPO_ROOT=""
LABEL_PREFIX="dev.codelens.mcp"
READONLY_PORT=7839
MUTATION_PORT=7838
SOURCE_BIN=""
TARGET_BIN=""
SKIP_READONLY=0
SKIP_MUTATION=0
DO_BUILD=0
DO_PROBE=0
WAIT_SECS=12

while [[ $# -gt 0 ]]; do
	case "$1" in
		--label-prefix) LABEL_PREFIX="$2"; shift 2 ;;
		--readonly-port) READONLY_PORT="$2"; shift 2 ;;
		--mutation-port) MUTATION_PORT="$2"; shift 2 ;;
		--source) SOURCE_BIN="$2"; shift 2 ;;
		--target) TARGET_BIN="$2"; shift 2 ;;
		--skip-readonly) SKIP_READONLY=1; shift ;;
		--skip-mutation) SKIP_MUTATION=1; shift ;;
		--build) DO_BUILD=1; shift ;;
		--probe) DO_PROBE=1; shift ;;
		--wait-secs) WAIT_SECS="$2"; shift 2 ;;
		--help|-h) usage; exit 0 ;;
		--*) echo "unknown option: $1" >&2; usage >&2; exit 64 ;;
		*) REPO_ROOT="$1"; shift ;;
	esac
done

if [[ -z "${REPO_ROOT}" ]]; then
	REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi
SOURCE_BIN="${SOURCE_BIN:-${REPO_ROOT}/target/release/codelens-mcp}"
TARGET_BIN="${TARGET_BIN:-${REPO_ROOT}/.codelens/bin/codelens-mcp-http}"

log() { printf '[redeploy] %s\n' "$*"; }

if [[ $DO_BUILD -eq 1 ]]; then
	# Apple Silicon dev-machine native CPU tuning. Detects M-series chip and
	# sets `-C target-cpu=apple-mN` so the release binary uses chip-specific
	# instructions (modest speed-up on hot paths, no portability needed for
	# a daemon that only runs on this host). Skipped when RUSTFLAGS is
	# already set (allow override), in CI (CI env var), or on non-arm64
	# macOS. SIGILL risk: a binary built with -C target-cpu=apple-m4 will
	# fault on M1/M2/M3 — we narrow by chip detection.
	if [[ "$(uname -sm)" == "Darwin arm64" ]] && [[ -z "${RUSTFLAGS:-}" ]] && [[ -z "${CI:-}" ]]; then
		BRAND="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "")"
		case "$BRAND" in
			*M4*) export RUSTFLAGS="-C target-cpu=apple-m4" ;;
			*M3*) export RUSTFLAGS="-C target-cpu=apple-m3" ;;
			*M2*) export RUSTFLAGS="-C target-cpu=apple-m2" ;;
			*M1*) export RUSTFLAGS="-C target-cpu=apple-m1" ;;
		esac
		[[ -n "${RUSTFLAGS:-}" ]] && log "auto-applied RUSTFLAGS=\"${RUSTFLAGS}\" for ${BRAND}"
	fi
	# Default: full language support (lang-extra ON, matches `cargo install` users).
	# Set CODELENS_LANG_MINIMAL=1 to drop the 17 niche tree-sitter languages
	# (go/java/kt/php/swift/scala/rb/cs/dart/zig/ex/hs/ml/erl/r/jl/clj) and
	# shave ~2-3 MB off the binary. Only safe on hosts that don't index those
	# languages — this repo's `~/.codelens/index/symbols.db` had 0 files for
	# all 17 at the time the flag was introduced.
	if [[ "${CODELENS_LANG_MINIMAL:-0}" == "1" ]]; then
		BUILD_FEATURES_ARGS=(--no-default-features --features http,semantic,coreml,scip-backend)
		log "minimal language build (CODELENS_LANG_MINIMAL=1, lang-extra OFF)"
	else
		BUILD_FEATURES_ARGS=(--features http,semantic,coreml)
	fi
	log "cargo build --release ${BUILD_FEATURES_ARGS[*]}"
	(cd "${REPO_ROOT}" && cargo build --release "${BUILD_FEATURES_ARGS[@]}")
fi

if [[ ! -x "${SOURCE_BIN}" ]]; then
	log "ERROR: source binary not found at ${SOURCE_BIN}" >&2
	log "       run with --build, or build manually:" >&2
	log "         cargo build --release --features http,semantic,coreml" >&2
	exit 1
fi

mkdir -p "$(dirname "${TARGET_BIN}")"
log "copying ${SOURCE_BIN} -> ${TARGET_BIN}"
cp -f "${SOURCE_BIN}" "${TARGET_BIN}"

log "stripping com.apple.provenance xattr (macOS gatekeeper)"
xattr -dr com.apple.provenance "${TARGET_BIN}" 2>/dev/null || true

log "ad-hoc resigning ${TARGET_BIN}"
codesign --force --sign - "${TARGET_BIN}"

NEW_VERSION="$("${TARGET_BIN}" --version 2>/dev/null || true)"
if [[ -n "${NEW_VERSION}" ]]; then
	log "deployed: ${NEW_VERSION}"
else
	log "WARNING: ${TARGET_BIN} --version returned no output (codesign may still block)" >&2
fi

UID_VAL="$(id -u)"
KICK_LABELS=()
[[ $SKIP_READONLY -eq 0 ]] && KICK_LABELS+=("${LABEL_PREFIX}-readonly")
[[ $SKIP_MUTATION -eq 0 ]] && KICK_LABELS+=("${LABEL_PREFIX}-mutation")
if [[ ${#KICK_LABELS[@]} -eq 0 ]]; then
	log "both daemons skipped, nothing to kick"
	exit 0
fi

for label in "${KICK_LABELS[@]}"; do
	log "launchctl kickstart -k gui/${UID_VAL}/${label}"
	launchctl kickstart -k "gui/${UID_VAL}/${label}"
done

log "waiting up to ${WAIT_SECS}s for LISTEN sockets"
EXPECTED_PORTS=()
[[ $SKIP_READONLY -eq 0 ]] && EXPECTED_PORTS+=("${READONLY_PORT}")
[[ $SKIP_MUTATION -eq 0 ]] && EXPECTED_PORTS+=("${MUTATION_PORT}")

deadline=$(( $(date +%s) + WAIT_SECS ))
while :; do
	missing=0
	for port in "${EXPECTED_PORTS[@]}"; do
		if ! lsof -nP -iTCP:"${port}" -sTCP:LISTEN >/dev/null 2>&1; then
			missing=1
			break
		fi
	done
	if [[ $missing -eq 0 ]]; then
		break
	fi
	if [[ $(date +%s) -ge $deadline ]]; then
		log "ERROR: not all expected ports are LISTEN after ${WAIT_SECS}s" >&2
		for port in "${EXPECTED_PORTS[@]}"; do
			lsof -nP -iTCP:"${port}" -sTCP:LISTEN 2>/dev/null | head -1 >&2 || \
				log "  port ${port}: no listener" >&2
		done
		log "  inspect: tail .codelens/reports/launchd/${LABEL_PREFIX}-*.err.log" >&2
		exit 3
	fi
	sleep 1
done

for port in "${EXPECTED_PORTS[@]}"; do
	listener="$(lsof -nP -iTCP:"${port}" -sTCP:LISTEN 2>/dev/null | tail -1)"
	log "port ${port}: ${listener}"
done

if [[ $DO_PROBE -eq 1 ]]; then
	for port in "${EXPECTED_PORTS[@]}"; do
		log "tools/list health probe on :${port}"
		response="$(curl -sS --max-time 10 -X POST "http://localhost:${port}/mcp" \
			-H 'Content-Type: application/json' \
			-H 'Accept: application/json, text/event-stream' \
			-d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' || true)"
		if [[ -z "${response}" ]]; then
			log "  port ${port}: probe failed (no response)" >&2
			exit 3
		fi
		# Look for "tool_count" or "tools" array in the SSE payload to confirm the daemon answered.
		if printf '%s' "${response}" | grep -qE '"tool_count"|"tools"\s*:\s*\['; then
			log "  port ${port}: tools/list OK"
		else
			log "  port ${port}: unexpected response — $(printf '%s' "${response}" | head -c 160)" >&2
			exit 3
		fi
	done
fi

log "done"
