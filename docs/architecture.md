# CodeLens MCP — Architecture & Project Overview

> Pure Rust MCP server and harness optimization tool for code intelligence
> Harness optimization control plane with generated surface governance and tree-sitter-first retrieval

## Current Snapshot (generated 2026-05-04)

<!-- SURFACE_MANIFEST_ARCHITECTURE_SNAPSHOT:BEGIN -->

- Workspace version: `1.13.22`
- Workspace members: `3` (`crates/codelens-engine`, `crates/codelens-mcp`, `crates/codelens-tui`)
- Registered tool definitions in source: `111`
- Tool output schemas in source: `77 / 111`
- Supported language families: `30` across `49` extensions
- Canonical manifest: [`docs/generated/surface-manifest.json`](generated/surface-manifest.json)

<!-- SURFACE_MANIFEST_ARCHITECTURE_SNAPSHOT:END -->
- Runtime surface is profile- and session-dependent; use [`prepare_harness_session`](../crates/codelens-mcp/src/tools/session/project_ops.rs) and `tools/list` for live counts rather than this document
- Published distribution channels: crates.io, GitHub Releases, Homebrew tap, installer script, source builds
- Current release notes: [latest GitHub release](https://github.com/mupozg823/codelens-mcp-plugin/releases/latest). For local release-quality comparisons, resolve the baseline tag with `git tag --sort=-v:refname | head -1`.
- Current release verification guide: [docs/release-verification.md](release-verification.md)
- Current external comparison status: CodeLens is stronger as a harness-native MCP layer, but not yet a strict Serena superset. See [docs/serena-comparison.md](serena-comparison.md).
- Current audit and simplification report: [docs/architecture-audit-2026-04-24.md](architecture-audit-2026-04-24.md)
- Current simplification decision record: [docs/adr/ADR-0001-runtime-boundaries-and-single-source-registries.md](adr/ADR-0001-runtime-boundaries-and-single-source-registries.md)
- Current enterprise productization decision record: [docs/adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)

This document describes the product shape and the stable architectural layers.
The audit document above captures the current overdesign, duplication, and drift findings against the latest code.

---

## Retrieval Adaptation Tiers

Semantic query shaping is split into two explicit tiers:

- generic adaptation lives in code and is expected to transfer across repositories
- project-specific adaptation lives in `.codelens/bridges.json` at the project root

Generic adaptation is limited to repository-agnostic shaping such as identifier splitting, natural-language code framing, and generic NL-to-code vocabulary alignment.

Project-specific adaptation is opt-in and file-backed. It is intended for repository-local vocabulary only and must not be used as evidence for general retrieval claims.

Current project bridge file format:

```json
[
  { "nl": "recently accessed", "code": "record_file_access recency" },
  { "nl": "stdin", "code": "run_stdio stdio" }
]
```

Rules:

- `nl` is the lower-signal natural-language phrase to detect
- `code` is the repository-local code vocabulary appended for embedding search
- missing or malformed `.codelens/bridges.json` is treated as empty
- generic bridges remain active without any project file

---

## Distribution Surface

CodeLens is currently packaged and deployed through four user-facing channels and one source path:

| Channel          | Current shape                             | Notes                                            |
| ---------------- | ----------------------------------------- | ------------------------------------------------ |
| crates.io        | `cargo install codelens-mcp`              | Fastest path for Rust users                      |
| GitHub Releases  | prebuilt tar/zip artifacts                | `darwin-arm64`, `linux-x86_64`, `windows-x86_64` |
| Homebrew tap     | `brew install mupozg823/tap/codelens-mcp` | Generated from release checksums in CI           |
| Installer script | `install.sh`                              | Convenience wrapper over published binaries      |
| Source build     | `cargo build --release`                   | Required for custom feature combinations         |

Operational deployment modes:

- stdio for single local agent sessions
- Streamable HTTP + SSE for shared daemon or multi-agent deployments
- read-only daemon mode for reviewer/planner/CI surfaces
- mutation-enabled daemon mode only for explicit refactor surfaces

---

## 1. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                 Agent Runtime / Harness Layer                        │
│  Claude Code / OpenAI Agents / LangGraph / Custom Agent SDK         │
├───────────────────────┬─────────────────────────────────────────────┤
│    A2A (future)       │              MCP Protocol                   │
│  Agent ↔ Agent        │  JSON-RPC 2.0 over stdio / Streamable HTTP  │
├───────────────────────┴─────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │          codelens-mcp (Harness Optimization Server)           │  │
│  │                                                               │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │  │
│  │  │ Dispatch │→ │  Tools   │→ │  State   │→ │  Telemetry   │  │  │
│  │  │  Table   │  │ (surface │  │ AppState │  │  Metrics     │  │  │
│  │  │          │  │ dependent)│  │          │  │              │  │  │
│  │  └──────────┘  └────┬─────┘  └──────────┘  └──────────────┘  │  │
│  │                     │                                         │  │
│  │  ┌─────────────────────────────────────────────────────────┐  │  │
│  │  │              Tool Categories (profile dependent)        │  │  │
│  │  │  File │ Symbol │ LSP │ Analysis │ Edit                  │  │  │
│  │  │  Workflow │ Memory │ Session │ Semantic*               │  │  │
│  │  │                         * cfg-gated                     │  │  │
│  │  └─────────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────┬───────────────────────────────┘  │
│                                  │                                   │
│  ┌───────────────────────────────▼───────────────────────────────┐  │
│  │                 codelens-engine (Engine)                      │  │
│  │                                                               │  │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌───────────────┐    │  │
│  │  │ Symbols │  │ Search  │  │   DB    │  │ Import Graph  │    │  │
│  │  │ Parser  │  │ FTS5 +  │  │ SQLite  │  │ PageRank,SCC  │    │  │
│  │  │ Ranking │  │ Scoring │  │ Schema  │  │ Dead Code     │    │  │
│  │  │ Reader  │  │ Hybrid  │  │ + vec   │  │ Call Graph    │    │  │
│  │  │ Writer  │  │         │  │ v4 + heal│ │ Coupling      │    │  │
│  │  └────┬────┘  └────┬────┘  └────┬────┘  └──────┬────────┘    │  │
│  │       │            │            │               │             │  │
│  │  ┌────▼────────────▼────────────▼───────────────▼────────┐    │  │
│  │  │              Foundation Layer                          │    │  │
│  │  │  tree-sitter registry  │ LSP pool (opt-in)            │    │  │
│  │  │  Lang Registry         │ Scope Analysis               │    │  │
│  │  │  Lang Config           │ File Watcher (notify)        │    │  │
│  │  │  VFS / Project Root    │ Embedding (MiniLM + fastembed)│   │  │
│  │  └───────────────────────────────────────────────────────┘    │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

CodeLens is not the interactive agent runtime.
It is the bounded MCP tool that optimizes harnesses by compressing context, producing verifier evidence, and reusing heavy analysis through handles and stored artifacts.

Use repo-local contracts alongside this document:

- `EVAL_CONTRACT.md`
- `docs/platform-setup.md`
- `docs/archive/REFERENCE-2026-03.md`
- Project `CLAUDE.md` (harness routing + mutation gates)
- Project `AGENTS.md`

## 2. Project Directory Structure

```
codelens-mcp-plugin/
├── Cargo.toml                            # 3-crate workspace; shared dependency and feature policy
├── README.md                             # Public install and operating guide
├── AGENTS.md / CLAUDE.md                 # Repo-local agent contracts
├── EVAL_CONTRACT.md                      # Local and CI verification contract
├── install.sh                            # Release/installer entrypoint
│
├── crates/
│   ├── codelens-engine/                  # Code intelligence engine, no MCP protocol policy
│   │   ├── benches/ and tests/           # Engine performance and integration gates
│   │   └── src/
│   │       ├── lang_registry.rs          # 30 parser families, 49 extensions
│   │       ├── lang_config.rs            # tree-sitter language/query registry
│   │       ├── symbols/                  # SymbolIndex extraction, reader/writer, ranking
│   │       ├── db/                       # SQLite + FTS5 + sqlite-vec schema and operations
│   │       ├── import_graph/             # Import graph, resolvers, dead-code passes
│   │       ├── lsp/                      # Optional LSP authority and recipes
│   │       ├── call_graph.rs             # Multi-language call graph extraction
│   │       ├── redundant_definitions.rs  # Duplicate/redundancy analysis
│   │       ├── phantom_modules.rs        # Phantom module detection
│   │       ├── oxc_analysis.rs           # JS/TS OXC-assisted analysis
│   │       ├── scip_backend.rs           # Optional SCIP-backed precise navigation
│   │       ├── embedding_types.rs        # Embedding configuration and runtime types
│   │       ├── embedding_store.rs        # sqlite-vec storage for semantic lanes
│   │       └── edit_transaction.rs       # Transactional edit staging primitive
│   │
│   ├── codelens-mcp/                     # MCP server, transport, tool surface, policy gates
│   │   ├── tools.toml                    # Declarative tool surface source
│   │   ├── build.rs                      # Generated surface/schema inputs
│   │   └── src/
│   │       ├── main.rs / cli/            # CLI, host attach/doctor/status, manifest printing
│   │       ├── server/                   # stdio, Streamable HTTP, routing, auth/session support
│   │       ├── dispatch/                 # Access checks, rate limit, response envelope, handler table
│   │       ├── state/ and state.rs       # AppState assembly, project/session/runtime services
│   │       ├── tool_defs/                # Generated tool constructors, schemas, presets/profiles
│   │       ├── tools/                    # File, symbol, graph, mutation, report, rule, session handlers
│   │       ├── surface_manifest/         # Runtime/docs manifest builders and host adapters
│   │       ├── telemetry/                # In-memory metrics plus JSONL writer support
│   │       ├── resources.rs              # MCP resources, including generated surface resources
│   │       ├── surface_audit.rs          # Tool-surface consistency and visibility audits
│   │       ├── orphan_handlers.rs        # Handler/registry consistency checks
│   │       └── integration_tests/        # Protocol, mutation, readonly, semantic, workflow suites
│   │
│   └── codelens-tui/                     # Lightweight terminal UI crate
│
├── docs/                                 # Human docs plus generated manifest blocks
├── benchmarks/                           # Token, retrieval, daemon, call-graph, eval gates
├── scripts/                              # Release, generated-doc, benchmark, install helpers
├── models/                               # Source-tree model payloads for semantic/source builds
└── .github/workflows/                    # CI, benchmark, release, publishing gates
```

---

## 3. Data Flow

```
                 ┌─────────────────────┐
                 │   AI Agent Request  │
                 │ "find dispatch_tool │
                 │   function"         │
                 └──────────┬──────────┘
                            │
              ┌─────────────▼─────────────┐
              │     Transport Layer       │
              │  stdio │ HTTP+SSE │ CLI   │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │     server/router.rs      │
              │  initialize / tools/list  │
              │  tools/call → dispatch    │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │      dispatch/mod.rs      │
              │  ┌─────────────────────┐  │
              │  │ schema pre-validate │  │  ← MissingParam fast-fail
              │  │ _profile override   │  │  ← token budget control
              │  │ preflight gate      │  │  ← mutation_gate checks
              │  │ DISPATCH_TABLE      │  │  ← handler lookup
              │  │ telemetry record    │  │  ← latency tracking
              │  │ doom-loop detect    │  │  ← recent_buffer
              │  │ response envelope   │  │  ← dispatch/response.rs + _meta
              │  └─────────────────────┘  │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │      Tool Handler         │
              │  tools/symbols.rs         │
              │  tools/lsp.rs             │
              │  tools/graph.rs           │
              │  tools/mutation.rs        │
              │  tools/reports/*.rs       │
              └─────────────┬─────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
   ┌────▼────┐         ┌────▼────┐        ┌────▼────┐
   │SymbolIdx│         │ImportGr │        │   LSP   │
   │ SQLite  │         │petgraph │        │  (opt)  │
   │  FTS5   │         │PageRank │        │         │
   └────┬────┘         └────┬────┘        └────┬────┘
        │                   │                   │
   ┌────▼───────────────────▼───────────────────▼────┐
   │        tree-sitter (30 families / 49 extensions)│
   │         Statically linked, zero-config          │
   │         Error recovery, millisecond parsing     │
   └──────────────────────────────────────────────────┘
```

---

## 4. Tool Ecosystem (Current Generated Surface)

The live tool surface is generated from [`crates/codelens-mcp/tools.toml`](../crates/codelens-mcp/tools.toml), emitted through `tool_defs/generated`, and summarized in [`docs/generated/surface-manifest.json`](generated/surface-manifest.json). Do not hand-edit counts in prose; run `python3 scripts/surface-manifest.py --write` after changing tool definitions, profiles, language support, or host-adapter manifests.

### Preset Distribution

| Preset     | Tools | Intended use                                      |
| ---------- | ----: | ------------------------------------------------- |
| `minimal`  |    27 | Small visible surface for simple local sessions   |
| `balanced` |    82 | Default bounded surface for most agent workflows  |
| `full`     |   111 | Debugging, audits, and explicit broad inspection  |

### Namespace Distribution

| Namespace    | Tools | Main responsibility                                       |
| ------------ | ----: | --------------------------------------------------------- |
| `filesystem` |     6 | bounded file/search/test discovery                        |
| `symbols`    |     9 | symbol lookup, ranked context, references, type hierarchy |
| `lsp`        |     3 | diagnostics, recipe, and LSP health                       |
| `graph`      |     8 | changed files, call graph, coupling, complexity           |
| `mutation`   |    16 | preflight-gated edit primitives and refactors             |
| `reports`    |    25 | workflow reports, async analysis jobs, review/readiness   |
| `session`    |    39 | project activation, coordination, audits, capability state |
| `memory`     |     5 | project memory CRUD                                       |

### Tier Distribution

| Tier        | Tools | Interpretation                                      |
| ----------- | ----: | --------------------------------------------------- |
| `primitive` |    53 | direct local lookup/edit operations                 |
| `analysis`  |    28 | structured reports, audits, and diagnostic helpers  |
| `workflow`  |    30 | higher-level harness entrypoints and composed flows |

### Profile Surface

| Profile             | Tools | Status     | Notes                                      |
| ------------------- | ----: | ---------- | ------------------------------------------ |
| `planner-readonly`  |    32 | active     | planning and context collection            |
| `builder-minimal`   |    36 | active     | bounded editing surface                    |
| `reviewer-graph`   |    36 | active     | read-only review and graph evidence        |
| `evaluator-compact` |    14 | deprecated | kept during the v1.x compatibility window  |
| `refactor-full`     |    50 | deprecated | use only for explicit mutation-heavy paths |
| `ci-audit`          |    43 | deprecated | CI/audit compatibility surface             |
| `workflow-first`    |    19 | deprecated | migration compatibility surface            |

### Output Schemas

- Current source-declared schema coverage is `77 / 111`.
- All read handles (`analysis_handle`), mutation results, and primary symbol/reference payloads are schema-typed.
- Response annotations include `_meta["anthropic/maxResultSizeChars"]` per MCP v2.1.91+.
- The product direction is profile-first: docs and host adapters should advertise compact workflow profiles before the full 111-tool surface.

---

## 5. Language Support

<!-- SURFACE_MANIFEST_ARCHITECTURE_LANGUAGES:BEGIN -->

Canonical parser families (30): C, Clojure/ClojureScript, C++, C#, CSS, Dart, Erlang, Elixir, Go, Haskell, HTML, Java, Julia, JavaScript, Kotlin, Lua, OCaml, PHP, Python, R, Ruby, Rust, Scala, Bash/Shell, Swift, TOML, TypeScript, TSX/JSX, YAML, Zig

Import-graph capable families: C, C++, C#, CSS, Dart, Go, Java, JavaScript, Kotlin, PHP, Python, Ruby, Rust, Scala, Swift, TypeScript, TSX/JSX

The canonical family/extension inventory is generated from `codelens_engine::lang_registry` and published in [`docs/generated/surface-manifest.json`](generated/surface-manifest.json).

<!-- SURFACE_MANIFEST_ARCHITECTURE_LANGUAGES:END -->

---

## 6. Core Design Principles

```
┌─────────────────────────────────────────────────────────────┐
│                  tree-sitter-first                           │
│                                                              │
│  "MCP 도구의 소비자는 IDE 사용자가 아니라 AI 에이전트"        │
│                                                              │
│  ┌──────────────────┐      ┌──────────────────┐             │
│  │  tree-sitter     │      │  LSP (opt-in)    │             │
│  │  ✓ 0ms 시작      │      │  ✗ 2-30s 콜드    │             │
│  │  ✓ 제로 설정     │      │  ✗ 서버 설치     │             │
│  │  ✓ 30개 family   │      │  ✗ 설정 필요     │             │
│  │  ✓ 에러 복구     │      │  ✗ 빌드 실패시   │             │
│  │  ✓ 결정적        │      │    무응답        │             │
│  │  DEFAULT ←───────┤      │  use_lsp=true    │             │
│  └──────────────────┘      └──────────────────┘             │
│                                                              │
│  에이전트 우선순위: 속도 > 가용성 > 안정성 > 정밀도          │
└─────────────────────────────────────────────────────────────┘
```

### Bounded Answer Principle

CodeLens is no longer a "more tools" MCP — it is a **bounded-answer MCP**.

- Workflow tools return pre-synthesized reports (impact_report, refactor_safety_report, module_boundary_report)
- Analysis handles let agents expand only one section at a time (`get_analysis_section`)
- Durable analysis jobs (`start_analysis_job` → `get_analysis_job`) keep heavy reports out of the response envelope
- Role profiles + token budget (`_profile`) cap response size before serialization
- Adaptive compression (5-stage OpenDev) kicks in when budget usage approaches 100%

### Mutation Gate Protocol

All mutation tools are gated:

1. `verify_change_readiness` (or `safe_rename_report` for rename) must return `mutation_ready: "ready"` or `"caution"`
2. `preflight_store` caches readiness with TTL (override via `CODELENS_PREFLIGHT_TTL_SECS`)
3. Mutation audit log at `.codelens/audit/mutation-audit.jsonl`
4. Post-mutation `suggested_next_tools` always includes `get_file_diagnostics`

---

## 7. Protocol Stack & Future Readiness

```
┌─────────────────────────────────────────────────────────┐
│              2026 Agentic Architecture                  │
│                                                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Agent SDK (Claude/OpenAI/LangGraph/ADK)           │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                              │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │ A2A Protocol (Agent ↔ Agent)           [future]   │  │
│  │ Agent Cards, Task lifecycle, Discovery            │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                              │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │ MCP Protocol (Agent ↔ Tool)                       │  │
│  │ JSON-RPC 2.0, stdio/HTTP+SSE                      │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                              │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │ CodeLens MCP Server                               │  │
│  │                                                   │  │
│  │  ✅ Streamable HTTP + SSE                         │  │
│  │  ✅ Tool Annotations (readOnly/destructive)       │  │
│  │  ✅ Tool Output Schemas (generated manifest)      │  │
│  │  ✅ Preset + Role Profile subsetting              │  │
│  │  ✅ Token budget control (_profile)               │  │
│  │  ✅ Adaptive compression (OpenDev 5-stage)        │  │
│  │  ✅ Session management (UUID, TTL)                │  │
│  │  ✅ .well-known/mcp.json Server Card              │  │
│  │  ✅ Deferred tool loading (bootstrap-aware)       │  │
│  │  ✅ In-memory telemetry (per-tool metrics)        │  │
│  │  ✅ suggest_next_tools (contextual chaining)      │  │
│  │  ✅ v2.1.91+ `_meta` annotation                   │  │
│  │  ✅ Doom-loop detection (rapid-burst → async)     │  │
│  │  ⬜ Persistent telemetry (JSONL, planned)         │  │
│  │  ⬜ Stateless session tokens (spec pending)       │  │
│  │  ⬜ A2A Agent Card (long-term)                    │  │
│  └───────────────────────────────────────────────────┘  │
│                                                         │
│  AAIF (Linux Foundation) — 146 member organizations     │
│  Anthropic, Google, OpenAI, Microsoft, AWS              │
└─────────────────────────────────────────────────────────┘
```

---

## 8. Historical Metrics Snapshot (2026-04-11)

These metrics are a historical benchmark snapshot, not the canonical current-state inventory.
Use the **Current Snapshot** above and `docs/benchmarks.md` for current measurements.

| Metric                            | Value                                                                                  |
| --------------------------------- | -------------------------------------------------------------------------------------- |
| Total LOC                         | historical: 46,045 (38,820 prod + 7,225 test)                                          |
| Rust source files                 | historical: 115                                                                        |
| Tools (FULL / BALANCED / MINIMAL) | current: 111 / 82 / 27; historical 2026-04-11: 89 / 55 / 20                            |
| Tool categories (base)            | current namespaces: filesystem 6 · symbols 9 · lsp 3 · graph 8 · mutation 16 · reports 25 · session 39 · memory 5 |
| Semantic tools (cfg-gated)        | feature-gated semantic lanes are included only when the binary is built with `semantic` |
| Output schemas                    | current: `77 / 111` source-declared schemas                                            |
| Languages                         | current: 30 parser families across 49 extensions                                       |
| Tests                             | historical snapshot, superseded by current gate totals                                 |
| Clippy                            | 0 warnings (default + http feature)                                                    |
| DB schema version                 | v4 (FTS5 + sqlite-vec + self-heal)                                                     |
| tree-sitter grammars              | current: 30 parser families from `codelens_engine::lang_registry`                      |
| LSP recipes                       | 22 servers                                                                             |
| Ranking signals                   | 4 (text + pagerank + recency + semantic)                                               |
| Import resolvers                  | 11 languages (tsconfig.json, go.mod, src/)                                             |
| Transport                         | stdio, Streamable HTTP + SSE, CLI oneshot                                              |
| Preset/Profile budgets            | planner / builder / reviewer / refactor / ci-audit                                     |
| Binary size / model payload       | channel-dependent; crates.io default excludes the ONNX sidecar, release bundles include it |

### Performance Snapshot

| Operation            | Latency                           | Source                      |
| -------------------- | --------------------------------- | --------------------------- |
| find_symbol          | <1ms                              | SQLite FTS5                 |
| get_symbols_overview | <1ms                              | Cached                      |
| get_ranked_context   | ~135ms (hybrid) / ~39ms (lexical) | self regression benchmark   |
| get_impact_analysis  | ~1ms                              | Graph cache                 |
| semantic_search      | ~507ms                            | self regression benchmark   |
| Project onboard      | ~21ms                             | benchmarks/token-efficiency |
| Cold start           | ~12ms                             | No LSP boot                 |

### Quality Snapshot (Current Regression Tiers, 2026-04-17 v1.9.36 re-measurement)

| Dataset                              | Semantic MRR | Lexical MRR | Hybrid MRR | Notes                                                                                               |
| ------------------------------------ | -----------: | ----------: | ---------: | --------------------------------------------------------------------------------------------------- |
| Self regression (`104` queries)      |        0.647 |       0.532 |  **0.681** | Ground truth re-anchored onto dispatch decomposition in v1.9.34 — strictly harder than v1.9.12 tier |
| Role regression (`70` queries)       |        0.783 |       0.757 |  **0.814** | Workflow-style phrasing and implementation ownership queries                                        |
| External smoke (2 repos, 24 queries) |        0.847 |       0.528 |  **0.896** | Mixed TS/Rust via `external-retrieval-dataset.json`; semantic + hybrid both clearly lead lexical    |

- Hybrid remains the default product path because it outperforms lexical by **+0.15 MRR** on self, **+0.06** on role, and **+0.37** on external — the spread widens as queries move toward natural language.
- The earlier 2026-04-12 v1.9.12 snapshot (Self 0.841, Role 0.962, Flask 0.563, curl 0.623) remains in `docs/benchmarks.md §8 Historical Snapshots` as an experiment-log reference; the tighter ground truth introduced in v1.9.34 makes the current baseline a stricter test, not a regression (see `docs/design/retrieval-regression-bisect-2026-04-17.md`).

### Token Efficiency Snapshot (vs Read/Grep, tiktoken cl100k_base)

| Task               | Baseline | CodeLens | Savings        |
| ------------------ | -------- | -------- | -------------- |
| Find symbol        | 616      | 309      | 2.0x           |
| File structure     | 5,988    | 1,612    | 3.7x           |
| Impact analysis    | 5,321    | 1,651    | 3.2x           |
| Find references    | 616      | 240      | 2.6x           |
| Project onboarding | 7,972    | 763      | 10.4x          |
| Context retrieval  | 7,692    | 46       | **167.2x**     |
| **Total**          | 28,205   | 4,621    | **6.1x (84%)** |

Workflow profile compression (Balanced vs Profile):

- Planner change request: 15.6x
- Reviewer impact analysis: 16.3x
- Refactor safety: 4.4x

Re-run: `python3 benchmarks/token-efficiency.py <project>`
