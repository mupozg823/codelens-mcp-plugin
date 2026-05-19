#!/usr/bin/env bash
set -euo pipefail

# Compare the local launchd daemon binary's git SHA against the source HEAD
# and exit non-zero if they drifted. Read-only — no build, no kickstart.
#
# Why: this repo's HTTP daemons (dev.codelens.mcp-readonly/mutation) run a
# binary copied into `.codelens/bin/codelens-mcp-http`. A `cargo build
# --release` does NOT update that binary; the daemons keep running the
# previous build until `scripts/redeploy-daemons.sh` re-copies it. That
# silent staleness produces "Unknown tool" responses and dispatch
# inconsistencies that look like server-side bugs but are actually a
# stale binary.
#
# Usage:
#   bash scripts/daemon-stale-check.sh
#   bash scripts/daemon-stale-check.sh /custom/path/to/codelens-mcp-http
#
# Exit codes:
#   0 — daemon binary matches source HEAD (in sync)
#   1 — daemon binary lags source HEAD (stale, redeploy required)
#   2 — daemon binary not found
#   3 — daemon binary version string unparseable

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-${REPO_ROOT}/.codelens/bin/codelens-mcp-http}"

if [[ ! -x "${TARGET}" ]]; then
	echo "[daemon-stale-check] binary not found: ${TARGET}" >&2
	echo "  run: bash scripts/redeploy-daemons.sh --build --probe" >&2
	exit 2
fi

DAEMON_VER="$("${TARGET}" --version 2>/dev/null || true)"
if [[ -z "${DAEMON_VER}" ]]; then
	echo "[daemon-stale-check] ${TARGET} --version returned empty (codesign?)" >&2
	exit 3
fi

DAEMON_SHA="$(printf '%s' "${DAEMON_VER}" | grep -oE 'git [a-f0-9]+' | awk '{print $2}' || true)"
SOURCE_SHA="$(git -C "${REPO_ROOT}" rev-parse --short=7 HEAD 2>/dev/null || true)"

if [[ -z "${DAEMON_SHA}" ]]; then
	echo "[daemon-stale-check] cannot parse git sha from: ${DAEMON_VER}" >&2
	exit 3
fi
if [[ -z "${SOURCE_SHA}" ]]; then
	echo "[daemon-stale-check] not a git repo: ${REPO_ROOT}" >&2
	exit 3
fi

# Length-tolerant prefix match — `--version` truncation length varies by build
# (typically 7 chars, occasionally 8). Compare on the shorter of the two.
LEN_D=${#DAEMON_SHA}
LEN_S=${#SOURCE_SHA}
PREFIX_LEN=$(( LEN_D < LEN_S ? LEN_D : LEN_S ))
DAEMON_PREFIX="${DAEMON_SHA:0:${PREFIX_LEN}}"
SOURCE_PREFIX="${SOURCE_SHA:0:${PREFIX_LEN}}"

if [[ "${DAEMON_PREFIX}" == "${SOURCE_PREFIX}" ]]; then
	echo "[daemon-stale-check] in sync: daemon=${DAEMON_SHA}, source HEAD=${SOURCE_SHA}"
	exit 0
fi

echo "[daemon-stale-check] STALE: daemon=${DAEMON_SHA}, source HEAD=${SOURCE_SHA}" >&2
echo "  run: bash scripts/redeploy-daemons.sh --build --probe" >&2
exit 1
