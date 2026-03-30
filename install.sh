#!/usr/bin/env bash
# CodeLens MCP — Universal Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash
#
# Installs the binary and auto-configures detected AI coding agents:
#   Claude Code, Cursor, VS Code, Codex, Windsurf, Cline
set -euo pipefail

REPO="mupozg823/codelens-mcp-plugin"
INSTALL_DIR="${CODELENS_INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="codelens-mcp"

# ── Detect platform ──────────────────────────────────────────────────
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
linux) OS_NAME="linux" ;;
darwin) OS_NAME="darwin" ;;
*)
	echo "Unsupported OS: $OS" >&2
	exit 1
	;;
esac

case "$ARCH" in
x86_64 | amd64) ARCH_NAME="x86_64" ;;
aarch64 | arm64) ARCH_NAME="arm64" ;;
*)
	echo "Unsupported architecture: $ARCH" >&2
	exit 1
	;;
esac

PLATFORM="${OS_NAME}-${ARCH_NAME}"
echo "==> Installing codelens-mcp for ${PLATFORM}..."

# ── Install binary ───────────────────────────────────────────────────
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null |
	grep "browser_download_url.*${PLATFORM}" | head -1 | cut -d'"' -f4 || true)

if [ -n "$LATEST" ]; then
	mkdir -p "$INSTALL_DIR"
	echo "    Downloading from release..."
	curl -fsSL "$LATEST" | tar xz -C "$INSTALL_DIR"
	chmod +x "${INSTALL_DIR}/${BIN_NAME}"
else
	echo "    No pre-built binary found. Building from source..."
	if ! command -v cargo &>/dev/null; then
		echo "    ERROR: cargo not found. Install Rust first: https://rustup.rs" >&2
		exit 1
	fi
	TMP_DIR=$(mktemp -d)
	git clone --depth 1 "https://github.com/${REPO}.git" "$TMP_DIR" 2>/dev/null
	(cd "$TMP_DIR" && cargo build --release)
	mkdir -p "$INSTALL_DIR"
	cp "$TMP_DIR/target/release/${BIN_NAME}" "$INSTALL_DIR/"
	rm -rf "$TMP_DIR"
fi

BIN_PATH="${INSTALL_DIR}/${BIN_NAME}"
echo "    Installed: ${BIN_PATH}"
echo ""

# ── MCP config writer ───────────────────────────────────────────────
write_mcp_json() {
	local file="$1" fmt="$2"
	mkdir -p "$(dirname "$file")"
	case "$fmt" in
	claude)
		cat >"$file" <<EOF
{
  "mcpServers": {
    "codelens": {
      "type": "stdio",
      "command": "${BIN_PATH}",
      "args": ["."]
    }
  }
}
EOF
		;;
	cursor)
		cat >"$file" <<EOF
{
  "mcpServers": {
    "codelens": {
      "command": "${BIN_PATH}",
      "args": [".", "--preset", "balanced"]
    }
  }
}
EOF
		;;
	vscode)
		cat >"$file" <<EOF
{
  "servers": {
    "codelens": {
      "type": "stdio",
      "command": "${BIN_PATH}",
      "args": [".", "--preset", "balanced"]
    }
  }
}
EOF
		;;
	generic)
		cat >"$file" <<EOF
{
  "codelens": {
    "command": "${BIN_PATH}",
    "args": [".", "--preset", "balanced"],
    "transport": "stdio"
  }
}
EOF
		;;
	esac
}

# ── Auto-configure detected agents ──────────────────────────────────
echo "==> Detecting AI coding agents..."
CONFIGURED=""

# Claude Code
if [ -d "$HOME/.claude" ] || command -v claude &>/dev/null; then
	write_mcp_json "$HOME/.claude.json" "claude"
	CONFIGURED="${CONFIGURED}  ✓ Claude Code\n"
fi

# Cursor
if [ -d "$HOME/.cursor" ] || [ -d "${HOME}/Library/Application Support/Cursor" ] 2>/dev/null; then
	write_mcp_json "$HOME/.cursor/mcp.json" "cursor"
	CONFIGURED="${CONFIGURED}  ✓ Cursor\n"
fi

# VS Code
for vsc_dir in "$HOME/.vscode" "$HOME/Library/Application Support/Code/User" "$HOME/.config/Code/User"; do
	if [ -d "$vsc_dir" ]; then
		write_mcp_json "${vsc_dir}/mcp.json" "vscode"
		CONFIGURED="${CONFIGURED}  ✓ VS Code\n"
		break
	fi
done

# Windsurf
for ws_dir in "$HOME/.windsurf" "$HOME/.codeium/windsurf"; do
	if [ -d "$ws_dir" ]; then
		write_mcp_json "${ws_dir}/mcp_servers.json" "generic"
		CONFIGURED="${CONFIGURED}  ✓ Windsurf\n"
		break
	fi
done

if [ -n "$CONFIGURED" ]; then
	echo -e "$CONFIGURED"
else
	echo "  (no agents detected — configure manually)"
	echo "  See: https://github.com/${REPO}/blob/main/docs/platform-setup.md"
fi

# ── PATH check ───────────────────────────────────────────────────────
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
	echo "==> Add to PATH:"
	echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
	echo ""
fi

echo "==> Done! Verify: ${BIN_NAME} . --cmd get_capabilities --args '{}'"
