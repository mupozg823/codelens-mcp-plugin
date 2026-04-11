# Changelog

All notable changes to **CodeLens MCP** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (v1.5 work-in-progress)

- **`embedding/vec_store.rs` submodule** — split `SqliteVecStore` + its `EmbeddingStore` impl out of `embedding.rs` (2,934 LOC → 2,501 + 451). Pure structural refactor, git rename-detected at 84% similarity. Phase 1 of the planned embedding-crate decomposition.
- **Embedding hint infrastructure** — new `join_hint_lines`, `hint_line_budget`, `hint_char_budget` helpers plus `CODELENS_EMBED_HINT_LINES` (1..=10) and `CODELENS_EMBED_HINT_CHARS` (60..=512) env overrides. Multi-line body hints separated by `·` when a future PoC needs more than one line. The defaults stay at 1 line / 60 chars (v1.4.0 parity) — see "Changed" below for the reasoning.
- **Dataset path fix** — `benchmarks/embedding-quality-dataset-self.json` rewritten from `crates/codelens-core/...` to `crates/codelens-engine/...` so `expected_file_suffix` actually matches real files after the v1.4.0 crate rename. Without this fix every NL query scored `rank=None` on current main.

### Changed

- **`extract_body_hint` refactor** — now goes through `join_hint_lines` and respects the runtime budgets above. Behaviour at default budgets is unchanged: still returns a single meaningful body line truncated at 60 chars. Future experiments can crank the budgets via env without a rebuild.

### Measured (no behaviour change — evidence log)

- **v1.5 Phase 2 "cAST PoC" reverted** based on A/B measurement on the fixed dataset (2026-04-11):

  | Method                        | HINT_LINES=1 | HINT_LINES=3 |          Δ |
  | ----------------------------- | -----------: | -----------: | ---------: |
  | `get_ranked_context` (hybrid) |        0.573 |        0.568 |     −0.005 |
  | **NL hybrid MRR**             |    **0.472** |    **0.464** | **−0.008** |
  | NL `semantic_search`          |        0.422 |        0.381 |     −0.041 |
  | identifier (hybrid)           |        0.800 |        0.800 |          0 |

  Hypothesis: "more body text lines → higher NL recall". **Rejected** — the bundled CodeSearchNet-INT8 is signature-optimised and extra body tokens dilute signal for natural-language queries. Full experiment log, reproduce commands, and follow-up candidates in [`docs/benchmarks.md` §8.1](docs/benchmarks.md).

- **v1.5 baseline for all future v1.5.x measurements** is **`get_ranked_context` hybrid MRR = 0.573** on the fixed 89-query self-matching dataset. The `0.664` number in earlier memos is from the pre-rename dataset and is no longer apples-to-apples — see the §8 footnote in `docs/benchmarks.md`.

### Rationale

These changes are bundled into a single Unreleased block because the refactor (`vec_store.rs` split), the new env knobs, the dataset fix, and the PoC revert all arrived in the same 2026-04-11 iteration. Each item is its own commit/PR so `git log` and GitHub PR history stay bisectable.

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
