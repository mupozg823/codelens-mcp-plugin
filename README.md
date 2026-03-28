# CodeLens MCP

**Pure Rust MCP server for code intelligence.** 49 tools, 16 languages, file watcher, instant startup.

Works with Claude Code, Cursor, Windsurf, Cline, Codex, and any MCP-compatible client.

## Install

```bash
# Homebrew (macOS/Linux)
brew tap mupozg823/codelens
brew install codelens-mcp

# Cargo
cargo install codelens-mcp

# From source
cargo build --release
```

## Configure

**Claude Code** (`~/.claude.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": ["."]
    }
  }
}
```

### Presets

```bash
codelens-mcp .                        # balanced (default, 41 tools)
codelens-mcp . --preset full          # all 49 tools
codelens-mcp . --preset minimal       # 18 tools (for subagents)
CODELENS_PRESET=minimal codelens-mcp . # via env var
```

## Features

### 49 Tools

| Category   | Tools | Highlights                                       |
| ---------- | ----- | ------------------------------------------------ |
| Filesystem | 7     | read, list, search, find files/annotations/tests |
| Symbols    | 6     | tree-sitter overview, find, ranked context       |
| LSP        | 6     | references, diagnostics, type hierarchy, rename  |
| Graph      | 7     | blast radius, dead code, callers, circular deps  |
| Mutation   | 11    | rename, replace, insert, auto-import             |
| Memory     | 6     | Serena-compatible CRUD                           |
| Session    | 5     | activate, watch status, config                   |
| Composite  | 1     | extract function refactoring                     |

### 16 Languages

Python, JavaScript, TypeScript, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart

### File Watcher

Automatic re-indexing on file changes (300ms debounce) with GraphCache invalidation.

### Performance

| Metric         | Value |
| -------------- | ----- |
| Cold start     | ~12ms |
| Symbol search  | 131ms |
| Pattern search | 6ms   |
| Binary size    | ~32MB |
| Memory (idle)  | ~10MB |

## Tech Stack

- **tree-sitter**: 16 languages, native bindings
- **SQLite**: WAL-mode incremental index (rusqlite)
- **notify**: file watcher with debounce
- **Rayon**: parallel parsing
- **MCP**: stdio JSON-RPC 2.0 + Tool Annotations

## License

Apache-2.0
