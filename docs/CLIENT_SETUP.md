# CodeLens Client Setup Guide

## Prerequisites

Build one of:

```bash
# Option A: Rust binary (fastest)
cd rust && cargo build --release

# Option B: Kotlin standalone jar
./gradlew :standalone:standaloneFatJar
```

---

## Claude Code

### Using launcher script

Add to `~/.claude.json` or project `.mcp.json`:

```json
{
  "mcpServers": {
    "codelens-standalone": {
      "command": "/path/to/codelens-mcp-plugin/scripts/codelens",
      "args": ["."]
    }
  }
}
```

### Using Rust binary directly

```json
{
  "mcpServers": {
    "codelens-standalone": {
      "command": "/path/to/codelens-mcp-plugin/rust/target/release/codelens-mcp",
      "args": ["."]
    }
  }
}
```

### Using Kotlin fat-jar

```json
{
  "mcpServers": {
    "codelens-standalone": {
      "command": "java",
      "args": [
        "-jar",
        "/path/to/standalone-1.0.0-standalone.jar",
        ".",
        "--stdio"
      ]
    }
  }
}
```

---

## Codex (OpenAI)

Add to `~/.codex/config.toml`:

```toml
[mcp_servers.codelens]
command = "/path/to/codelens-mcp-plugin/scripts/codelens"
args = ["."]
```

---

## Cursor

1. Open Cursor Settings
2. Go to **MCP Servers**
3. Add server:
   - **Name:** codelens
   - **Command:** `/path/to/codelens-mcp-plugin/scripts/codelens`
   - **Args:** `.`

---

## Cline (VS Code)

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "codelens": {
      "command": "/path/to/codelens-mcp-plugin/scripts/codelens",
      "args": ["."]
    }
  }
}
```

---

## IntelliJ IDEA (Plugin)

No MCP client config needed. The plugin auto-registers on port 24226.

1. Build: `./gradlew buildPlugin`
2. Install: **Settings -> Plugins -> Install from Disk**
3. MCP endpoint: `http://127.0.0.1:24226/mcp`

---

## Verification

After setup, test with any MCP client:

```
# Should return server info and tool list
tools/list
```

Or use the launcher script directly:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize"}' | ./scripts/codelens /path/to/project
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | ./scripts/codelens /path/to/project
```

---

## Troubleshooting

| Issue                                 | Solution                                                                           |
| ------------------------------------- | ---------------------------------------------------------------------------------- |
| "No CodeLens binary found"            | Run `cd rust && cargo build --release` or `./gradlew :standalone:standaloneFatJar` |
| Tools return errors about GoogleDrive | Your project root is too broad (e.g. home directory). Set a specific project path. |
| Import graph tools return null        | The file language may not be supported. Check the 14 supported languages.          |
| Slow first response                   | First call triggers symbol index build. Subsequent calls use SQLite cache.         |
