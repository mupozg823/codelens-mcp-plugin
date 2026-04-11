# Changelog

All notable changes to **CodeLens MCP** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet — the next entry will appear here when work begins on v1.5.0.

## [1.4.0] — 2026-04-11

First public release cut. This version marks the transition from a
"more tools" MCP into a **bounded-answer, telemetry-aware, reviewer-ready**
code-intelligence server.

### Added

- **Telemetry persistence** — new append-only JSONL log at
  `.codelens/telemetry/tool_usage.jsonl`. Gated by
  `CODELENS_TELEMETRY_ENABLED=1` or `CODELENS_TELEMETRY_PATH=<path>`.
  Disabled by default. Graceful degradation: write failures are logged
  once and swallowed — telemetry never breaks dispatch.
- **`mermaid_module_graph` workflow tool** — renders upstream/downstream
  module dependencies as a Mermaid flowchart, ready to paste into
  GitHub/GitLab/VS Code Markdown. Reuses `get_impact_analysis` data;
  no new engine surface.
- **Reproducible public benchmarks doc** (`docs/benchmarks.md`) — every
  headline performance number is now backed by an executable script
  under `benchmarks/` and can be re-run on any machine. Includes
  token-efficiency (tiktoken cl100k_base), MRR/Accuracy@k, and per-
  operation latency.
- **Output schemas**: expanded from 31 → 45 of 89 tools (51% coverage),
  including 7 new schemas for mutation + semantic tools.
- **MCP v2.1.91+ compliance**:
  - `_meta["anthropic/maxResultSizeChars"]` response annotation
  - Deferred tool loading during `initialize`
  - Schema pre-validation (fail fast on missing required params)
  - Rapid-burst doom-loop detection (3+ identical calls within 10s →
    `start_analysis_job` suggestion)
- **Harness phase tracking** — telemetry timeline now records an
  optional `phase` field (plan/build/review/eval) per invocation.
- **Effort level** — `CODELENS_EFFORT_LEVEL=low|medium|high` adjusts
  adaptive compression thresholds and default token budget.
- **Self-healing SQLite indexes** — corrupted FTS5 / vec indexes are
  detected on open and rebuilt automatically without user intervention.
- **Project-scoped memory store** — `list_memories`, `read_memory`,
  `write_memory`, `delete_memory`, `rename_memory` tools for persistent
  architecture notes, RCA history, and kaizen logs.

### Changed

- **Crate rename**: `codelens-core` → `codelens-engine` to resolve a
  crates.io name collision. Workspace consumers should update their
  `Cargo.toml` dependency from `codelens-core` to `codelens-engine`.
  Binary name (`codelens-mcp`) unchanged.
- **Architecture docs** (`docs/architecture.md`) resynced from stale
  63-tool / 22K-LOC / 197-test snapshot to current
  90-tool / 46K-LOC / 547-test ground truth.
- **Tool surface**: 89 → 90 tools (FULL preset). BALANCED auto-includes
  new tools via the exclude-list pattern; MINIMAL intentionally stays
  at 20.

### Fixed

- **Clippy cleanup**: resolved 28 accumulated warnings across default
  and `http` features. `cargo clippy --all-targets -- -D warnings`
  is now clean on both feature sets.
- **Rename lookup fallback** hardened for LSP-absent flows.
- **Analysis state scope**: analysis queue state now scoped to
  session project — prevents cross-project contamination on HTTP
  transport.
- **HTTP session runtime state** isolated per session.

### Removed

- No public API removals.

### Migration notes

1. If your `Cargo.toml` depends on `codelens-core`, update it to
   `codelens-engine`. No API signatures changed — only the package name.
2. Binary name (`codelens-mcp`) and CLI surface are unchanged.
3. To opt into telemetry persistence, set
   `CODELENS_TELEMETRY_ENABLED=1` when launching the server and grep
   `.codelens/telemetry/tool_usage.jsonl` afterwards.
4. Mermaid diagrams produced by `mermaid_module_graph` embed directly
   in GitHub-flavored Markdown — no extra renderer needed.

### Metrics snapshot

Measured on this repository at the 1.4.0 cut:

| Metric                                 | Value                      |
| -------------------------------------- | -------------------------- |
| Tools (FULL / BALANCED / MINIMAL)      | 90 / 55 / 20               |
| Rust source files                      | 115                        |
| LOC (prod + test)                      | 46K (38.8K + 7.2K)         |
| Tests                                  | 547 (222 engine + 325 mcp) |
| Clippy warnings                        | 0 (default + http feature) |
| Token efficiency vs Read/Grep          | **6.1x (84%)**             |
| Workflow profile compression           | 15-16x (planner/reviewer)  |
| Hybrid retrieval MRR                   | **0.664** (self-dataset)   |
| Hybrid retrieval Acc@5                 | **0.775**                  |
| `find_symbol` / `get_symbols_overview` | < 1 ms                     |
| Cold start                             | ~ 12 ms                    |

See [`docs/benchmarks.md`](docs/benchmarks.md) for reproduce commands.

---

## Earlier history

Pre-1.4.0 work lives in git history on the `main` branch. Notable
milestones:

- **2026-03-28** — `feat: unified project & backend integration` (PR #1),
  `feat: Pure Rust MCP server — 54 tools, 15 languages, semantic search,
token budget` (PR #2)
- **2026-04-04** — `refactor: state.rs -33%, full green, Store
extraction` (PR #3)
- **2026-04-08** — `feat: semantic code review, structural search
boosting, cross-phase context` (PR #4)
- **2026-04-09** — `feat: essential main integration: rename, session
scope, report runtime, clean-clone tests` (PR #5),
  `feat: track MCP recommendation outcomes in Codex harness` (PR #6)
- **2026-04-11** — PR #7 (harness compliance + crate rename + telemetry
  persistence), PR #8 (benchmarks doc + mermaid_module_graph) → 1.4.0 cut

[Unreleased]: https://github.com/mupozg823/codelens-mcp-plugin/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.4.0
