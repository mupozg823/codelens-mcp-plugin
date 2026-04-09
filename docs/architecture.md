# CodeLens MCP — Architecture & Project Overview

> Pure Rust MCP server and harness optimization tool for code intelligence
> Default release registry: 88 tools | 25 languages | tree-sitter-first | ~44K Rust LOC

---

## 1. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                 Agent Runtime / Harness Layer                        │
│  Claude Code / OpenAI Agents / LangGraph / Custom Agent SDK         │
├───────────────────────┬─────────────────────────────────────────────┤
│    A2A (future)       │              MCP Protocol                   │
│  Agent ↔ Agent        │  JSON-RPC 2.0 over stdio / Streamable HTTP │
├───────────────────────┴─────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │          codelens-mcp (Harness Optimization Server)          │  │
│  │                                                               │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐ │  │
│  │  │ Dispatch │→ │  Tools   │→ │  State   │→ │  Telemetry   │ │  │
│  │  │  Table   │  │ (88 기본)│  │ AppState │  │  Metrics     │ │  │
│  │  └──────────┘  └────┬─────┘  └──────────┘  └──────────────┘ │  │
│  │                     │                                         │  │
│  │  ┌─────────────────────────────────────────────────────────┐ │  │
│  │  │              Tool Categories                             │ │  │
│  │  │  Symbol │ Edit │ Analysis │ File │ Session              │ │  │
│  │  │  Memory │ Semantic │ Workflow/Report │ Deferred Loading  │ │  │
│  │  └─────────────────────────────────────────────────────────┘ │  │
│  └───────────────────────────────┬───────────────────────────────┘  │
│                                  │                                   │
│  ┌───────────────────────────────▼───────────────────────────────┐  │
│  │                   codelens-core (Engine)                       │  │
│  │                                                               │  │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌───────────────┐  │  │
│  │  │ Symbols │  │ Search   │  │   DB    │  │ Import Graph  │  │  │
│  │  │ Parser  │  │ FTS5 +   │  │ SQLite  │  │ PageRank,SCC  │  │  │
│  │  │ Ranking │  │ Hybrid   │  │ Schema  │  │ Dead Code     │  │  │
│  │  │ Reader  │  │ Merge    │  │  v6     │  │ Call Graph    │  │  │
│  │  │ Writer  │  │          │  │         │  │ Coupling      │  │  │
│  │  └────┬────┘  └────┬────┘  └────┬────┘  └──────┬────────┘  │  │
│  │       │            │            │               │            │  │
│  │  ┌────▼────────────▼────────────▼───────────────▼────────┐  │  │
│  │  │              Foundation Layer                          │  │  │
│  │  │  tree-sitter (25 lang) │ LSP pool (opt-in)           │  │  │
│  │  │  Lang Registry         │ Scope Analysis              │  │  │
│  │  │  Lang Config           │ File Watcher (notify)       │  │  │
│  │  │  VFS / Project Root    │ Embedding (bundled/opt-in)  │  │  │
│  │  └───────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

CodeLens is not the interactive agent runtime.
It is the bounded MCP tool that optimizes harnesses by compressing context, producing verifier evidence, and reusing heavy analysis through handles and stored artifacts.

Use repo-local contracts alongside this document:

- `AGENTS.md`
- `EVAL_CONTRACT.md`
- `CLAUDE.md`
- `docs/platform-setup.md`
- `benchmarks/harness/README.md`

## 2. Project Directory Structure

