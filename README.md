<div align="center">

# CodeLens MCP

**The harness-native compressed context engine for AI coding agents.**

Pure Rust MCP server that plugs into any multi-agent harness — planner, builder, reviewer, refactor — and delivers bounded, ranked code intelligence at 50-87% fewer tokens.

[![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)](https://github.com/mupozg823/codelens-mcp-plugin/actions)
[![crates.io](https://img.shields.io/crates/v/codelens-mcp.svg)](https://crates.io/crates/codelens-mcp)
[![docs.rs](https://docs.rs/codelens-engine/badge.svg)](https://docs.rs/codelens-engine)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/codelens-mcp.svg)](https://crates.io/crates/codelens-mcp)

</div>

---

## The Problem

Multi-agent coding harnesses fail when every agent sees too many tools, too much raw code, and too many intermediate results. Tokens get burned on `tools/list`, repeated file reads, and low-value raw graph expansion.

## The Solution

CodeLens maintains a **live, indexed understanding** of your codebase and exposes it as a harness optimization layer. The model asks a precise question and gets a bounded answer with a handle for deeper expansion only when needed.

```
Without CodeLens                                    With CodeLens
─────────────────────────────────────────────────────────────────
Read file + grep references   → 4,600 tokens       get_impact_analysis    → 1,500 tokens  (67% saved)
Read manifest + entry + files → 5,000 tokens       onboard_project        →   660 tokens  (87% saved)
Read + grep × 3 files         → 3,200 tokens       get_ranked_context     →   800 tokens  (75% saved)
```

> Measured with tiktoken (cl100k_base) on real projects. Reproducible via `benchmarks/token-efficiency.py`.

## Quick Install

```bash
# From crates.io (recommended)
cargo install codelens-mcp

# Homebrew (macOS / Linux)
brew install mupozg823/tap/codelens-mcp

# From source
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp
```

## Setup

### Claude Code / Cursor

```json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": []
    }
  }
}
```

### Shared HTTP Daemon (Multi-Agent)

```bash
# Read-only for planners/reviewers
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# Mutation-enabled for refactor agents
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

```json
{
  "mcpServers": {
    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
  }
}
```

See [docs/platform-setup.md](docs/platform-setup.md) for Codex, Windsurf, VS Code, and other platforms.

## Why CodeLens?

|                       | CodeLens                             | Read/Grep baseline           |
| --------------------- | ------------------------------------ | ---------------------------- |
| **Token cost**        | 50-87% less                          | Full file content every time |
| **Context quality**   | Ranked, bounded, structured          | Raw text, no prioritization  |
| **Multi-file impact** | 1 tool call                          | 5-10 grep + read cycles      |
| **Runtime**           | Single Rust binary, <12ms cold start | N/A                          |
| **Language support**  | 25 languages, zero runtime deps      | N/A                          |
| **Agent awareness**   | Doom-loop detection, mutation gates  | None                         |

## Key Features

### Problem-First Workflows

Instead of starting from the full raw tool registry, begin with the workflow-first entrypoints:

| Workflow           | Tool                     | When                                  |
| ------------------ | ------------------------ | ------------------------------------- |
| Explore codebase   | `explore_codebase`       | First look or targeted context search |
| Trace execution    | `trace_request_path`     | Follow request or symbol flow         |
| Plan safe refactor | `plan_safe_refactor`     | Preview rename/refactor risk first    |
| Review changes     | `analyze_change_impact`  | Pre-merge impact and blast radius     |
| Audit architecture | `review_architecture`    | Boundaries, coupling, module shape    |
| Audit security     | `audit_security_context` | Risk-oriented changed-file review     |

### Role-Based Surfaces

| Profile            | Tools Visible   | Use Case                                 |
| ------------------ | --------------- | ---------------------------------------- |
| `planner-readonly` | Workflow-first  | Planner/architect context compression    |
| `builder-minimal`  | Workflow-first  | Implementation with focused Codex/agent surface |
| `reviewer-graph`   | Review-heavy    | Graph-aware review and risk analysis     |
| `refactor-full`    | Preview-first + gated mutation | Safe refactors               |
| `ci-audit`         | Machine-oriented| CI/CD review and report emission         |

### Adaptive Token Compression

5-stage budget-aware compression automatically adjusts response size:

- **Stage 1** (<75% budget): Full detail pass-through
- **Stage 2-3** (75-95%): Structured summarization
- **Stage 4-5** (>95%): Skeleton + truncation with expansion handles

### Analysis Handles

Heavy reports run as durable async jobs. Agents poll for completion and expand only needed sections:

```
start_analysis_job → get_analysis_job → get_analysis_section("impact")
```

### Mutation Safety

Refactor flows require verification before code changes:

```
verify_change_readiness → "ready" → rename_symbol
                        → "blocked" → fix blockers first
```

## 25 Languages

Python, JavaScript, TypeScript, TSX, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart, Lua, Zig, Elixir, Haskell, OCaml, Erlang, R, Bash, Julia

All via statically-linked tree-sitter grammars. Zero runtime dependencies.

## Performance

| Operation              | Time  | Backend                 |
| ---------------------- | ----- | ----------------------- |
| `find_symbol`          | <1ms  | SQLite FTS5             |
| `get_symbols_overview` | <1ms  | Cached                  |
| `get_ranked_context`   | ~20ms | 4-signal hybrid ranking |
| `get_impact_analysis`  | ~1ms  | Graph cache             |
| Cold start             | ~12ms | No LSP boot needed      |

## Semantic Search

Optional embedding-based code search (feature-gated: `semantic`):

- **Bundled MiniLM-L12 CodeSearchNet** model (ONNX INT8) — works offline
- Hybrid ranking: semantic supplements structural in `get_ranked_context`
- Measured quality: `MRR 0.639` hybrid vs `0.417` lexical-only (+53% uplift)

```bash
# Measure on your project
python3 benchmarks/embedding-quality.py . --isolated-copy
```

## vs Serena

| Axis             | CodeLens                             | Serena                    |
| ---------------- | ------------------------------------ | ------------------------- |
| Runtime          | Single Rust binary                   | Python + uv               |
| Intelligence     | tree-sitter + SQLite + optional LSP  | LSP by default            |
| Token efficiency | Bounded workflows, 50-87% savings    | Standard tool responses   |
| Workflow layer   | Composite reports + analysis handles | Symbolic tools            |
| Semantic search  | Bundled ONNX model + hybrid ranking  | No bundled model          |
| Refactoring      | Preview-first gated mutations        | Stronger IDE-backed edits |
| Offline          | Full support                         | Depends on backend        |

See [docs/serena-comparison.md](docs/serena-comparison.md) for detailed gap analysis.

## Building

```bash
cargo build --release                         # includes semantic (76MB)
cargo build --release --no-default-features   # without ML model (23MB)
cargo build --release --features http         # add HTTP transport

# Tests (537)
cargo test -p codelens-engine && cargo test -p codelens-mcp
```

## Harness Architecture

CodeLens is designed as a **harness coprocessor** — it doesn't replace your agent, it makes your agent's harness smarter.

```
┌──────────────────────────────────────────────────────────────────┐
│                        Agent Harness                             │
│                                                                  │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│   │ Planner  │  │ Builder  │  │ Reviewer  │  │ Refactor │       │
│   └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘       │
│        │              │              │              │             │
│        └──────────────┴──────────────┴──────────────┘             │
│                              │ MCP                               │
│                    ┌─────────▼──────────┐                        │
│                    │   CodeLens MCP     │                        │
│                    │  ┌──────────────┐  │                        │
│                    │  │  Profiles    │  │ planner-readonly       │
│                    │  │  Workflows   │  │ builder-minimal        │
│                    │  │  Handles     │  │ reviewer-graph         │
│                    │  │  Gates       │  │ refactor-full          │
│                    │  └──────┬───────┘  │                        │
│                    │         │          │                        │
│                    │  ┌──────▼───────┐  │                        │
│                    │  │codelens-engine│  │ tree-sitter + SQLite  │
│                    │  │  25 langs    │  │ + embedding + graphs  │
│                    │  └──────────────┘  │                        │
│                    └────────────────────┘                        │
└──────────────────────────────────────────────────────────────────┘
```

**Each agent role sees a different tool surface:**

- **Planner** gets `analyze_change_request`, `onboard_project` — compressed context, no mutations
- **Builder** gets `find_symbol`, `get_ranked_context` — minimal surface, focused implementation
- **Reviewer** gets `impact_report`, `diff_aware_references` — graph-aware bounded reviews
- **Refactor** gets `safe_rename_report`, `verify_change_readiness` — gate-protected mutations

**Harness primitives built in:**

- **Analysis handles** — agents expand only the section they need, not the full report
- **Mutation gates** — verification required before code changes, preventing blind rewrites
- **Doom-loop detection** — identical tool calls auto-detected and redirected
- **Token compression** — 5-stage adaptive budget keeps responses bounded
- **Suggested next tools** — contextual chaining guides agents through optimal tool sequences

## MCP Spec Compliance

| Feature                                 | Status                                 |
| --------------------------------------- | -------------------------------------- |
| Streamable HTTP + SSE                   | Supported                              |
| Role-based capability negotiation       | `--profile` flag                       |
| Tool Annotations (readOnly/destructive) | Supported                              |
| Tool Output Schemas                     | 45 tools covered                       |
| `.well-known/mcp.json` Server Card      | HTTP transport                         |
| Analysis handles + section expansion    | Supported                              |
| Durable analysis jobs                   | Supported                              |
| Mutation audit log                      | `.codelens/audit/mutation-audit.jsonl` |
| Multi-project queries                   | `query_project`                        |
| Contextual tool chaining                | `suggested_next_tools`                 |
| MCP 2025-03-26 spec                     | Full compliance                        |

## Contributing

Contributions are welcome! Please open an issue first to discuss what you'd like to change.

```bash
# Development workflow
cargo check && cargo test -p codelens-engine && cargo test -p codelens-mcp
cargo clippy -- -W clippy::all
```

## License

[Apache-2.0](LICENSE)
