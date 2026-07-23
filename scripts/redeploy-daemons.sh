#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/redeploy-daemons.sh [repo-root] [options]

Redeploy the canonical repo-local CodeLens HTTP daemon after a fresh build. Encodes
the friction discovered during the 2026-05-18 self-dogfood session:

  1. cp target/release/codelens-mcp → .codelens/bin/codelens-mcp-http
  2. xattr -dr com.apple.provenance ${TARGET}     # macOS gatekeeper
  3. codesign --force --sign - ${TARGET}          # ad-hoc resign
  4. launchctl bootout/bootstrap + kickstart      # refresh launchd LWCR
  5. wait for LISTEN on the canonical mutation port
  6. (optional) tools/list health probe on that one endpoint

The legacy readonly label is explicitly disabled and booted out on every run;
it is never bootstrapped. Reviewer/planner and builder/refactor sessions share
the canonical endpoint and select their profile/RBAC per session.

Without steps 2-3, every `cargo build` + `cp` cycle ends with
`OS_REASON_CODESIGNING SIGKILL` and KeepAlive loops, leaving the daemons
unable to bind.

Options:
  --label-prefix PREFIX       launchd label prefix (default: dev.codelens.mcp)
  --readonly-port N           deprecated compatibility alias; maps to the
                              canonical port only when --mutation-port is absent
  --mutation-port N           canonical daemon port (default: 7838)
  --source PATH               built binary (default: <repo>/target/release/codelens-mcp)
  --target PATH               daemon binary (default: <repo>/.codelens/bin/codelens-mcp-http)
  --skip-readonly             deprecated no-op (readonly is never started)
  --skip-mutation             skip the canonical daemon restart
  --build                     also run `cargo build --release --features http,semantic,coreml` first
  --probe                     after kickstart, run a tools/list health probe
  --wait-secs N               LISTEN wait timeout in seconds (default: 12)
  --help                      print this help

Environment:
  CODELENS_PORT_RELEASE_SECS  after bootout, seconds to wait for the old daemon
                              to release its port before bootstrap (default: 15).
                              On timeout the redeploy aborts BEFORE bootstrap so a
                              fresh instance never meets the busy port and yields
                              exit(0) into a KeepAlive=SuccessfulExit=false down.

Examples:
  bash scripts/redeploy-daemons.sh                    # quick post-build redeploy
  bash scripts/redeploy-daemons.sh --build --probe    # build + redeploy + health probe
  bash scripts/redeploy-daemons.sh --skip-mutation    # copy only; do not restart
EOF
}

REPO_ROOT=""
LABEL_PREFIX="dev.codelens.mcp"
READONLY_PORT=7839
MUTATION_PORT=7838
READONLY_PORT_EXPLICIT=0
MUTATION_PORT_EXPLICIT=0
SOURCE_BIN=""
TARGET_BIN=""
SKIP_READONLY=0
SKIP_MUTATION=0
DO_BUILD=0
DO_PROBE=0
WAIT_SECS=12
PORT_RELEASE_SECS="${CODELENS_PORT_RELEASE_SECS:-15}"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--label-prefix) LABEL_PREFIX="$2"; shift 2 ;;
		--readonly-port) READONLY_PORT="$2"; READONLY_PORT_EXPLICIT=1; shift 2 ;;
		--mutation-port) MUTATION_PORT="$2"; MUTATION_PORT_EXPLICIT=1; shift 2 ;;
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

if [[ "$READONLY_PORT_EXPLICIT" == "1" ]]; then
	if [[ "$MUTATION_PORT_EXPLICIT" == "0" ]]; then
		MUTATION_PORT="$READONLY_PORT"
		printf '[redeploy] warning: --readonly-port is deprecated; using %s as the canonical single-writer port\n' "${MUTATION_PORT}" >&2
	else
		printf '[redeploy] warning: --readonly-port=%s is ignored; canonical --mutation-port=%s wins\n' "${READONLY_PORT}" "${MUTATION_PORT}" >&2
	fi
fi

if [[ -z "${REPO_ROOT}" ]]; then
	REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi
SOURCE_BIN="${SOURCE_BIN:-${REPO_ROOT}/target/release/codelens-mcp}"
TARGET_BIN="${TARGET_BIN:-${REPO_ROOT}/.codelens/bin/codelens-mcp-http}"

