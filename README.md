# CodeLens MCP

Pure Rust MCP server for code intelligence — 62 tools, 25 languages, tree-sitter-first, zero external dependencies.

![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

## Why CodeLens?

- **Single binary, zero config.** 25 languages built in via tree-sitter. No LSP servers, no Node, no Python required.
- **Tree-sitter-first.** Millisecond responses, works on incomplete code, no external server startup. LSP available as opt-in bonus.
- **62 tools, 3 presets.** From 21-tool minimal (for subagents) to 62-tool full — switch at runtime, no restart.
- **Built for AI agents.** Output schemas, tool annotations, `.well-known` server card, token budget control, contextual tool chaining.
- **Free and open-source.** Apache-2.0.

## Quick Install

```bash
# One-line installer (auto-detects and configures Claude Code, Cursor, VS Code, Codex)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# Or via Cargo
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp
```

## Configure Your AI Agent

### Claude Code

```json
// .mcp.json (project) or ~/.claude.json (global)
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

### Cursor

```json
// .cursor/mcp.json or ~/.cursor/mcp.json
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

```toml
# ~/.codex/config.toml
[mcp_servers.codelens]
command = "codelens-mcp"
args = [".", "--preset", "balanced"]
```

### VS Code (Copilot / Cline / Continue)

```json
// .vscode/mcp.json
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

### Windsurf / Zed / Claude Desktop

See [docs/platform-setup.md](docs/platform-setup.md) for all platforms including Docker and Claude Agent SDK.

## Architecture

```
Agent (Claude/Codex/Cursor/...)
  │
  │  MCP Protocol (stdio / Streamable HTTP+SSE)
  │
  ▼
┌─────────────────────────────────────────────────────┐
│  codelens-mcp (Server)                               │
│  ┌────────────┐ ┌──────────┐ ┌───────────────────┐ │
│  │  Dispatch   │ │  Tools   │ │  Telemetry        │ │
│  │  Registry   │ │  (62)    │ │  Metrics + Budget │ │
│  └────────────┘ └────┬─────┘ └───────────────────┘ │
│                      │                               │
│  Symbol(14) Edit(12) Analysis(7) File(7)            │
│  Memory(5) Session(12) Semantic(2) Composite(1)     │
├─────────────────────────┬───────────────────────────┤
│  codelens-core (Engine) │                            │
│  ┌───────────┐ ┌────────┴──┐ ┌──────────────────┐  │
│  │ Symbols   │ │  Import   │ │  SQLite FTS5     │  │
│  │ tree-sit. │ │  Graph    │ │  4-signal rank   │  │
│  │ 25 langs  │ │ PageRank  │ │  text+PR+rec+sem │  │
│  └───────────┘ └───────────┘ └──────────────────┘  │
│  ┌───────────┐ ┌───────────┐ ┌──────────────────┐  │
│  │ LSP pool  │ │  Watcher  │ │  Embeddings      │  │
│  │ (opt-in)  │ │  (notify) │ │  (fastembed)     │  │
│  └───────────┘ └───────────┘ └──────────────────┘  │
└─────────────────────────────────────────────────────┘
```

## Tool Categories (62 tools)

| Category      | Tools | Key tools                                                                               |
| ------------- | ----- | --------------------------------------------------------------------------------------- |
| **Symbol**    | 14    | `find_symbol`, `get_symbols_overview`, `get_ranked_context`, `find_referencing_symbols` |
| **Edit**      | 12    | `rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `add_import`       |
| **Analysis**  | 7     | `get_impact_analysis`, `find_dead_code`, `find_circular_dependencies`, `get_callers`    |
| **File**      | 7     | `read_file`, `search_for_pattern`, `find_tests`, `find_annotations`                     |
| **Session**   | 12    | `onboard_project`, `activate_project`, `query_project`, `set_preset`                    |
| **Memory**    | 5     | `write_memory`, `read_memory`, `list_memories`                                          |
| **Semantic**  | 2     | `semantic_search`, `index_embeddings`                                                   |
| **Composite** | 1     | `refactor_extract_function`                                                             |

## Presets

| Preset       | Tools | Use case                                                     |
| ------------ | ----- | ------------------------------------------------------------ |
| **FULL**     | 62    | Everything — advanced analysis, all file ops                 |
| **BALANCED** | 39    | Default — excludes niche analysis + Claude built-in overlaps |
| **MINIMAL**  | 21    | Subagents, token-constrained environments                    |

```bash
codelens-mcp . --preset balanced     # CLI flag
CODELENS_PRESET=minimal codelens-mcp .  # env var
# or call set_preset at runtime
```

## Languages (25)

Python, JavaScript, TypeScript, TSX, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart, Lua, Zig, Elixir, Haskell, OCaml, Erlang, R, Bash, Julia

All languages use statically-linked tree-sitter grammars — zero external dependencies.

## Performance

| Operation            | Time  | Notes             |
| -------------------- | ----- | ----------------- |
| find_symbol          | <1ms  | SQLite FTS5 index |
| get_symbols_overview | <1ms  | Cached per-file   |
| get_ranked_context   | ~50ms | 4-signal ranking  |
| get_impact_analysis  | ~1ms  | Graph cache       |
| Cold start           | ~12ms | No LSP boot       |

## Key Design Principles

```
tree-sitter-first: milliseconds, zero-config, works on broken code
LSP: opt-in bonus via use_lsp=true (not required)

Agent priorities: speed > availability > stability > precision
```

## Building

```bash
cargo build --release                         # standard (includes semantic)
cargo build --release --no-default-features   # minimal (no embeddings)
cargo build --release --features http         # add HTTP transport

# Tests (190+)
cargo test -p codelens-core && cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
```

## Agentic Architecture Ready

| Feature                                 | Status                                  |
| --------------------------------------- | --------------------------------------- |
| Streamable HTTP + SSE                   | Supported                               |
| Tool Annotations (readOnly/destructive) | All tools                               |
| Tool Output Schemas                     | 13 core tools                           |
| `.well-known/mcp.json` Server Card      | HTTP transport                          |
| Preset-based capability negotiation     | 3 presets                               |
| Token budget control (`_profile`)       | `fast_local`/`balanced`/`deep_semantic` |
| Multi-project queries                   | `query_project`                         |
| Contextual tool chaining                | `suggested_next_tools`                  |

## License

Apache-2.0