```
codelens-mcp-plugin/
├── Cargo.toml                          # Workspace: 2 crates, tree-sitter-backed engine + MCP server
├── CLAUDE.md                           # AI agent instructions
├── README.md                           # Project documentation
├── install.sh                          # Installation script
│
├── crates/
│   ├── codelens-core/                  # Engine (~21K Rust LOC)
│   │   ├── Cargo.toml                  # deps: tree-sitter x25, rusqlite, rayon
│   │   ├── benches/indexing.rs         # Performance benchmarks
│   │   ├── tests/
│   │   │   ├── rename_real.rs          # Real-world rename validation
│   │   │   ├── rename_vs_grep.rs       # Rename vs grep comparison
│   │   │   └── snapshot_golden.rs      # Golden snapshot tests
│   │   └── src/
│   │       ├── lib.rs                  # Public API surface
│   │       ├── project.rs              # ProjectRoot, framework detection
│   │       │
│   │       ├── lang_registry.rs        # 25 languages: ext → canonical → config
│   │       ├── lang_config.rs          # tree-sitter Language + Query per lang
│   │       │
│   │       ├── symbols/                # Symbol extraction & ranking
│   │       │   ├── mod.rs              # SymbolIndex — central API
│   │       │   ├── parser.rs           # tree-sitter query execution
│   │       │   ├── writer.rs           # Index builder (refresh_all)
│   │       │   ├── reader.rs           # Symbol queries (find/overview)
│   │       │   ├── ranking.rs          # 4-signal ranking engine
│   │       │   ├── scoring.rs          # Score computation
│   │       │   ├── types.rs            # SymbolInfo, SymbolKind, etc.
│   │       │   └── tests.rs            # Symbol subsystem tests
│   │       │
│   │       ├── db/                     # SQLite + FTS5
│   │       │   ├── mod.rs              # IndexDb — schema v6, migrations
│   │       │   ├── ops.rs              # CRUD operations
│   │       │   └── tests.rs            # DB tests
│   │       │
│   │       ├── search.rs              # Hybrid search: FTS5 + jaro_winkler
│   │       │
│   │       ├── import_graph/          # Dependency analysis
│   │       │   ├── mod.rs             # Graph builder (petgraph)
│   │       │   ├── parsers.rs         # Import statement parsing
│   │       │   ├── resolvers.rs       # Path resolution per language
│   │       │   └── dead_code.rs       # Multi-pass dead code detection
│   │       │
│   │       ├── lsp/                   # LSP integration (opt-in)
│   │       │   ├── mod.rs             # LspSessionPool
│   │       │   ├── session.rs         # Single LSP session lifecycle
│   │       │   ├── protocol.rs        # JSON-RPC for LSP
│   │       │   ├── parsers.rs         # LSP response parsing
│   │       │   ├── registry.rs        # 22 LSP recipes (install hints)
│   │       │   ├── types.rs           # Request/response types
│   │       │   └── tests.rs           # LSP tests
│   │       │
│   │       ├── file_ops/              # File operations
│   │       │   ├── mod.rs             # Text reference search
│   │       │   ├── reader.rs          # File reading utilities
│   │       │   └── writer.rs          # File mutation (replace, insert)
│   │       │
│   │       ├── scope_analysis.rs      # def/read/write/import classification
│   │       ├── call_graph.rs          # Function call graph (7 languages)
│   │       ├── circular.rs            # Tarjan SCC cycle detection
│   │       ├── coupling.rs            # Git temporal coupling
│   │       ├── type_hierarchy.rs      # Native inheritance analysis
│   │       ├── rename.rs              # Multi-file rename engine
│   │       ├── auto_import.rs         # Missing import detection
│   │       ├── git.rs                 # Git diff/changed files
│   │       ├── embedding.rs           # bundled CodeSearchNet + optional fastembed indexing
│   │       ├── embedding_store.rs     # sqlite-vec storage
│   │       ├── vfs.rs                 # Virtual filesystem normalization
│   │       └── watcher.rs             # File watcher (notify + debounce)
│   │
│   └── codelens-mcp/                  # MCP Server (~22K Rust LOC)
│       ├── Cargo.toml                 # deps: axum, tokio, serde_json
│       └── src/
│           ├── main.rs                # Entry: CLI args, transport selection
│           ├── state.rs               # AppState + project switching + runtime state
│           ├── dispatch.rs            # Dispatcher: routing, budgets, gate checks, telemetry
│           ├── protocol.rs            # Tool, ToolAnnotations, OutputSchema
│           ├── tool_defs/             # build.rs, presets.rs, output_schemas.rs
│           ├── error.rs               # CodeLensError enum
│           ├── authority.rs           # Backend metadata helpers
│           ├── telemetry.rs           # ToolMetricsRegistry
│           ├── prompts.rs             # MCP prompt templates
│           ├── resources.rs           # MCP resource endpoints
│           ├── integration_tests.rs   # Deferred loading + client contract integration tests
│           │
│           ├── server/                # Transport layer
│           │   ├── mod.rs             # Server module exports
│           │   ├── router.rs          # JSON-RPC method routing
│           │   ├── transport_stdio.rs # stdio transport
│           │   ├── transport_http.rs  # Streamable HTTP + SSE + Server Card
│           │   ├── session.rs         # HTTP session management (UUID, TTL)
│           │   ├── oneshot.rs         # CLI one-shot mode
│           │   └── http_tests.rs      # HTTP transport tests
│           │
│           └── tools/                 # Tool implementations
│               ├── mod.rs             # Dispatch table + suggest_next_tools
│               ├── symbols.rs         # Symbol lookup tools
│               ├── lsp.rs             # LSP tools — tree-sitter-first fallback model
│               ├── graph.rs           # Analysis tools
│               ├── filesystem.rs      # File I/O tools
│               ├── mutation.rs        # Code editing tools
│               ├── memory.rs          # Project memory tools
│               └── session.rs         # Session/config + onboarding/report tools
│
├── skills/                            # Claude Code skills
│   ├── code-review/SKILL.md           # /codelens-review
│   ├── onboard/SKILL.md               # /codelens-onboard
│   └── analyze/SKILL.md               # /codelens-analyze
│
├── agents/
│   └── codelens-explorer.md           # Read-only code exploration agent
│
├── hooks/
│   └── post-edit-diagnostics.sh       # Auto-diagnose after file edits
│
├── benchmarks/
│   ├── bench.sh                       # CLI benchmark runner
│   └── README.md                      # Benchmark documentation
│
└── .claude/
    ├── settings.local.json            # Claude Code settings
    ├── agents/codelens-explorer.md    # Agent definition
    └── skills/lsp-setup.md            # LSP setup skill
```