log() { printf '[redeploy] %s\n' "$*"; }

# 0 = something is accepting on 127.0.0.1:port (a daemon still holds it),
# non-zero = nothing is listening. Uses the bash /dev/tcp builtin so it needs
# no lsof/nc and behaves identically on macOS and Linux CI.
port_is_listening() {
	local port="$1"
	(exec 3<>"/dev/tcp/127.0.0.1/${port}") >/dev/null 2>&1
}

# Block until 127.0.0.1:port stops accepting connections (the old daemon has
# released its listening socket) or timeout_secs elapses. Returns 0 once the
# port is free; returns 1 on timeout with a diagnostic — the caller MUST abort
# before bootstrap rather than spawn an instance that would yield exit(0).
wait_for_port_release() {
	local port="$1"
	local timeout_secs="$2"
	local label="$3"
	[[ -z "${port}" ]] && return 0
	log "ensuring port ${port} is free before bootstrapping ${label}"
	if ! port_is_listening "${port}"; then
		return 0
	fi
	log "port ${port} still held after bootout; waiting up to ${timeout_secs}s for release"
	local release_deadline=$(( $(date +%s) + timeout_secs ))
	while port_is_listening "${port}"; do
		if [[ $(date +%s) -ge ${release_deadline} ]]; then
			log "ERROR: port ${port} still occupied ${timeout_secs}s after bootout of ${label} —" >&2
			log "       bootstrapping now would spawn an instance that yields exit(0) on the busy" >&2
			log "       port (transport_http.rs:330), which KeepAlive SuccessfulExit=false never" >&2
			log "       respawns. Kill the stale listener (lsof -nP -iTCP:${port} -sTCP:LISTEN) and re-run." >&2
			return 1
		fi
		sleep 1
	done
	log "port ${port} released; proceeding to bootstrap ${label}"
	return 0
}

if [[ $DO_BUILD -eq 1 ]]; then
	# Apple Silicon dev-machine native CPU tuning. `-C target-cpu=native`
	# lets rustc pick the running host's ISA — covers M1..M5+ without
	# enumeration drift and removes the cross-chip SIGILL footgun. Skipped
	# when RUSTFLAGS is already set (allow override), in CI (CI env var),
	# or on non-arm64 macOS.
	if [[ "$(uname -sm)" == "Darwin arm64" ]] && [[ -z "${RUSTFLAGS:-}" ]] && [[ -z "${CI:-}" ]]; then
		export RUSTFLAGS="-C target-cpu=native"
		log "auto-applied RUSTFLAGS=\"${RUSTFLAGS}\""
	fi
	# Default: full language support (lang-extra ON, matches `cargo install` users).
	# Set CODELENS_LANG_MINIMAL=1 to drop the niche tree-sitter languages gated
	# by `lang-extra` (canonical list in crates/codelens-engine/Cargo.toml) and
	# shave ~2-3 MB off the binary. Only safe on hosts that don't index those
	# languages — this repo's `~/.codelens/index/symbols.db` had 0 files for
	# all of them at the time the flag was introduced.
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

if command -v xattr >/dev/null 2>&1; then
	log "stripping com.apple.provenance xattr (macOS gatekeeper)"
	xattr -dr com.apple.provenance "${TARGET_BIN}" 2>/dev/null || true
fi

if command -v codesign >/dev/null 2>&1; then
	log "ad-hoc resigning ${TARGET_BIN}"
	codesign --force --sign - "${TARGET_BIN}" || {
		log "WARNING: codesign failed; daemon may be killed by Gatekeeper" >&2
	}
fi

NEW_VERSION="$("${TARGET_BIN}" --version 2>/dev/null || true)"
if [[ -n "${NEW_VERSION}" ]]; then
	log "deployed: ${NEW_VERSION}"
else
	log "WARNING: ${TARGET_BIN} --version returned no output (codesign may still block)" >&2
fi

UID_VAL="$(id -u)"
readonly_label="${LABEL_PREFIX}-readonly"
mutation_label="${LABEL_PREFIX}-mutation"

if [[ $SKIP_READONLY -eq 1 ]]; then
	log "warning: --skip-readonly is deprecated; readonly is never started by the single-writer installer"
fi

