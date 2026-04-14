<div align="center">

# CodeLens MCP

**Host-orchestrated code intelligence server for bounded evidence, preflight, and async analysis.**

Pure Rust MCP server for multi-agent harnesses. 25 languages, hybrid retrieval (tree-sitter + semantic), mutation-gated refactoring, 5-stage token compression, and enterprise-ready observability — all in a single binary with zero runtime dependencies.

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

CodeLens maintains a **live, indexed understanding** of your codebase and exposes it as a harness optimization layer. The host keeps orchestration ownership, and CodeLens returns bounded evidence, safety contracts, and async handles when deeper expansion is needed.

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

# GitHub installer (prebuilt release binary)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# From source
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp
```

Latest release notes: [v1.9.26](docs/release-notes/v1.9.26.md)

Support policy: [docs/support-policy.md](docs/support-policy.md)

Release docs check: `python3 scripts/check-release-docs.py`

Workspace crates publish dry-run: `scripts/publish-crates-workspace.sh --allow-dirty`

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

### Distribution Channels

| Channel          | Delivery                                  | Notes                                            |
| ---------------- | ----------------------------------------- | ------------------------------------------------ |
| crates.io        | `cargo install codelens-mcp`              | Standard Rust install path                       |
| Homebrew tap     | `brew install mupozg823/tap/codelens-mcp` | macOS/Linux package install                      |
| GitHub Releases  | prebuilt archives                         | `darwin-arm64`, `linux-x86_64`, `windows-x86_64` |
| installer script | `install.sh`                              | Convenience bootstrap for release assets         |
| source build     | `cargo build --release`                   | Custom feature builds and local hacking          |

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

### Problem-First Entrypoints

Instead of starting from the full raw tool registry, start with bounded entrypoints and let the host keep orchestration ownership:

| Entrypoint         | Tool                     | When                                  |
| ------------------ | ------------------------ | ------------------------------------- |
| Explore codebase   | `explore_codebase`       | First look or targeted context search |
| Trace execution    | `trace_request_path`     | Follow request or symbol flow         |
| Plan safe refactor | `plan_safe_refactor`     | Preview rename/refactor risk first    |
| Review changes     | `analyze_change_impact`  | Pre-merge impact and blast radius     |
| Audit architecture | `review_architecture`    | Boundaries, coupling, module shape    |
| Audit security     | `audit_security_context` | Risk-oriented changed-file review     |

### Role-Based Surfaces

| Profile            | Tools Visible                  | Use Case                                        |
| ------------------ | ------------------------------ | ----------------------------------------------- |
| `planner-readonly` | Workflow-first                 | Planner/architect context compression           |
| `builder-minimal`  | Workflow-first                 | Implementation with focused Codex/agent surface |
| `reviewer-graph`   | Review-heavy                   | Graph-aware review and risk analysis            |
| `refactor-full`    | Preview-first + gated mutation | Safe refactors                                  |
| `ci-audit`         | Machine-oriented               | CI/CD review and report emission                |

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
- 2-tier NL→code bridging: generic core (15 entries) + auto-generated project bridges (`.codelens/bridges.json`)
- Multi-language test symbol filtering: Python, JS/TS, Go, Java, Kotlin, Ruby

### Retrieval Quality (v1.9.23)

| Project            | Language | Hybrid MRR | Semantic MRR | Queries |
| ------------------ | -------- | ---------- | ------------ | ------- |
| Self (CodeLens)    | Rust     | **0.841**  | 0.798        | 104     |
| Role (adversarial) | Rust     | **0.962**  | 0.900        | 70      |
| Flask              | Python   | 0.563      | **0.577**    | 20      |
| curl               | C        | **0.623**  | 0.555        | 18      |

6-language benchmark matrix: Rust (self/axum/ripgrep), Python (django/requests), TS/JS (jest/next-js/react-core/typescript), Go (gin), Java (gson), C (curl).

> Generic bridge only — no project-specific tuning. Hybrid > lexical in all languages.
> With project bridges (`.codelens/bridges.json`): self MRR rises to 0.841.

```bash
# Measure on your project
python3 benchmarks/embedding-quality.py . \
  --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json
