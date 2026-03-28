#!/usr/bin/env bash
set -euo pipefail

REPO="mupozg823/codelens-mcp-plugin"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and arch
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
darwin) PLATFORM="darwin" ;;
linux) PLATFORM="linux" ;;
*)
	echo "Unsupported OS: $OS" >&2
	exit 1
	;;
esac

case "$ARCH" in
arm64 | aarch64) ARCH_NAME="arm64" ;;
x86_64 | amd64) ARCH_NAME="x86_64" ;;
*)
	echo "Unsupported architecture: $ARCH" >&2
	exit 1
	;;
esac

ASSET="codelens-mcp-${PLATFORM}-${ARCH_NAME}.tar.gz"

# Get latest release tag
echo "Fetching latest release..."
TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$TAG" ]; then
	echo "Error: Could not find latest release" >&2
	exit 1
fi

URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"

echo "Downloading $ASSET ($TAG)..."
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "$TMPDIR/$ASSET"
tar xzf "$TMPDIR/$ASSET" -C "$TMPDIR"

mkdir -p "$INSTALL_DIR"
mv "$TMPDIR/codelens-mcp" "$INSTALL_DIR/codelens-mcp"
chmod +x "$INSTALL_DIR/codelens-mcp"

echo ""
echo "Installed codelens-mcp to $INSTALL_DIR/codelens-mcp"

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -q "^$INSTALL_DIR$"; then
	echo ""
	echo "Add to your PATH:"
	echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

# Auto-configure Claude Code if detected
CLAUDE_MCP="$HOME/.claude/mcp.json"
if [ -d "$HOME/.claude" ]; then
	echo ""
	echo "Detected Claude Code. Configuring MCP server..."
	cat >"$CLAUDE_MCP" <<HEREDOC
{
  "mcpServers": {
    "codelens": {
      "command": "$INSTALL_DIR/codelens-mcp",
      "args": ["."]
    }
  }
}
HEREDOC
	echo "  -> $CLAUDE_MCP configured"
	echo "  Restart Claude Code to activate."
fi

echo ""
echo "Done. Usage: codelens-mcp /path/to/project"