---

## 3. Data Flow

```
                 ┌─────────────────────┐
                 │   AI Agent Request   │
                 │  "find dispatch_tool │
                 │   function"          │
                 └──────────┬──────────┘
                            │
              ┌─────────────▼─────────────┐
              │     Transport Layer        │
              │  stdio │ HTTP+SSE │ CLI    │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │     router.rs              │
              │  initialize / tools/list   │
              │  tools/call → dispatch     │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │     dispatch.rs            │
              │  ┌─────────────────────┐   │
              │  │ _profile override   │   │  ← token budget control
              │  │ DISPATCH_TABLE      │   │  ← handler lookup
              │  │ telemetry record    │   │  ← latency tracking
              │  │ response envelope   │   │  ← truncation safety net
              │  └─────────────────────┘   │
              └─────────────┬─────────────┘
                            │
              ┌─────────────▼─────────────┐
              │     Tool Handler           │
              │  symbols.rs / lsp.rs /     │
              │  graph.rs / mutation.rs    │
              └─────────────┬─────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
   ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
   │SymbolIdx│        │ ImportGr│        │   LSP   │
   │ SQLite  │        │ petgraph│        │  (opt)  │
   │  FTS5   │        │PageRank │        │         │
   └────┬────┘        └────┬────┘        └────┬────┘
        │                   │                   │
   ┌────▼───────────────────▼───────────────────▼────┐
   │              tree-sitter (25 languages)          │
   │         Statically linked, zero-config           │
   │         Error recovery, millisecond parsing      │
   └──────────────────────────────────────────────────┘
```

---

## 4. Tool Ecosystem

### Default Registry Snapshot

```
FULL (88)        ████████████████████████████████████████████████ 100%
BALANCED (60)    █████████████████████████████████                68%
MINIMAL (22)     ████████████                                     25%
```

Counts above reflect the current source snapshot for the default semantic-enabled build.

### Preferred Profiles

The operational surface is now profile-first rather than preset-first.

```
planner-readonly   read-heavy planning, ranked context, references
builder-minimal    edit path with limited mutation surface
reviewer-graph     impact/review workflows, graph-heavy inspection
refactor-full      gated mutation tools + verifier workflows
ci-audit           bounded reports, readiness, impact, diagnostics
evaluator-compact  harness and benchmark-facing compact contract
```

### Categories

The registry is organized around:

- symbol lookup and ranked context
- file I/O and workspace inspection
- mutation and gated refactor flows
- analysis/report workflows
- project/session/runtime controls
- semantic indexing and hybrid retrieval

Deferred loading narrows the first exposed tool surface further for supported clients.

---

## 5. Language Support (25)

