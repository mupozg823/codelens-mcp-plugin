# CodeLens MCP

**The compressed context and verification tool for planner, reviewer, and refactor agent harnesses.**

Pure Rust MCP server with role-based tool surfaces, composite workflow tools, analysis handles, and tree-sitter-first code intelligence.

![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

## The Problem

Multi-agent coding harnesses fail when every agent sees too many tools, too much raw code, and too many intermediate results. Tokens get burned on `tools/list`, repeated file reads, and low-value raw graph expansion.

## The Solution

CodeLens maintains a **live, indexed understanding** of your codebase and exposes it as a harness optimization layer. The goal is not to show the model more code. The goal is to let the model ask a precise question and get a bounded answer with a handle for deeper expansion only when needed.

```
Without CodeLens: Read file + grep references → 4,600 tokens
With CodeLens:    get_impact_analysis         → 1,500 tokens (67% saved)

Without CodeLens: Read manifest + entry + README + file list → 5,000 tokens
With CodeLens:    onboard_project                            →   660 tokens (87% saved)
```

**Measured with tiktoken (cl100k_base) on real projects. 50-87% token reduction on structured tasks.**

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

### Shared HTTP Daemon (Preferred)

```bash
# Read-only shared daemon for planners, reviewers, and CI
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# Mutation-enabled daemon for explicit refactor passes
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

### Claude Code

```json
// .mcp.json (project) or ~/.claude.json (global)
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

### Cursor / VS Code / Codex / Windsurf

See [docs/platform-setup.md](docs/platform-setup.md) for all platforms.

### Stdio Fallback

Use stdio only for single local sessions:

```json
{
  "mcpServers": {
    "codelens": {
      "type": "stdio",
      "command": "codelens-mcp",
      "args": [".", "--profile", "builder-minimal"]
    }
  }
}
```

## What It Does

### For Agent Harnesses

| Need | Preferred Tool | Why |
| --- | --- | --- |
| "Compress a change request" | `analyze_change_request` | Returns ranked files, key symbols, risks, and next actions |
| "Start with the smallest useful context" | `find_minimal_context_for_change` | Avoids raw file/graph expansion |
| "Review module boundaries" | `module_boundary_report` | Bounded impact, coupling, and cycle evidence |
| "Check rename safety" | `safe_rename_report` | Preview-first report with blockers and sections |
| "Compress changed-file impact" | `impact_report` | Reviewer-friendly impact summary with bounded blast radius |
| "Compress diff references" | `diff_aware_references` | Keeps reviewer/CI context short around changed files |
| "Poll a durable report" | `start_analysis_job` → `get_analysis_job` | Async-friendly workflow for heavier reports |
| "Expand only one stored section" | `get_analysis_section` | Keeps the default answer short |

<details>
<summary>Benchmark methodology</summary>

Token counts measured with **tiktoken cl100k_base** (same tokenizer used by Claude/GPT-4).
Baselines simulate actual agent workflows: `rg -n` for search, `read file` for structure,
`read + search` for impact analysis. The benchmark now also compares `preset:balanced` low-level
tool chains against `planner-readonly` / `reviewer-graph` / `refactor-full` composite workflows.
No arbitrary multipliers. Tested on 2 projects (Rust 92 files, TypeScript 60 files).
Run `python3 benchmarks/token-efficiency.py <project>` to reproduce.

</details>

### Role-Based Surfaces

| Profile | Use case | Default transport |
| --- | --- | --- |
| `planner-readonly` | planner/architect context compression | `stdio` or HTTP |
| `builder-minimal` | implementation with minimal visible tool surface | `stdio` |
| `reviewer-graph` | graph-aware review and risk analysis | HTTP preferred |
| `refactor-full` | preview-first refactors and structured edits | `stdio` or HTTP |
| `ci-audit` | machine-friendly review around diffs and risk | HTTP preferred |

`ci-audit` composite reports use a fixed machine schema with `schema_version`, `report_kind`, `machine_summary`, and `evidence_handles` so CI can parse them without relying on prose.
`refactor-full` now enforces preflight-first mutation: run `verify_change_readiness` before file mutations, and use `safe_rename_report` or `unresolved_reference_check` before `rename_symbol`.

## Why This Shape

CodeLens is no longer primarily a "more tools" MCP. It is a bounded-answer MCP.

- Composite tools create short, high-value reports.
- Analysis handles let agents expand only one section at a time.
- Durable analysis jobs let harnesses poll heavier reports without dumping raw intermediate output into the model.
- Resources expose stable project/profile context without repeating long prompt instructions.
- `tools/list` can now be filtered by namespace or tier, and HTTP clients can opt into deferred loading during `initialize` with `{"deferredToolLoading": true}` so the default tool list only loads preferred namespaces and tiers first. In deferred bootstrap mode, `tools/list` omits `outputSchema` by default to reduce token overhead; clients can opt back in with `{"includeOutputSchema": true}`. Once a client expands a namespace or tier, later default `tools/list` calls include it; hidden namespaces and primitive tiers can also gate `tools/call` until the client explicitly loads them.
- The same deferred session state now applies to `codelens://tools/list` and `codelens://session/http` resources, so tool/resource discovery stays in sync.
- Legacy presets still work, but profiles are the preferred public interface.

## Shared Daemon Patterns

```bash
# Read-only shared daemon for planner/reviewer agents
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# Mutation-enabled shared daemon for refactor flows
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

Use `7837`-style read-only endpoints as the default harness attachment. Reserve mutation-enabled daemons for explicit refactor passes.
Mutation-enabled flows are gate-aware: recent matching verifier evidence is required before `refactor-full` content mutations execute.

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
| semantic_search      | warm, workload-dependent | Measure with `benchmarks/embedding-runtime.py` |
| Cold start           | ~12ms | No LSP boot needed      |

## Embedding Model

CodeLens defaults to a **bundled MiniLM-L12 CodeSearchNet model** (ONNX INT8) and can optionally use a `fastembed` built-in model via `CODELENS_EMBED_MODEL`:

- **No download required** — works offline, air-gapped environments
- **Current default model:** `MiniLM-L12-CodeSearchNet-INT8`
- **Hybrid usage:** semantic ranking only supplements structural ranking in `get_ranked_context`
- Powers: `semantic_search`, `get_ranked_context` hybrid ranking, `find_similar_code`, `find_code_duplicates`

Measure current runtime latency, indexing cost, and indexed symbol counts on your machine:

```bash
python3 benchmarks/embedding-runtime.py . --isolated-copy
```

Measure search quality and hybrid uplift on the current runtime:

```bash
python3 benchmarks/embedding-quality.py . --isolated-copy
```

The quality report now breaks results down by query type:

- `identifier`
- `short_phrase`
- `natural_language`

`get_ranked_context` now applies a query-type-aware policy:

- identifier-like queries stay lexical-first
- short phrases and natural-language queries keep hybrid semantic blending

Current reproducible local quality snapshot (`benchmarks/embedding-quality-results.json`, sequential run with `--isolated-copy`):

- `semantic_search`: `MRR 0.502`, `Acc@1 44%`, `Acc@3 56%`, `Acc@5 62%`
- `get_ranked_context` lexical-only: `MRR 0.407`, `Acc@1 28%`, `Acc@3 47%`, `Acc@5 53%`
- `get_ranked_context` hybrid: `MRR 0.654`, `Acc@1 53%`, `Acc@3 69%`, `Acc@5 78%`
- Hybrid uplift over lexical-only: `+0.246 MRR`, `+25% Acc@1`, `+22% Acc@3`, `+25% Acc@5`
- Identifier queries: hybrid uplift is neutral because `get_ranked_context` now stays lexical-first for identifier-like queries
- The benchmark scripts now fail fast if `index_embeddings` or any measured tool call fails, so stale partial outputs are no longer treated as valid results.

## vs Serena

Both are code intelligence MCP servers. Different trade-offs:

|                       | CodeLens                          | Serena                                 |
| --------------------- | --------------------------------- | -------------------------------------- |
| **Language**          | Rust                              | Python                                 |
| **Core engine**       | tree-sitter (AST)                 | LSP (language servers)                 |
| **Type resolution**   | Opt-in via LSP (`use_lsp=true`)   | Always-on (LSP is the core engine)     |
| **Setup**             | Single binary, zero config        | Python + uv + per-language LSP servers |
| **Cold start**        | 12ms                              | Seconds (LSP boot per language)        |
| **Offline / air-gap** | Fully offline (ML model bundled)  | Partial (needs LSP binaries)           |
| **ML / semantic**     | Bundled CodeSearchNet ONNX model  | None                                   |
| **Refactoring**       | 4 operations (inline, move, etc.) | 1 (replace symbol body)                |
| **Languages**         | 25 (tree-sitter grammars)         | 40+ (via LSP ecosystem)                |
| **Token budget**      | Role profiles + legacy presets    | No                                     |
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

For the repo-local development flow, see `DEVELOPMENT_PIPELINE.md`.

## Agentic Architecture

| Feature                                 | Status                                      |
| --------------------------------------- | ------------------------------------------- |
| Streamable HTTP + SSE                   | Supported                                   |
| Role-based capability negotiation       | `--profile` + legacy `--preset`             |
| Tool Annotations (readOnly/destructive) | Supported                                   |
| Tool Output Schemas                     | Core tools + analysis handles               |
| `.well-known/mcp.json` Server Card      | HTTP transport                              |
| Analysis handles + section expansion    | Supported                                   |
| Durable analysis jobs                   | `start_analysis_job` + `get_analysis_job`   |
| Mutation audit log                      | `.codelens/audit/mutation-audit.jsonl`      |
| Token budget control (`_profile`)       | legacy shortcuts + role-profile budgets      |
| Multi-project queries                   | `query_project`                             |
| Contextual tool chaining                | `suggested_next_tools`                      |
| MCP 2025-03-26 spec compliant           | Full                                        |

## License

Apache-2.0
