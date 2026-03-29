# CodeLens MCP

Pure Rust MCP server for code intelligence — 56 tools, 15 languages, 12ms startup, 251 tests.

![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

<!-- Demo GIF can be placed here -->

## Why CodeLens?

- **Free and open-source.** No subscriptions, no seat licenses. Comparable commercial tools (e.g. jCodeMunch) start at $79+/month.
- **Single binary, instant startup.** Pure Rust, 32MB, ~12ms cold start. No Node runtime, no JVM, no Python interpreter.
- **56 tools in one binary.** Tree-sitter symbol indexing, LSP integration, import graph with PageRank, dead code detection, and semantic vector search (BGE-small quantized) — all without external services.
- **15 languages, runtime preset switching.** Switch from 21-tool minimal mode to 56-tool full mode at runtime with `set_preset` — no server restart.
- **53-83x faster than grep** for symbol lookup and reference tracing. SQLite FTS5 index eliminates full-project scanning.
- **Context-aware workflow guidance.** Dynamic tool suggestions adapt to your task — mutation chains auto-suggest diagnostics, exploration chains suggest deeper context tools.

## Quick Comparison

| Feature              | CodeLens          | jCodeMunch        | mcp-language-server |
| -------------------- | ----------------- | ----------------- | ------------------- |
| Price                | Free / OSS        | $79+/month        | Free / OSS          |
| Languages            | 15                | 10                | varies by LSP       |
| MCP tools            | 56                | ~20               | ~10                 |
| LSP integration      | yes               | yes               | yes (only)          |
| Import graph         | yes               | no                | no                  |
| Circular dep. detect | yes               | no                | no                  |
| Semantic search      | yes (BGE-Q local) | yes (cloud)       | no                  |
| Runtime preset       | yes               | no                | no                  |
| Binary size          | ~32MB             | N/A (SaaS)        | ~5MB                |
| Cold start           | ~12ms             | network dependent | ~200ms              |

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

All clients use **stdio transport**. Ensure `codelens-mcp` is on your `$PATH` (via `cargo install`, Homebrew, or manual install).

### Claude Code

```json
// .mcp.json (project-level) or ~/.claude.json (global)
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"],
      "type": "stdio"
    }
  }
}
```

### Cursor

```json
// .cursor/mcp.json (project-level) or ~/.cursor/mcp.json (global)
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### OpenAI Codex

```json
// .codex/mcp.json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### Windsurf (Codeium)

```json
// ~/.codeium/windsurf/mcp_config.json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### Claude Desktop

```json
// macOS: ~/Library/Application Support/Claude/claude_desktop_config.json
// Windows: %APPDATA%/Claude/claude_desktop_config.json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### Cline (VS Code)

```json
// VS Code Settings → Cline → MCP Servers → Edit JSON
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### Continue (VS Code / JetBrains)

```json
// ~/.continue/config.json → mcpServers section (array format)
{
  "mcpServers": [
    {
      "name": "codelens",
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  ]
}
```

### Zed

```json
// ~/.config/zed/settings.json → context_servers section
{
  "context_servers": {
    "codelens": {
      "command": {
        "path": "codelens-mcp",
        "args": [".", "--preset", "balanced"]
      }
    }
  }
}
```

> Tip: Use `--preset minimal` (21 tools) for token-constrained environments or subagents.

## Performance

Benchmarked on a 27K LOC Rust project (this repository):

| Operation               | CodeLens | grep    | Speedup                             |
| ----------------------- | -------- | ------- | ----------------------------------- |
| Find function by name   | 24ms     | 2,002ms | **83x**                             |
| Trace all references    | 23ms     | 1,225ms | **53x**                             |
| File structure overview | 24ms     | 17ms    | ~1x (grep wins on trivial patterns) |
| Impact analysis         | 24ms     | N/A     | (no grep equivalent)                |
| Cold start + config     | 28ms     | —       | —                                   |

CodeLens uses SQLite FTS5 indexing, so lookups are O(log n) regardless of project size.

Run benchmarks yourself:

```bash
cargo build --release
./benchmarks/bench.sh . ./target/release/codelens-mcp
```

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  MCP Protocol (stdio / HTTP+SSE)                         │
├──────────────────────────────────────────────────────────┤
│  Dispatch — static table + context-aware suggestions     │
│  Session telemetry — per-tool + session-level metrics    │
│  Token budget — auto-preset by project size              │
├──────────────────────────────────────────────────────────┤
│  codelens-core                                           │
│  ├─ tree-sitter (15 langs)  ├─ SQLite FTS5 index        │
│  ├─ import graph + PageRank ├─ call graph                │
│  ├─ LSP session pool        ├─ file watcher              │
│  └─ fastembed (BGE-small)   └─ sqlite-vec (vectors)     │
└──────────────────────────────────────────────────────────┘
```

- **2-crate workspace**: `codelens-core` (pure logic) + `codelens-mcp` (protocol layer)
- **Reader/Writer split**: `Mutex<IndexDb>` for writes, per-query read-only connections
- **4-signal ranking**: text relevance + PageRank + recency + semantic similarity
- **Auto-weight tuning**: identifier queries → text-heavy, natural language → semantic-heavy

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

| Preset   | Tools | Budget | Use case                                       |
| -------- | ----- | ------ | ---------------------------------------------- |
| FULL     | 56    | 8K     | All tools, maximum capability                  |
| BALANCED | 38    | 4K     | Default — no built-in overlaps, no niche tools |
| MINIMAL  | 21    | 2K     | Subagents, token-constrained tasks             |

Auto-preset: `activate_project` automatically selects the preset based on project size (<50 files → Minimal, 50-500 → Balanced, >500 → Full).

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

Python, JavaScript, TypeScript, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart

> All 15 languages use native tree-sitter bindings for fast, accurate symbol parsing.

## Building from Source

```bash
# Standard build (semantic search included by default)
cargo build --release

# Minimal build — no semantic search, no HTTP
cargo build --release --no-default-features

# All features
cargo build --release --features semantic,http

# Run tests (251 total)
cargo test -p codelens-core && cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http  # HTTP/SSE transport tests

# Run benchmarks
./benchmarks/bench.sh . ./target/release/codelens-mcp
```

The binary is written to `target/release/codelens-mcp`.

## Contributing

Contributions welcome! Please run the full test suite before submitting:

```bash
cargo test -p codelens-core && cargo test -p codelens-mcp && cargo test -p codelens-mcp --features http
```

## License

Apache-2.0 — see [LICENSE](LICENSE).