```

## Enterprise Features

| Feature                    | Status                                                                     |
| -------------------------- | -------------------------------------------------------------------------- |
| Config policy              | `.codelens/config.json` per-project feature flags                          |
| Rate limiting              | Session-level throttle (default 300 calls, configurable)                   |
| Schema versioning          | `schema_version: "1.0"` in all responses                                   |
| Intelligence sources       | `tree_sitter`, `lsp`, `semantic`, `scip` — reported via `get_capabilities` |
| Mutation audit log         | `.codelens/audit/mutation-audit.jsonl`                                     |
| OTel exporter              | OTLP gRPC via `--features otel` + `CODELENS_OTEL_ENDPOINT` env var         |
| OTel-ready spans           | `tool.success`, `tool.backend`, `tool.elapsed_ms`, `otel.status_code`      |
| OTel local verification    | `docker-compose.otel.yml` + `scripts/verify-otel-local.sh`                 |
| SBOM                       | CycloneDX per release                                                      |
| Dataset lint               | CI-integrated benchmark hygiene (5 rules)                                  |
| Multi-language test filter | Python, JS/TS, Go, Java, Kotlin, Ruby test symbols excluded from index     |
| SCIP precise backend       | `--features scip-backend` — definitions, references, diagnostics, hover    |
| Docker                     | Multi-stage `Dockerfile.release` with healthcheck                          |

## vs Serena

| Axis             | CodeLens                                    | Serena                    |
| ---------------- | ------------------------------------------- | ------------------------- |
| Runtime          | Single Rust binary, <12ms cold start        | Python + uv               |
| Intelligence     | tree-sitter + SQLite + optional LSP/SCIP    | LSP by default            |
| Token efficiency | Bounded workflows, 50-87% savings           | Standard tool responses   |
| Workflow layer   | Composite reports + analysis handles        | Symbolic tools            |
| Semantic search  | Bundled ONNX + hybrid ranking + NL bridging | No bundled model          |
| Refactoring      | Preview-first gated mutations               | Stronger IDE-backed edits |
| Enterprise       | Config policy, rate limit, OTel, SBOM       | None                      |
| Offline          | Full support (bundled model + air-gap)      | Depends on backend        |

See [docs/serena-comparison.md](docs/serena-comparison.md) for detailed gap analysis.

## Building

```bash
cargo build --release                              # includes semantic (76MB)
cargo build --release --no-default-features        # without ML model (23MB)
cargo build --release --features http              # add HTTP transport
cargo build --release --features otel              # add OpenTelemetry OTLP exporter
cargo build --release --features scip-backend      # add SCIP precise navigation
cargo build --release --features http,otel         # HTTP + OTel

# MCP smoke test (real MCP handshake)
./scripts/mcp-smoke.sh . --transport stdio
./scripts/mcp-smoke.sh . --transport http

# MCP config/runtime diagnosis
./scripts/mcp-doctor.sh .
./scripts/mcp-doctor.sh . --strict

# Local dev install sync (~/.local/bin -> this repo's release build)
bash ./scripts/sync-local-bin.sh .

# Core verification
cargo test -p codelens-engine
cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
cargo test -p codelens-mcp --no-default-features   # semantic=off path

# Local OTel validation
docker compose -f docker-compose.otel.yml up -d
./scripts/verify-otel-local.sh
```

### MCP Transport

Use stdio by default (Codex/Claude spawns the server per session). For a local HTTP daemon:

```bash
./scripts/mcp-http-run.sh .
```

Use the doctor to confirm runtime/config alignment with an actual MCP attach:

```bash
./scripts/mcp-doctor.sh . --strict
```

If the configured stdio command is stale, relink `~/.local/bin/codelens-mcp` to the current workspace build:

```bash
bash ./scripts/sync-local-bin.sh .
```

### Feature Flags

| Feature        | Description                            | Binary Size Impact |
| -------------- | -------------------------------------- | ------------------ |
| `semantic`     | Bundled ONNX embedding model (default) | +53MB              |
| `http`         | Streamable HTTP + SSE transport        | +2MB               |
| `otel`         | OpenTelemetry OTLP gRPC exporter       | +4MB               |
| `scip-backend` | SCIP index precise navigation          | +1MB               |

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
| Tool Output Schemas                     | 32+ schemas covering core tools        |
| `.well-known/mcp.json` Server Card      | HTTP transport                         |
| Analysis handles + section expansion    | Supported                              |
| Durable analysis jobs                   | Supported                              |
| Mutation audit log                      | `.codelens/audit/mutation-audit.jsonl` |
| Multi-project queries                   | `query_project`                        |
| Contextual tool chaining                | `suggested_next_tools`                 |
| MCP 2025-03-26 spec                     | Full compliance                        |

## Quality Assurance

| Suite                      | Tests   | Scope                                      |
| -------------------------- | ------- | ------------------------------------------ |
| codelens-engine            | 286     | Parsing, ranking, embedding, IR            |
| codelens-mcp               | 210     | Dispatch, workflows, profiles, schemas     |
| codelens-mcp (no semantic) | 190     | Feature-off path verification              |
| Dataset lint               | 5 rules | file_exists, negative≠positive, duplicates |

```bash
# Full verification
cargo test -p codelens-engine && cargo test -p codelens-mcp
cargo test -p codelens-mcp --no-default-features  # semantic=off path
python3 benchmarks/lint-datasets.py --project .     # dataset hygiene
```

## Contributing

Contributions are welcome! Please open an issue first to discuss what you'd like to change.

```bash
# Development workflow
cargo check && cargo test -p codelens-engine && cargo test -p codelens-mcp
cargo clippy -- -W clippy::all
```

## License

[Apache-2.0](LICENSE)
