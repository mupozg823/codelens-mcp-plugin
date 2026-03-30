# CodeLens MCP

**The code intelligence engine that makes every AI agent smarter.**

Pure Rust MCP server — 70 tools, 25 languages, code-trained ML model bundled, single binary, zero config.

![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

## The Problem

AI coding agents waste tokens re-reading files, miss context, and lose understanding between sessions. Studies show 65% of developers cite "missing context" as their top frustration with AI tools.

## The Solution

CodeLens maintains a **live, indexed understanding** of your codebase — so any AI agent can get the right context in one call instead of reading dozens of files.

```
Without CodeLens: Agent reads 20 files → 12,000+ tokens → slow, noisy
With CodeLens:    get_impact_analysis → 874 tokens → precise, instant
                  onboard_project    → 595 tokens → complete overview
```

**93-97% token reduction. One tool call instead of six.**

## Quick Install

```bash
# Homebrew (macOS / Linux)
brew install mupozg823/tap/codelens-mcp

# One-line installer (auto-configures Claude Code, Cursor, VS Code, Codex)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# Cargo
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp
```

## Works With Every AI Agent

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

### Cursor / VS Code / Codex / Windsurf

See [docs/platform-setup.md](docs/platform-setup.md) for all platforms.

## What It Does

### For AI Agents (why they call CodeLens)

| Need                               | Tool                    | Tokens saved                  |
| ---------------------------------- | ----------------------- | ----------------------------- |
| "What's in this codebase?"         | `onboard_project`       | **97%** vs manual exploration |
| "What breaks if I change this?"    | `get_impact_analysis`   | **93%** vs read + grep        |
| "Find relevant code for this task" | `get_ranked_context`    | **69%** vs keyword grep       |
| "Rename this symbol safely"        | `rename_symbol`         | Multi-file, scope-aware       |
| "Move this to another file"        | `refactor_move_to_file` | Rewrites imports              |
| "Find similar code"                | `find_similar_code`     | ML-powered semantic match     |

### Under the Hood

```
                    ┌──────────────────────────────┐
Agent Request ────▶ │     70 MCP Tools             │
                    │  ┌────────┐ ┌─────────────┐  │
                    │  │ Symbol │ │ Refactoring  │  │
                    │  │ Search │ │ 4 operations │  │
                    │  └────┬───┘ └──────┬──────┘  │
                    │       │            │          │
                    │  ┌────▼────────────▼──────┐  │
                    │  │   Hybrid Ranking Engine │  │
                    │  │  FTS5 + PageRank +      │  │
                    │  │  Semantic + Recency     │  │
                    │  └────────────┬───────────┘  │
                    │  ┌────────────▼───────────┐  │
                    │  │  tree-sitter (25 langs) │  │
                    │  │  Import graph + cache   │  │
                    │  │  ML model (bundled 34MB)│  │
                    │  │  SQLite FTS5 + vec      │  │
                    │  └────────────────────────┘  │
                    └──────────────────────────────┘
                         57MB single binary
                         12ms cold start
                         Zero external dependencies
```

## 70 Tools in 8 Categories

| Category        | Count | Highlights                                                                                                    |
| --------------- | ----- | ------------------------------------------------------------------------------------------------------------- |
| **Symbol**      | 14    | `find_symbol`, `get_symbols_overview`, `get_ranked_context`, `find_referencing_symbols`                       |
| **Edit**        | 16    | `rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `add_import`                             |
| **Refactoring** | 4     | `refactor_extract_function`, `refactor_inline_function`, `refactor_move_to_file`, `refactor_change_signature` |
| **Analysis**    | 7     | `get_impact_analysis`, `find_dead_code`, `find_circular_dependencies`                                         |
| **Semantic**    | 6     | `semantic_search`, `find_similar_code`, `find_code_duplicates`, `classify_symbol`, `find_misplaced_code`      |
| **File**        | 7     | `read_file`, `search_for_pattern`, `find_tests`, `find_annotations`                                           |
| **Session**     | 12    | `onboard_project`, `activate_project`, `query_project`, `set_preset`                                          |
| **Memory**      | 5     | `write_memory`, `read_memory`, `list_memories`                                                                |

## 3 Presets

| Preset       | Tools | Use case                                                  |
| ------------ | ----- | --------------------------------------------------------- |
| **FULL**     | 70    | Everything — advanced analysis, semantic, all refactoring |
| **BALANCED** | 39    | Default — core tools optimized for typical workflows      |
| **MINIMAL**  | 21    | Subagents, token-constrained environments                 |

## 25 Languages

Python, JavaScript, TypeScript, TSX, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart, Lua, Zig, Elixir, Haskell, OCaml, Erlang, R, Bash, Julia

All via statically-linked tree-sitter grammars. Zero runtime dependencies.

## Performance

| Operation            | Time  | Notes                   |
| -------------------- | ----- | ----------------------- |
| find_symbol          | <1ms  | SQLite FTS5             |
| get_symbols_overview | <1ms  | Cached                  |
| get_ranked_context   | ~20ms | 4-signal hybrid ranking |
| get_impact_analysis  | ~1ms  | Graph cache             |
| semantic_search      | ~9ms  | Bundled ONNX model      |
| Cold start           | ~12ms | No LSP boot needed      |

## Embedding Model

CodeLens bundles a **code-trained MiniLM-L12 model** (fine-tuned on CodeSearchNet, ONNX INT8) directly in the binary:

- **No download required** — works offline, air-gapped environments
- **MRR 0.878** on code search benchmarks
- **8.5ms** per query, **686 symbols/sec** indexing throughput
- Powers: `semantic_search`, `get_ranked_context` hybrid ranking, `find_similar_code`, `find_code_duplicates`

## vs Serena

Both are code intelligence MCP servers. Different trade-offs:

|                       | CodeLens                          | Serena                                 |
| --------------------- | --------------------------------- | -------------------------------------- |
| **Language**          | Rust                              | Python                                 |
| **Core engine**       | tree-sitter (AST)                 | LSP (language servers)                 |
| **Type resolution**   | No (AST-level only)               | Yes (full type-aware via LSP)          |
| **Setup**             | Single binary, zero config        | Python + uv + per-language LSP servers |
| **Cold start**        | 12ms                              | Seconds (LSP boot per language)        |
| **Offline / air-gap** | Fully offline (ML model bundled)  | Partial (needs LSP binaries)           |
| **ML / semantic**     | Bundled ONNX model (34MB)         | None                                   |
| **Refactoring**       | 4 operations (inline, move, etc.) | 1 (replace symbol body)                |
| **Languages**         | 25 (tree-sitter grammars)         | 40+ (via LSP ecosystem)                |
| **Token budget**      | 3 presets + per-call budget       | No                                     |
| **Stars**             | New project                       | 22K+                                   |

**When to choose CodeLens:** Fast setup, offline environments, token-efficient agent workflows, refactoring operations.

**When to choose Serena:** Deep type-aware analysis, languages not covered by tree-sitter, existing LSP infrastructure.

## Building

```bash
cargo build --release                         # includes semantic (57MB)
cargo build --release --no-default-features   # without ML model (23MB)
cargo build --release --features http         # add HTTP transport

# Tests (209+)
cargo test -p codelens-core && cargo test -p codelens-mcp
```

## Agentic Architecture

| Feature                                 | Status                                      |
| --------------------------------------- | ------------------------------------------- |
| Streamable HTTP + SSE                   | Supported                                   |
| Tool Annotations (readOnly/destructive) | All 70 tools                                |
| Tool Output Schemas                     | 13 core tools                               |
| `.well-known/mcp.json` Server Card      | HTTP transport                              |
| Preset-based capability negotiation     | 3 presets                                   |
| Token budget control (`_profile`)       | `fast_local` / `balanced` / `deep_semantic` |
| Multi-project queries                   | `query_project`                             |
| Contextual tool chaining                | `suggested_next_tools`                      |
| MCP 2025-03-26 spec compliant           | Full                                        |

## License

Apache-2.0