```
Phase 1-5 (Original 16):
  Python, JavaScript, TypeScript, TSX, Go, Java, Kotlin, Rust,
  C, C++, PHP, Swift, Scala, Ruby, C#, Dart

Phase 6a (Added 9):
  Lua, Zig, Elixir, Haskell, OCaml, Erlang, R, Bash, Julia

Deferred (tree-sitter 0.26 required):
  Perl

Each language has:
  ├── tree-sitter grammar (statically linked)
  ├── Symbol extraction query (lang_config.rs)
  ├── Extension mapping (lang_registry.rs)
  └── LSP recipe + command mapping (opt-in)
```

---

## 6. Core Design Principles

```
┌─────────────────────────────────────────────────────────────┐
│                  tree-sitter-first                           │
│                                                             │
│  "MCP 도구의 소비자는 IDE 사용자가 아니라 AI 에이전트"       │
│                                                             │
│  ┌──────────────────┐      ┌──────────────────┐            │
│  │  tree-sitter     │      │  LSP (opt-in)    │            │
│  │  ✓ 0ms 시작     │      │  ✗ 2-30s 콜드   │            │
│  │  ✓ 제로 설정    │      │  ✗ 서버 설치    │            │
│  │  ✓ 25개 내장    │      │  ✗ 설정 필요    │            │
│  │  ✓ 에러 복구    │      │  ✗ 빌드 실패시  │            │
│  │  ✓ 결정적      │      │    무응답        │            │
│  │  DEFAULT ←──────┤      │  use_lsp=true    │            │
│  └──────────────────┘      └──────────────────┘            │
│                                                             │
│  에이전트 우선순위: 속도 > 가용성 > 안정성 > 정밀도        │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. Protocol Stack & Future Readiness

```
┌─────────────────────────────────────────────────────────┐
│              2026 Agentic Architecture                   │
│                                                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Agent SDK (Claude/OpenAI/LangGraph/ADK)           │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                               │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │ A2A Protocol (Agent ↔ Agent)           [future]   │  │
│  │ Agent Cards, Task lifecycle, Discovery             │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                               │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │ MCP Protocol (Agent ↔ Tool)                        │  │
│  │ JSON-RPC 2.0, stdio/HTTP+SSE                       │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                               │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │ CodeLens MCP Server                                │  │
│  │                                                    │  │
│  │  ✅ Streamable HTTP + SSE                          │  │
│  │  ✅ Tool Annotations (readOnly/destructive)        │  │
│  │  ✅ Tool Output Schemas (7 core tools)             │  │
│  │  ✅ Preset-based capability subsetting             │  │
│  │  ✅ Token budget control (_profile)                │  │
│  │  ✅ Session management (UUID, TTL)                 │  │
│  │  ✅ .well-known/mcp.json Server Card               │  │
│  │  ✅ Telemetry (per-tool metrics)                   │  │
│  │  ✅ suggest_next_tools (contextual chaining)       │  │
│  │  ⬜ Stateless session tokens (spec pending)        │  │
│  │  ⬜ A2A Agent Card (long-term)                     │  │
│  └────────────────────────────────────────────────────┘  │
│                                                         │
│  AAIF (Linux Foundation) — 146 member organizations     │
│  Anthropic, Google, OpenAI, Microsoft, AWS              │
└─────────────────────────────────────────────────────────┘
```

---

## 8. Key Metrics

| Metric                        | Value                                           |
| ----------------------------- | ----------------------------------------------- |
| Total LOC                     | ~22,500                                         |
| Rust source files             | 73                                              |
| Total symbols (self)          | 1,007                                           |
| Tools (Full/Balanced/Minimal) | 63 / 39 / 21                                    |
| Languages                     | 25 (+ Perl deferred)                            |
| Tests                         | 197 (core 156 + mcp 41)                         |
| DB schema version             | v4 (FTS5)                                       |
| tree-sitter grammars          | 25 (statically linked)                          |
| LSP recipes                   | 22 servers                                      |
| Ranking signals               | 4 (text + pagerank + recency + semantic)        |
| Import resolvers              | 11 languages (with tsconfig.json, go.mod, src/) |
| Transport                     | stdio, Streamable HTTP + SSE, CLI oneshot       |
| Binary size (release)         | Single binary, zero runtime deps                |
