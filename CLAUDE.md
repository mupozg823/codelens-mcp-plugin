# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

This repo **is** the CodeLens MCP server. The routing/workflow blocks below are also consumed by `.cursor/rules/codelens-routing.mdc` (`alwaysApply: true`) and by `AGENTS.md` (Codex). The `<!-- CODELENS_HOST_ROUTING:BEGIN/END -->` markers carry the canonical block printed by `codelens-mcp attach <host>` (claude-code here, codex in AGENTS.md) — sync from that output, do not edit by hand. `scripts/surface-manifest.py` manages the separate `SURFACE_MANIFEST_*` marker family.

## Repository Architecture

Cargo workspace, edition 2024, `version = "1.13.22"` shared via `[workspace.package]`:

- **`crates/codelens-engine`** — pure library: tree-sitter extractors, SQLite FTS5 + sqlite-vec store, hybrid retrieval (BM25 + ONNX embeddings), call/import graph, refactor primitives (rename/move/inline/edit-transaction), LSP client, optional SCIP backend. No MCP-specific code.
- **`crates/codelens-mcp`** — MCP server binary. Owns the dispatch table, tool surfaces (presets/profiles), workflow orchestration, response envelope (token compression, suggested_next_tools, doom-loop detection), HTTP/stdio transports, and integration tests. The bin target is `codelens-mcp`; **lib target does not exist** — `cargo test -p codelens-mcp --lib` fails.

Three concepts that show up across files and require reading several to understand:

1. **Tool definitions are codegen.** `crates/codelens-mcp/tools.toml` is the canonical schema source. `scripts/regen-tool-defs.py --write` regenerates `crates/codelens-mcp/src/tool_defs/generated/build_generated.rs`. CI fails on drift (`tool-defs codegen drift check`). After editing `tools.toml`, always run the regen and commit the generated file verbatim.
2. **Surfaces gate which tools are visible.** A tool can be registered in `tools.toml` + dispatched in `tools/mod.rs` + implemented in `tools/<area>.rs` and **still not appear in `tools/list`** because no preset/profile exposes it. The preset constants (`PLANNER_READONLY_TOOLS`, `BUILDER_MINIMAL_TOOLS`, `REVIEWER_GRAPH_TOOLS`, `REFACTOR_FULL_TOOLS`, `CI_AUDIT_TOOLS`) live in `crates/codelens-mcp/src/tool_defs/presets.rs`. `set_preset`/`set_profile` switch the active surface at runtime per session.
3. **Generated documentation blocks must round-trip.** `scripts/surface-manifest.py` rewrites marker pairs (`SURFACE_MANIFEST_*`, `CODELENS_HOST_ROUTING`) in README.md, AGENTS.md, CLAUDE.md, docs/architecture.md, etc. The script's `replace_block` produces `BEGIN + \n\n + content + \n\n + END` to coexist with Prettier (which would otherwise re-insert the blank line and cause permanent drift). Do not hand-edit content inside markers.

### Symbol-query path lives behind one seam

`get_ranked_context`, `find_symbol`, and `get_symbols_overview` all dispatch through a single deep module: `crates/codelens-mcp/src/tools/symbol_query/`. Each tool's `pub fn` in `tools/symbols/handlers.rs` is a 3-line entry that constructs a `SymbolQueryRequest` variant and calls `SymbolQueryPipeline::run`. The orchestration body (query analysis → retrieval → rank fusion → SCIP enrichment → payload shaping) lives **inside** the pipeline module, not in `handlers.rs`.

Module layout (post-PR-F/G/H):

```
crates/codelens-mcp/src/tools/
├── semantic_retriever.rs           ← cross-cutting (pipeline + impact reports)
├── symbol_query/
│   ├── mod.rs                       ← SymbolQueryPipeline + SymbolQueryRequest
│   ├── find_symbol.rs               ← stage body for find_symbol
│   ├── ranked_context.rs            ← stage body for get_ranked_context
│   ├── symbols_overview.rs          ← stage body for get_symbols_overview
│   ├── sparse_retriever.rs          ← BM25F + context-window-adaptive budget + flatten_symbols
│   └── rank_fusion.rs               ← stage-4 helpers (5 fn + RankFusionPolicy, all pub(super))
└── symbols/
    ├── handlers.rs                  ← 31 LOC: 3 thin pipeline stubs only
    ├── bm25_search.rs               ← bm25_symbol_search + suggested_follow_up + confidence_tier
    ├── fuzzy_search.rs              ← search_symbols_fuzzy (hybrid + semantic boost)
    ├── inventory.rs                 ← refresh_symbol_index + get_complexity + get_project_structure
    ├── formatter.rs                 ← compact_symbol_bodies (used by pipeline)
    └── analyzer.rs                  ← semantic_scores_for_query
```

