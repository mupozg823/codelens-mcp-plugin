# CodeLens MCP — Architecture & Project Overview

> Pure Rust MCP server and harness optimization tool for code intelligence
> Harness optimization control plane with generated surface governance and tree-sitter-first retrieval

## Current Snapshot (2026-04-25)

<!-- SURFACE_MANIFEST_ARCHITECTURE_SNAPSHOT:BEGIN -->

- Workspace version: `1.9.60`
- Workspace members: `2` (`crates/codelens-engine`, `crates/codelens-mcp`)
- Registered tool definitions in source: `112`
- Tool output schemas in source: `82 / 112`
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
- `docs/REFERENCE-2026-03.md`
- Project `CLAUDE.md` (harness routing + mutation gates)
- Project `AGENTS.md`

## 2. Project Directory Structure

```
codelens-mcp-plugin/
├── Cargo.toml                            # Workspace: 2 crates, 25 tree-sitter deps
├── CLAUDE.md                             # AI agent instructions (harness routing)
├── AGENTS.md                             # Agent role contracts
├── EVAL_CONTRACT.md                      # Verification command contracts
├── README.md                             # Public documentation
├── install.sh                            # One-line installer
│
├── crates/
│   ├── codelens-engine/                  # Engine crate
│   │   ├── Cargo.toml                    # deps: tree-sitter x25, rusqlite, rayon, ort
│   │   ├── benches/                      # Performance benchmarks
│   │   ├── tests/                        # Integration suites
│   │   └── src/
│   │       ├── lib.rs                    # Public API surface
│   │       ├── project.rs                # ProjectRoot, framework detection
│   │       │
│   │       ├── lang_registry.rs          # 25 languages: ext → canonical → config
│   │       ├── lang_config.rs            # tree-sitter Language + Query per lang
│   │       │
│   │       ├── symbols/                  # Symbol extraction & ranking
│   │       │   ├── mod.rs                # SymbolIndex — central API
│   │       │   ├── parser.rs             # tree-sitter query execution
│   │       │   ├── writer.rs             # Index builder (refresh_all)
│   │       │   ├── reader.rs             # Symbol queries (find/overview)
│   │       │   ├── ranking.rs            # 4-signal ranking engine
│   │       │   └── scoring.rs            # Score computation
│   │       │
│   │       ├── db/                       # SQLite + FTS5 + sqlite-vec
│   │       │   ├── mod.rs                # IndexDb — schema v4, self-heal, migrations
│   │       │   ├── ops.rs                # CRUD operations (1K+ LOC)
│   │       │   └── tests.rs              # DB suite (incl. self-heal)
│   │       │
│   │       ├── search.rs                 # Hybrid search: FTS5 + jaro_winkler
│   │       │
│   │       ├── import_graph/             # Dependency analysis
│   │       │   ├── mod.rs                # Graph builder (petgraph)
│   │       │   ├── parsers.rs            # Import statement parsing
│   │       │   ├── resolvers.rs          # Path resolution per language
│   │       │   └── dead_code.rs          # Multi-pass dead code detection
│   │       │
│   │       ├── lsp/                      # LSP integration (opt-in)
│   │       │   ├── mod.rs                # LspSessionPool
│   │       │   ├── session.rs            # Single LSP session lifecycle
│   │       │   ├── protocol.rs           # JSON-RPC for LSP
│   │       │   └── registry.rs           # 22 LSP recipes
│   │       │
│   │       ├── file_ops/                 # File I/O + mutation writers
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs
│   │       │   └── writer.rs
│   │       │
│   │       ├── scope_analysis.rs         # def/read/write/import classification
│   │       ├── call_graph.rs             # Function call graph
│   │       ├── circular.rs               # Tarjan SCC cycle detection
│   │       ├── coupling.rs               # Git temporal coupling
│   │       ├── type_hierarchy.rs         # Native inheritance analysis
│   │       ├── rename.rs                 # Multi-file rename engine
│   │       ├── auto_import.rs            # Missing import detection
│   │       ├── change_signature.rs       # Refactoring: change signature
│   │       ├── inline.rs                 # Refactoring: inline function
│   │       ├── move_symbol.rs            # Refactoring: move to file
│   │       ├── git.rs                    # Git diff/changed files
│   │       ├── community.rs              # Community detection for clustering
│   │       ├── embedding.rs              # bundled MiniLM + optional fastembed (2.9K LOC)
│   │       ├── embedding_store.rs        # sqlite-vec storage
│   │       ├── memory.rs                 # Project memory store
│   │       ├── vfs.rs                    # Virtual filesystem normalization
│   │       ├── watcher.rs                # File watcher (notify + debounce)
│   │       └── oxc_analysis.rs           # JS/TS semantic analysis (oxc)
│   │
│   └── codelens-mcp/                     # MCP server crate
│       ├── Cargo.toml                    # deps: axum, tokio, serde_json
│       └── src/
│           ├── main.rs                   # Entry: CLI args, transport selection
│           ├── state.rs                  # AppState assembly + shared runtime helpers
│           ├── state/                    # Project/session/preflight/watcher services
│           ├── dispatch/                 # Dispatcher, access, response shaping
│           ├── protocol.rs               # Tool, ToolAnnotations, OutputSchema
│           ├── error.rs                  # CodeLensError enum
│           ├── telemetry.rs              # ToolMetricsRegistry (in-memory session)
│           ├── mutation_gate.rs          # verify_change_readiness enforcement
│           ├── mutation_audit.rs         # .codelens/audit/mutation-audit.jsonl
│           ├── preflight_store.rs        # Preflight TTL cache
│           ├── analysis_queue.rs         # Durable analysis job queue
│           ├── artifact_store.rs         # Analysis handle storage
│           ├── job_store.rs              # Job persistence
│           ├── session_context.rs        # Per-session state
│           ├── session_metrics_payload.rs
│           ├── recent_buffer.rs          # Doom-loop detection
│           ├── client_profile.rs         # Client identity heuristics
│           ├── authority.rs              # Backend metadata helpers
│           ├── resource_catalog.rs       # MCP resource registry
│           ├── resource_context.rs       # Profile-scoped resource access
│           ├── resource_profiles.rs
│           ├── resource_analysis.rs
│           ├── resources.rs              # MCP resource endpoints
│           ├── prompts.rs                # MCP prompt templates
│           ├── runtime_types.rs
│           ├── tool_runtime.rs
│           ├── test_helpers.rs           # Shared test utilities
│           │
│           ├── tool_defs/                # Tool registration
│           │   ├── mod.rs
│           │   ├── build.rs              # registered tool definitions (central registry)
│           │   ├── output_schemas.rs     # 45 output schemas
│           │   └── presets.rs            # FULL/BALANCED/MINIMAL + profiles
│           │
│           ├── server/                   # Transport layer
│           │   ├── mod.rs
│           │   ├── router.rs             # JSON-RPC method routing
│           │   ├── transport_stdio.rs    # stdio transport
│           │   ├── transport_http.rs     # Streamable HTTP + SSE + Server Card
│           │   ├── session.rs            # HTTP session management (UUID, TTL)
│           │   └── oneshot.rs            # CLI one-shot mode
│           │
│           └── tools/                    # Tool handler implementations
│               ├── mod.rs                # Dispatch table + suggest_next_tools
│               ├── symbols.rs            # Symbol lookup handlers
│               ├── workflows.rs          # Workflow-first alias layer for agent entrypoints
│               ├── lsp.rs                # LSP-backed handlers
│               ├── graph.rs              # Analysis graph handlers
│               ├── filesystem.rs         # File I/O handlers
│               ├── mutation.rs           # Code edit handlers
│               ├── memory.rs             # Memory handlers
│               ├── composite.rs          # Composite workflow handlers
│               ├── report_contract.rs    # Analysis-handle contract
│               ├── report_jobs.rs        # Job lifecycle handlers
│               ├── report_payload.rs     # Report shaping
│               ├── report_utils.rs
│               ├── report_verifier.rs    # Verifier-first mutation gate
│               ├── reports/              # Workflow report implementations
│               │   ├── context_reports.rs
│               │   ├── impact_reports.rs
│               │   └── verifier_reports.rs
│               └── session/              # Session-scoped handlers
│                   ├── metrics_config.rs
│                   └── project_ops.rs
│
├── docs/
│   ├── architecture.md                   # this file
│   ├── platform-setup.md                 # per-platform install/config
│   └── REFERENCE-2026-03.md              # architecture reference snapshot
│
├── benchmarks/
│   ├── token-efficiency.py               # MCP vs Read/Grep A/B (tiktoken cl100k_base)
│   ├── embedding-quality.py              # MRR / Acc@k
│   ├── embedding-runtime.py              # latency/throughput
│   ├── daemon-latency-gate.py            # daemon hot-path p95 gate
│   ├── embedding-quality-dataset-self.json  # 89 self-matching queries
│   └── *.json                            # result snapshots
│
├── models/                               # ONNX model assets, INT8
└── install.sh                            # Homebrew / one-line installer
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
   │              tree-sitter (25 languages)         │
   │         Statically linked, zero-config          │
   │         Error recovery, millisecond parsing     │
   └──────────────────────────────────────────────────┘
```

