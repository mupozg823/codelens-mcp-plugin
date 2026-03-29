# CodeLens MCP

Pure Rust MCP server for code intelligence — 50 tools, 16 languages, 12ms startup.

![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

<!-- Demo GIF can be placed here -->

## Why CodeLens?

- **Free and open-source.** No subscriptions, no seat licenses. Comparable commercial tools (e.g. jCodeMunch) start at $79+/month.
- **Single binary, instant startup.** Pure Rust, 32MB, ~12ms cold start. No Node runtime, no JVM, no Python interpreter.
- **50 tools in one binary.** Tree-sitter symbol indexing, LSP integration, import graph with PageRank, dead code detection, and semantic vector search — all without external services.
- **16 languages, runtime preset switching.** Switch from 21-tool minimal mode to 50-tool full mode at runtime with `set_preset` — no server restart.

## Quick Comparison

| Feature              | CodeLens        | jCodeMunch        | mcp-language-server |
| -------------------- | --------------- | ----------------- | ------------------- |
| Price                | Free / OSS      | $79+/month        | Free / OSS          |
| Languages            | 16              | 10                | varies by LSP       |
| MCP tools            | 50              | ~20               | ~10                 |
| LSP integration      | yes             | yes               | yes (only)          |
| Import graph         | yes             | no                | no                  |
| Circular dep. detect | yes             | no                | no                  |
| Semantic search      | yes (fastembed) | yes (cloud)       | no                  |
| Runtime preset       | yes             | no                | no                  |
| Binary size          | ~32MB           | N/A (SaaS)        | ~5MB                |
| Cold start           | ~12ms           | network dependent | ~200ms              |

## 5-Minute Quickstart

### Install

**Cargo:**

```bash
cargo install codelens-mcp
```

**Homebrew (macOS / Linux):**

```bash
brew tap mupozg823/codelens
brew install codelens-mcp
```

**GitHub Releases:** Download the pre-built binary for your platform from the [Releases page](https://github.com/mupozg823/codelens-mcp-plugin/releases), extract, and place it on your `$PATH`.

### Configure

Add to your `.mcp.json` (or `~/.claude.json` for Claude Code):

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

### First command

Start the server and ask your AI client:

```
What symbols are defined in src/main.rs?
```

The server indexes your project automatically on first activation.

## MCP Client Configurations

### Claude Code

```json
// .mcp.json (project-level) or ~/.claude/.mcp.json (global)
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### Cursor

```json
// .cursor/mcp.json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": ["."]
    }
  }
}
```

### Windsurf

```json
// ~/.codeium/windsurf/mcp_config.json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": ["."]
    }
  }
}
```

### Cline (VS Code)

```json
// VS Code Settings → Cline MCP Servers
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": ["."]
    }
  }
}
```

### Continue (VS Code/JetBrains)

```json
// ~/.continue/config.json → mcpServers section
{
  "mcpServers": [
    {
      "name": "codelens",
      "command": "codelens-mcp",
      "args": ["."]
    }
  ]
}
```

All clients use stdio transport. Set `--preset minimal` for lower token usage.

## Tool Categories

| Category   | Count | Highlights                                                  |
| ---------- | ----- | ----------------------------------------------------------- |
| Filesystem | 7     | read_file, list_dir, find_file, search_for_pattern          |
| Symbols    | 6     | get_symbols_overview, find_symbol, get_ranked_context       |
| LSP        | 6     | find_referencing_symbols, get_file_diagnostics, type hier.  |
| Graph      | 7     | get_impact_analysis, find_dead_code, callers, circular deps |
| Mutation   | 11    | rename_symbol, replace_symbol_body, add_import              |
| Memory     | 6     | list/read/write/delete/edit/rename_memory                   |
| Session    | 6     | activate_project, get_watch_status, set_preset              |
| Composite  | 1     | refactor_extract_function                                   |

2 tools migrated to Skills: `onboarding` → `/onboard-project`, `get_lsp_recipe` → `/lsp-setup`.

## Presets

| Preset   | Tools | Tokens | Use case                           |
| -------- | ----- | ------ | ---------------------------------- |
| FULL     | 50    | ~5K    | All tools, maximum capability      |
| BALANCED | 42    | ~4K    | Default — excludes 8 niche tools   |
| MINIMAL  | 21    | ~2K    | Subagents, token-constrained tasks |

Switch via CLI flag, environment variable, or at runtime:

```bash
codelens-mcp . --preset full
CODELENS_PRESET=minimal codelens-mcp .
# or at runtime (no restart needed):
# call set_preset tool with "minimal" | "balanced" | "full"
```

## Feature Flags

| Feature    | Build flag                      | Binary delta | Notes                               |
| ---------- | ------------------------------- | ------------ | ----------------------------------- |
| `semantic` | `--features semantic` (default) | +18MB        | fastembed + sqlite-vec embeddings   |
| `http`     | `--features http`               | +18MB        | axum HTTP transport for agent teams |

## Languages

Python, JavaScript, TypeScript, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart (16 languages)

> All 16 languages use native tree-sitter bindings for fast, accurate symbol parsing.

## Building from Source

```bash
# Standard build (semantic search included by default)
cargo build --release

# Minimal build — no semantic search, no HTTP
cargo build --release --no-default-features

# All features
cargo build --release --features semantic,http

# Run tests
cargo test -- --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan
```

The binary is written to `target/release/codelens-mcp`.

## License

Apache-2.0 — see [LICENSE](LICENSE).
