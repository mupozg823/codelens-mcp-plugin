#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/sync-local-bin.sh [repo-root] [--debug|--release] [--no-build]

Builds codelens-mcp from the current checkout (release by default) and
re-links ~/.local/bin/codelens-mcp to that build so PATH resolves to the
latest local checkout instead of a stale cargo-installed binary.

Examples:
  bash scripts/sync-local-bin.sh .
  bash scripts/sync-local-bin.sh . --debug
  CODELENS_INSTALL_DIR="$HOME/.cargo/bin" bash scripts/sync-local-bin.sh . --no-build
EOF
}

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT=""
PROFILE="release"
NO_BUILD=0

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--debug)
		PROFILE="debug"
		shift
		;;
	--release)
		PROFILE="release"
		shift
		;;
	--no-build)
		NO_BUILD=1
		shift
		;;
	-*)
		echo "unknown option: $1" >&2
		usage >&2
		exit 1
		;;
	*)
		if [[ -n "$REPO_ROOT" ]]; then
			echo "multiple repo roots provided" >&2
			usage >&2
			exit 1
		fi
		REPO_ROOT="$1"
		shift
		;;
	esac
done

if [[ -z "$REPO_ROOT" ]]; then
	REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
else
	REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd)"
fi

if [[ ! -f "$REPO_ROOT/Cargo.toml" || ! -f "$REPO_ROOT/crates/codelens-mcp/Cargo.toml" ]]; then
	echo "repo root does not look like codelens-mcp-plugin: $REPO_ROOT" >&2
	exit 1
fi

INSTALL_DIR="${CODELENS_INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="codelens-mcp"
TARGET_BIN="$REPO_ROOT/target/$PROFILE/$BIN_NAME"
BUILD_ARGS=(-p codelens-mcp)

if [[ "$PROFILE" == "release" ]]; then
	BUILD_ARGS+=(--release)
fi

if [[ "$NO_BUILD" == "0" ]]; then
	echo "==> Building $BIN_NAME ($PROFILE) from $REPO_ROOT"
	(
		cd "$REPO_ROOT"
		cargo build "${BUILD_ARGS[@]}"
	)
fi

if [[ ! -x "$TARGET_BIN" ]]; then
	echo "expected built binary not found: $TARGET_BIN" >&2
	exit 1
fi

mkdir -p "$INSTALL_DIR"
LINK_PATH="$INSTALL_DIR/$BIN_NAME"
ln -sfn "$TARGET_BIN" "$LINK_PATH"

echo "==> Linked $LINK_PATH -> $TARGET_BIN"
if command -v "$BIN_NAME" >/dev/null 2>&1; then
	RESOLVED="$(command -v "$BIN_NAME")"
	echo "==> PATH resolves $BIN_NAME to: $RESOLVED"
fi

if (
	cd "$REPO_ROOT"
	"$LINK_PATH" status --json claude-code >/dev/null 2>&1
); then
	echo "==> Verified: \`$BIN_NAME status --json claude-code\` works from the synced binary"
else
	echo "==> Warning: synced binary did not pass \`$BIN_NAME status --json claude-code\` smoke check" >&2
fi