When changing symbol-query semantics:
- Body of `run_ranked_context` / `run_find_symbol` / `run_symbols_overview` is in `tools/symbol_query/<tool>.rs`.
- Cross-cutting retrieval seams owned by the pipeline:
  - `tools/semantic_retriever.rs` (dense ONNX semantic results) — used by the pipeline **and** the impact-report family.
  - `tools/symbol_query/sparse_retriever.rs` (BM25F sparse hits, context-window-adaptive budget, `flatten_symbols` utility) — used by the pipeline **and** `symbols::{bm25_search, inventory}`.
- Rank-fusion stage (PR-H): the 5 helpers + `RankFusionPolicy` are `pub(super)` in `symbol_query/rank_fusion.rs`. `ranked_context.rs` is the only legitimate caller — the seam exists so the pipeline owns stage-4 entirely. Do not export rank-fusion items out of `symbol_query/`.
- Other stage helpers (SCIP signature/body slicing in `find_symbol.rs`, body Jaccard, query analysis) are file-private inside their `symbol_query/<tool>.rs` — do not promote to `pub(super)` casually.

Dependency direction is one-way: `symbols::*` → `symbol_query::*`. Never reach upward from the pipeline back into `symbols::*` — that was the cycle PR-F removed (`review_architecture` reported a 3-node loop `mod.rs → ranked_context.rs → handlers.rs`). If new sparse/retrieval helpers are needed, add them to `symbol_query/sparse_retriever.rs` (or a sibling sub-module).

## Feature Flag Matrix (build-time)

The default `cargo install codelens-mcp` build is `default = ["scip-backend"]` (set in `crates/codelens-mcp/Cargo.toml`; SCIP itself only activates when an `index.scip` exists in the project). Most other operational use needs explicit features:

| Feature        | When required                                                    | Symptom if missing                                                                                        |
| -------------- | ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `http`         | Any HTTP transport / daemon mode                                 | `Error: HTTP transport requires the http feature` at startup, port never binds                            |
| `semantic`     | `semantic_search`, `index_embeddings`, hybrid ranking            | Tools degrade to BM25-only; status reports `FeatureDisabled`                                              |
| `scip-backend` | SCIP precise navigation in `find_symbol`, `heuristic_body_slice` | `cargo clippy --no-default-features` flags `dead_code` on `#[cfg(feature = "scip-backend")]`-only callees |
| `coreml`       | macOS CoreML execution provider for ONNX                         | Falls back to CPU silently                                                                                |
| `otel`         | OpenTelemetry export                                             | No telemetry emitted                                                                                      |

**Daemon rule:** `~/Library/LaunchAgents/dev.codelens.mcp-{readonly,mutation}.plist` invokes `target/release/codelens-mcp --transport http …`. The release binary **must** be built with `--features http` or both daemons exit immediately. `cargo build --release` alone is insufficient.

## Build & Verify

```bash
# Default verify (matches local pre-push)
cargo check
cargo test -p codelens-engine
cargo test -p codelens-mcp --bin codelens-mcp        # NOT --lib (no lib target)

# Feature-matrix mirroring CI (.github/workflows/ci.yml)
cargo check --workspace --features http
cargo check --workspace --features otel
cargo check --workspace --features scip-backend
cargo check --workspace --no-default-features        # "semantic-off" gate
cargo clippy --workspace -- -D warnings
cargo clippy --workspace --features scip-backend -- -D warnings
cargo clippy --workspace --no-default-features -- -D warnings
cargo nextest run --workspace                        # CI uses nextest
cargo nextest run --workspace --features http
cargo nextest run --workspace --no-default-features
cargo test --doc --workspace
cargo fmt --all -- --check                           # Prefer running this; matches CI

# Single test
cargo test -p codelens-mcp --bin codelens-mcp <test_substring>
cargo test -p codelens-engine --lib <test_substring>

# Codegen drift gates (CI runs these too)
python3 scripts/regen-tool-defs.py --check           # tools.toml ↔ build_generated.rs
python3 scripts/surface-manifest.py --check          # generated doc blocks
python3 benchmarks/lint-datasets.py --project .      # benchmark dataset hygiene

# Release build for the local launchd daemons
cargo build --release --features http,semantic
bash scripts/install-http-daemons-launchd.sh . --load
```

`scripts/quality-gate.sh` and `scripts/mcp-doctor.sh . --strict` are convenience wrappers; CI is the authoritative pre-merge gate.

## HTTP Daemon Operations

Repo-local launchd readonly/mutation daemons, the `redeploy-daemons.sh` restart
cycle, and macOS xattr/codesign (`OS_REASON_CODESIGNING`) recovery live in
[`docs/operations/http-daemon.md`](docs/operations/http-daemon.md).

## Common Pitfalls