# Disable and boot out the old readonly service before touching the canonical
# writer. This is deliberately unconditional so a stale plist cannot restart
# into ProjectWriterBusy after a redeploy.
if command -v launchctl >/dev/null 2>&1; then
	log "disabling legacy readonly label gui/${UID_VAL}/${readonly_label}"
	launchctl disable "gui/${UID_VAL}/${readonly_label}" >/dev/null 2>&1 || true
	launchctl bootout "gui/${UID_VAL}/${readonly_label}" >/dev/null 2>&1 || true
else
	log "launchctl unavailable; legacy readonly cleanup deferred: ${readonly_label}" >&2
fi

if [[ $SKIP_MUTATION -eq 1 ]]; then
	log "canonical mutation daemon skipped (--skip-mutation); legacy readonly remains disabled"
	exit 0
fi

KICK_LABELS=("${mutation_label}")

MISSING_LABELS=()
for label in "${KICK_LABELS[@]}"; do
	plist="${HOME}/Library/LaunchAgents/${label}.plist"
	if [[ -f "${plist}" ]]; then
		# launchd caches a lightweight code requirement (LWCR) for the target
		# executable. After replacing and re-signing the Mach-O, kickstart alone
		# can keep launching against the stale requirement and report
		# OS_REASON_CODESIGNING. Reload the LaunchAgent so the new signature is
		# accepted before the health probe.
		log "launchctl bootout/bootstrap gui/${UID_VAL}/${label}"
		launchctl bootout "gui/${UID_VAL}/${label}" 2>/dev/null || true
		# #356: bootout is asynchronous — launchd may still be tearing the
		# service down when bootstrap runs, failing with 'Bootstrap failed:
		# 5: Input/output error'. Wait for the label to disappear (max 5s),
		# then retry bootstrap with backoff before letting set -e abort.
		for _ in 1 2 3 4 5; do
			launchctl print "gui/${UID_VAL}/${label}" >/dev/null 2>&1 || break
			sleep 1
		done
		# 2026-07-10 incident: bootout is async at the socket layer too — the
		# launchd label can disappear while the old process still holds the
		# listening socket. Bootstrapping now spawns a fresh instance that meets
		# the occupied port and yields exit(0) (transport_http.rs:330); with
		# KeepAlive SuccessfulExit=false (PR #378) launchd never respawns it,
		# leaving a silent permanent-down. Block until the port is released.
		label_port="${MUTATION_PORT}"
		if ! wait_for_port_release "${label_port}" "${PORT_RELEASE_SECS}" "${label}"; then
			exit 4
		fi
		bootstrap_ok=0
		for attempt in 1 2 3; do
			if launchctl bootstrap "gui/${UID_VAL}" "${plist}" 2>/dev/null; then
				bootstrap_ok=1
				break
			fi
			log "bootstrap attempt ${attempt} failed for ${label}; retrying in ${attempt}s"
			sleep "${attempt}"
		done
		if [[ ${bootstrap_ok} -eq 0 ]]; then
			# Final attempt without stderr suppression so the real launchd
			# error surfaces if the retries could not recover.
			launchctl bootstrap "gui/${UID_VAL}" "${plist}"
		fi
		launchctl enable "gui/${UID_VAL}/${label}" || true
		log "launchctl kickstart -k gui/${UID_VAL}/${label}"
		launchctl kickstart -k "gui/${UID_VAL}/${label}"
	else
		log "WARNING: plist not found for ${label} (${plist}); skipping bootstrap+kickstart. Run scripts/install-http-daemons-launchd.sh first." >&2
		MISSING_LABELS+=("${label}")
	fi
done

log "waiting up to ${WAIT_SECS}s for the canonical LISTEN socket"
EXPECTED_PORTS=("${MUTATION_PORT}")

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

if [[ ${#MISSING_LABELS[@]} -gt 0 ]]; then
	log "ERROR: no LaunchAgent plist for requested label(s): ${MISSING_LABELS[*]}" >&2
	log "  Nothing was bootstrapped/kickstarted for them; any LISTEN success above is from" >&2
	log "  pre-existing daemons, not this redeploy (e.g. a mistyped --label-prefix)." >&2
	log "  Run scripts/install-http-daemons-launchd.sh first, or fix --label-prefix, then re-run." >&2
	exit 1
fi

log "done"
