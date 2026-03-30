# CodeLens MCP — Platform Setup Guide

> One binary, every AI coding agent.

## Quick Install

```bash
# Option 1: Cargo (recommended)
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp

# Option 2: One-line installer (downloads pre-built binary)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# Option 3: Build from source
git clone https://github.com/mupozg823/codelens-mcp-plugin
cd codelens-mcp-plugin && cargo build --release
cp target/release/codelens-mcp ~/.local/bin/
```

Verify: `codelens-mcp . --cmd get_capabilities --args '{}'`

---

## Platform Configurations

### 1. Claude Code (CLI / Desktop / Web)

**Global config** (`~/.claude.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "type": "stdio",
      "command": "codelens-mcp",
      "args": ["."]
    }
  }
}
```

**Per-project** (`.mcp.json` in project root):

```json
{
  "mcpServers": {
    "codelens": {
      "type": "stdio",
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

**Presets:**

- `minimal` — 21 tools, fastest, read-only exploration + safe edits
- `balanced` — 39 tools, default, excludes niche analysis + Claude built-in overlaps
- `full` — 62 tools, everything including advanced analysis

---

### 2. Cursor

**Project config** (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

**Global config** (`~/.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "full"]
    }
  }
}
```

---

### 3. Codex (OpenAI)

**`codex.json` or CLI flag:**

```json
{
  "mcpServers": {
    "codelens": {
      "type": "stdio",
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

**Or via CLI:**

```bash
codex --mcp-server "codelens-mcp . --preset balanced"
```

---

### 4. VS Code (Copilot / Continue / Cline)

**`.vscode/mcp.json`:**

```json
{
  "servers": {
    "codelens": {
      "type": "stdio",
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

---

### 5. JetBrains (IntelliJ / WebStorm / PyCharm)

**Settings → Tools → MCP Servers → Add:**

- Name: `codelens`
- Command: `codelens-mcp`
- Arguments: `. --preset balanced`
- Transport: stdio

---

### 6. Windsurf / Cline / Aider

**`mcp_servers.json` or equivalent:**

```json
{
  "codelens": {
    "command": "codelens-mcp",
    "args": [".", "--preset", "balanced"],
    "transport": "stdio"
  }
}
```

---

### 7. HTTP Transport (Remote / Multi-client)

For remote deployment or multi-agent scenarios:

```bash
# Start HTTP server
codelens-mcp /path/to/project --transport http --port 7837

# Client connects to:
#   POST http://localhost:7837/mcp          (JSON-RPC)
#   GET  http://localhost:7837/mcp          (SSE stream)
#   GET  http://localhost:7837/.well-known/mcp.json  (Server Card)
```

**Docker:**

```dockerfile
FROM rust:1.83-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev
COPY . /app
WORKDIR /app
RUN cargo build --release --features http

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/codelens-mcp /usr/local/bin/
EXPOSE 7837
ENTRYPOINT ["codelens-mcp", "/workspace", "--transport", "http", "--port", "7837"]
```

---

### 8. Claude Agent SDK / Custom Agents

```python
import anthropic
from anthropic.types import MCPServerStdio

client = anthropic.Anthropic()

# Use CodeLens as MCP tool server
agent = client.agents.create(
    model="claude-sonnet-4-20250514",
    mcp_servers=[
        MCPServerStdio(
            command="codelens-mcp",
            args=[".", "--preset", "balanced"],
        )
    ],
)
```

---

## Environment Variables

| Variable             | Default    | Description                                     |
| -------------------- | ---------- | ----------------------------------------------- |
| `CODELENS_LOG`       | `warn`     | Log level (trace/debug/info/warn/error)         |
| `CODELENS_PRESET`    | `balanced` | Default preset (overridden by --preset)         |
| `CLAUDE_PROJECT_DIR` | —          | Auto-detected project root (set by Claude Code) |
| `MCP_PROJECT_DIR`    | —          | Generic project root override                   |

---

## Preset Comparison

```
Feature                    MINIMAL(21)  BALANCED(39)  FULL(62)
─────────────────────────  ──────────   ──────────    ────────
Symbol lookup (find/get)   ✓            ✓             ✓
Code editing (rename/etc)  ✓            ✓             ✓
File I/O (read/list/find)  ✓            ✗¹            ✓
LSP diagnostics            ✓            ✓             ✓
Impact analysis            ✗            ✓             ✓
Dead code / cycles         ✗            ✗             ✓
PageRank importance        ✗            ✗             ✓
Semantic search            ✗            ✓²            ✓²
Multi-project queries      ✗            ✓             ✓
Unified insert/replace     ✓            ✓             ✓
─────────────────────────
¹ Claude Code has built-in Read/Glob/Grep
² Requires `semantic` feature (default enabled)
```

---

## Verify Installation

```bash
# Check binary
codelens-mcp --help 2>&1 || codelens-mcp . --cmd get_capabilities --args '{}'

# Check tool count per preset
codelens-mcp . --cmd set_preset --args '{"preset":"full"}'
codelens-mcp . --cmd set_preset --args '{"preset":"balanced"}'
codelens-mcp . --cmd set_preset --args '{"preset":"minimal"}'

# Test symbol extraction
codelens-mcp /path/to/project --cmd find_symbol --args '{"name":"main","include_body":true}'
```