---

## 4. Tool Ecosystem (Historical Shape Reference)

This section is a broad shape reference for the product surface.
For the latest authoritative counts, use the **Current Snapshot** at the top of this file and the audit report in [docs/architecture-audit-2026-04-24.md](architecture-audit-2026-04-24.md).

### Preset Distribution

```
FULL     (89)   ████████████████████████████████████████████  100%
BALANCED (55)   ██████████████████████████████                 62%
MINIMAL  (20)   ██████████████                                 22%
```

### Tool Categories (counted from tool_defs/build.rs)

```
┌─────────────────────────────────────────────────────────────────┐
│                         89 Tools                                 │
├───────────────────┬─────────────────┬───────────────────────────┤
│ File (7)          │ Symbol (7)      │ LSP (7)                   │
│  read_file        │  get_symbols_   │  find_referencing_symbols │
│  list_dir         │   overview      │  get_file_diagnostics     │
│  find_file        │  find_symbol    │  search_workspace_symbols │
│  search_for_      │  get_ranked_    │  get_type_hierarchy       │
│   pattern         │   context       │  plan_symbol_rename       │
│  find_annotations │  search_symbols_│  check_lsp_status         │
│  find_tests       │   fuzzy         │  get_lsp_recipe           │
│  get_current_     │  get_complexity │                           │
│   config          │  get_project_   │                           │
│                   │   structure     │                           │
│                   │  refresh_symbol │                           │
│                   │   _index        │                           │
├───────────────────┼─────────────────┼───────────────────────────┤
│ Analysis (7)      │ Edit (17)       │ Workflow/Composite (17)   │
│  get_changed_     │  rename_symbol  │  onboard_project          │
│   files           │  replace_symbol │  analyze_change_request   │
│  get_impact_      │   _body         │  verify_change_readiness  │
│   analysis        │  replace_content│  find_minimal_context_    │
│  find_scoped_     │  replace_lines  │   for_change              │
│   references      │  delete_lines   │  summarize_symbol_impact  │
│  get_symbol_      │  insert_at_line │  module_boundary_report   │
│   importance      │  insert_before_ │  safe_rename_report       │
│  find_dead_code   │   symbol        │  unresolved_reference_    │
│  find_circular_   │  insert_after_  │   check                   │
│   dependencies    │   symbol        │  dead_code_report         │
│  get_change_      │  insert_content │  impact_report            │
│   coupling        │  replace        │  refactor_safety_report   │
│                   │  create_text_   │  diff_aware_references    │
│                   │   file          │  semantic_code_review     │
│                   │  analyze_       │  start_analysis_job       │
│                   │   missing_      │  get_analysis_job         │
│                   │   imports       │  cancel_analysis_job      │
│                   │  add_import     │  get_analysis_section     │
│                   │  refactor_      │                           │
│                   │   extract/      │                           │
│                   │   inline/       │                           │
│                   │   move_to_file/ │                           │
│                   │   change_       │                           │
│                   │   signature     │                           │
├───────────────────┼─────────────────┼───────────────────────────┤
│ Memory (5)        │ Session (16)    │ Semantic (6, cfg-gated)   │
│  list_memories    │  activate_      │  semantic_search          │
│  read_memory      │   project       │  index_embeddings         │
│  write_memory     │  prepare_       │  find_similar_code        │
│  delete_memory    │   harness_      │  find_code_duplicates     │
│  rename_memory    │   session       │  classify_symbol          │
│                   │  prepare_for_   │  find_misplaced_code      │
│                   │   new_          │                           │
│                   │   conversation  │                           │
│                   │  summarize_     │                           │
│                   │   changes       │                           │
│                   │  get_watch_     │                           │
│                   │   status        │                           │
│                   │  prune_index_   │                           │
│                   │   failures      │                           │
│                   │  add/remove/    │                           │
│                   │   query/list    │                           │
│                   │   _queryable_   │                           │
│                   │   project (4)   │                           │
│                   │  set_preset     │                           │
│                   │  set_profile    │                           │
│                   │  get_           │                           │
│                   │   capabilities  │                           │
│                   │  get_tool_      │                           │
│                   │   metrics       │                           │
│                   │  export_session │                           │
│                   │   _markdown     │                           │
│                   │  summarize_file │                           │
└───────────────────┴─────────────────┴───────────────────────────┘
```

