#!/usr/bin/env bash
# One-line installer for CodeLens MCP
# Usage: curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash
set -euo pipefail

REPO="mupozg823/codelens-mcp-plugin"
INSTALL_DIR="${CODELENS_INSTALL_DIR:-$HOME/.local/bin}"

# Detect platform
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
echo "Installing codelens-mcp for ${PLATFORM}..."

# Get latest release URL
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep "browser_download_url.*${PLATFORM}" | head -1 | cut -d'"' -f4)

if [ -z "$LATEST" ]; then
	echo "No release found for ${PLATFORM}. Building from source..." >&2
	echo "  git clone https://github.com/${REPO} && cd codelens-mcp-plugin && cargo build --release" >&2
	exit 1
fi

# Download and install
mkdir -p "$INSTALL_DIR"
echo "Downloading ${LATEST}..."
curl -fsSL "$LATEST" | tar xz -C "$INSTALL_DIR"
chmod +x "${INSTALL_DIR}/codelens-mcp"

echo ""
echo "Installed: ${INSTALL_DIR}/codelens-mcp"
echo ""

# Auto-configure Claude Code if present
if [ -d "$HOME/.claude" ]; then
	python3 -c "
import json, os
path = os.path.expanduser('~/.claude.json')
data = json.load(open(path)) if os.path.exists(path) else {}
data.setdefault('mcpServers', {})['codelens'] = {
    'type': 'stdio',
    'command': '${INSTALL_DIR}/codelens-mcp',
    'args': ['.']
}
json.dump(data, open(path, 'w'), indent=2)
" 2>/dev/null && echo "Claude Code configured automatically." || true
fi

echo "Add to PATH if needed: export PATH=\"${INSTALL_DIR}:\$PATH\""
echo "Usage: codelens-mcp /path/to/project"