- **Local rustfmt vs CI rustfmt drift on `use` ordering.** A user-global post-edit hook may reorder imports alphabetically. CI uses `cargo fmt --all -- --check` with the workspace's default rustfmt config (declaration order). Always run `cargo fmt --all` before pushing — `cargo fmt --check` exit code is the truth.
- **Rebase reverts merged content silently.** When a long-lived branch is rebased onto a moved `main`, commits authored before recent merges can drop the merged content if they happened to touch overlapping regions. After every rebase, `git diff main..HEAD -- <suspect-file>` must show only the intended PR changes.
- **`cargo install codelens-mcp` is BM25 + SCIP only, no semantic.** Default features are `["scip-backend"]` (ADR-0012 set them to `[]` in v1.10.0; v1.13.17 added `scip-backend` to the default set when SCIP became on-by-default). The `cargo install --force` upgrade path won't auto-add `semantic` or `http` — both still need explicit `--features`.
- **Surface manifest version markers.** `Workspace version: \`1.x.y\``strings inside non-marker README/docs sections trigger`canonical*truth_violations()`in`scripts/surface-manifest.py`. Keep version claims inside `SURFACE_MANIFEST*\*` blocks only.
- **Tools.toml entries without preset membership are invisible.** A new analysis tool added to `tools.toml` must be inserted into one of the preset constants in `tool_defs/presets.rs` to surface in any `tools/list` response, even though it remains directly callable via `tools/call`.

<!-- CODELENS_HOST_ROUTING:BEGIN -->

## CodeLens Routing

- Use native Read/Glob/Grep first for trivial point lookups and single-file edits.
- Escalate to CodeLens after the first local step for multi-file review, refactor preflight, or durable artifact generation.
- Default CodeLens profile for planning/review is `reviewer-graph`.
- Before dispatching a builder, run:
  1. `prepare_harness_session`
  2. `get_symbols_overview` per target file
  3. `get_file_diagnostics` per target file
  4. `verify_change_readiness`
- Prefer asymmetric handoff over live planner/builder chat.
- If `delegate_to_codex_builder` appears in `suggested_next_calls`, preserve `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` verbatim when dispatching the builder.

## Compiled Routing Overlays

- Primary bootstrap sequence: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `explore_codebase` -> `review_architecture`
- `planner-readonly` + `planning` [bias: `claude`]: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `explore_codebase` -> `review_architecture`
- `reviewer-graph` + `review` [bias: `claude`]: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `diff_aware_references` -> `audit_planner_session`
- `planner-readonly` + `onboarding` [bias: `claude`]: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `onboard_project` -> `explore_codebase` -> `review_architecture`

<!-- CODELENS_HOST_ROUTING:END -->

## Tool Routing Reference

The exhaustive CodeLens-vs-Grep scenario matrix, scale-dependency measurements,
known accuracy limits, and problem-first workflow patterns live in
[`docs/operations/tool-routing-matrix.md`](docs/operations/tool-routing-matrix.md).
The concise routing rules below cover the common path.

## Agent Roles

- **Codex**: implementation, local refactor, direct test execution
- **Claude**: orchestration, review, evaluation, harness supervision
- CodeLens = external coprocessor, not embedded runtime

## Routing

- Simple local lookup/edit → native first
- Multi-file impact/review/refactor → escalate to CodeLens workflow
- Heavy analysis → async handle/job path (`start_analysis_job` → `get_analysis_job`)
- CodeLens timeout/fail → native fallback
- **Precision refactoring** → use `use_lsp=true` for type-aware results

## Harness Modes

- **A: Native Fast Path** — trivial lookups, single-file, < 30 LOC
- **B: CodeLens Read-Only** — multi-file context, ranked symbols, impact review
- **C: Verifier-First Mutation** — `verify_change_readiness` before rename/edit
- **D: Async Analysis** — `start_analysis_job` → poll → `get_analysis_section`

## Mutation Gate Protocol (Mode C)

**Before CodeLens mutation tools** (`rename_symbol`, `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `refactor_*`), you SHOULD:

1. Run `verify_change_readiness` with the target file path(s)
2. Check `mutation_ready` field in the response:
   - `"ready"` → proceed with mutation
   - `"caution"` → proceed but run `get_file_diagnostics` after
   - `"blocked"` → resolve blockers before mutating
3. For `rename_symbol` specifically: run `safe_rename_report` instead of `verify_change_readiness`

**Fallback:** If CodeLens is unavailable or returns an error, proceed with native tools (Edit + cargo check/test). The harness MUST NOT block on CodeLens failures.

**After mutation:** follow `suggested_next_tools` from the response when available.

**Preflight TTL:** Override via `CODELENS_PREFLIGHT_TTL_SECS` env var (default 600s).

## Runtime & Response Reference

Runtime knobs and response-shaping internals are documented out of the hot path:

- [`docs/operations/response-envelope.md`](docs/operations/response-envelope.md) — effort level, lean response contract, adaptive 5-stage compression, doom-loop protection, index-freshness signal, schema pre-validation, MCP response annotations.
- [`docs/operations/runtime-knobs.md`](docs/operations/runtime-knobs.md) — semantic edit backend selection, analysis artifact cache (LRU + TTL), backup rotation.