### Output Schemas

- Output schema coverage is generated from the surface manifest snapshot above
- All read handles (`analysis_handle`), mutation results, and primary symbol/reference payloads are schema-typed
- Response annotations include `_meta["anthropic/maxResultSizeChars"]` per MCP v2.1.91+

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
│  │  ✓ 25개 내장     │      │  ✗ 설정 필요     │             │
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
| Total LOC                         | 46,045 (38,820 prod + 7,225 test)                                                      |
| Rust source files                 | 115                                                                                    |
| Tools (FULL / BALANCED / MINIMAL) | 89 / 55 / 20                                                                           |
| Tool categories (base)            | File 7 · Symbol 7 · LSP 7 · Analysis 7 · Edit 17 · Workflow 17 · Memory 5 · Session 16 |
| Semantic tools (cfg-gated)        | 6                                                                                      |
| Output schemas                    | historical snapshot, superseded by current `65 / 101` snapshot above                   |
| Languages                         | 25 (+ Perl deferred)                                                                   |
| Tests                             | historical snapshot, superseded by current gate totals                                 |
| Clippy                            | 0 warnings (default + http feature)                                                    |
| DB schema version                 | v4 (FTS5 + sqlite-vec + self-heal)                                                     |
| tree-sitter grammars              | 25 (statically linked)                                                                 |
| LSP recipes                       | 22 servers                                                                             |
| Ranking signals                   | 4 (text + pagerank + recency + semantic)                                               |
| Import resolvers                  | 11 languages (tsconfig.json, go.mod, src/)                                             |
| Transport                         | stdio, Streamable HTTP + SSE, CLI oneshot                                              |
| Preset/Profile budgets            | planner / builder / reviewer / refactor / ci-audit                                     |
| Binary size (release, default)    | ~76 MB (bundled ONNX embedding model)                                                  |
| Binary size (release, minimal)    | ~23 MB (`--no-default-features`)                                                       |

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
