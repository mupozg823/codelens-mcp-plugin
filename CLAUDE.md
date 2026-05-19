# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

This repo **is** the CodeLens MCP server. The routing/workflow blocks below are also consumed by `.cursor/rules/codelens-routing.mdc` (`alwaysApply: true`) and by `AGENTS.md` (Codex). The `<!-- CODELENS_HOST_ROUTING:BEGIN/END -->` markers are managed by `scripts/surface-manifest.py` — do not edit by hand.

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

## HTTP Daemon Operations (this repo)

Two repo-local launchd agents share the on-disk index and use advisory `register_agent_work` / `claim_files` for mutation collisions:

- `dev.codelens.mcp-readonly` → `:7839`, profile `reviewer-graph`, mode `read-only` — for planner/reviewer (Claude) sessions
- `dev.codelens.mcp-mutation` → `:7838`, profile `refactor-full`, mode `mutation-enabled` — for builder (Codex) sessions

Both clients (`~/.claude.json`, `~/.codex/config.toml`) attach by URL to `:7839` by default. Restart cycle (preferred path):

```bash
bash scripts/redeploy-daemons.sh --probe          # quick: cp + xattr/codesign + kickstart + LISTEN + tools/list
bash scripts/redeploy-daemons.sh --build --probe  # also runs cargo build --release --features http,semantic
bash scripts/daemon-stale-check.sh                # read-only: compare daemon binary git sha to source HEAD (exit 1 if stale)
```

What the script does: `cp target/release/codelens-mcp → .codelens/bin/codelens-mcp-http`, `xattr -dr com.apple.provenance ${target}` (otherwise macOS gatekeeper SIGKILLs the daemon with `OS_REASON_CODESIGNING`), `codesign --force --sign -` (ad-hoc resign so launchd accepts the new mach-o), `launchctl kickstart -k gui/$UID/dev.codelens.mcp-{readonly,mutation}`, wait for LISTEN on 7838/7839, and (with `--probe`) issue `tools/list` against both.

Manual fallback (if the script is unavailable):

```bash
cp -f target/release/codelens-mcp .codelens/bin/codelens-mcp-http
xattr -dr com.apple.provenance .codelens/bin/codelens-mcp-http
codesign --force --sign - .codelens/bin/codelens-mcp-http
launchctl kickstart -k "gui/$(id -u)/dev.codelens.mcp-readonly"
launchctl kickstart -k "gui/$(id -u)/dev.codelens.mcp-mutation"
sleep 4 && pgrep -fl codelens-mcp
```

If `pgrep` shows nothing after restart, the binary is missing `--features http` (see the matrix above) — check `.codelens/reports/launchd/dev.codelens.mcp-readonly.err.log`. If the err log shows `last exit reason = OS_REASON_CODESIGNING`, the xattr/codesign step was skipped.

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
- `reviewer-graph` + `review` [bias: `claude`]: `prepare_harness_session` -> `review_changes` -> `impact_report` -> `diff_aware_references` -> `audit_planner_session`
- `planner-readonly` + `onboarding` [bias: `claude`]: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `onboard_project` -> `explore_codebase` -> `review_architecture`
<!-- CODELENS_HOST_ROUTING:END -->

## Tool Routing — honest scenario matrix (updated 2026-04-19)

Benchmarks (see `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md`)
show CodeLens and grep are **complementary, not a one-way replacement**.
Pick by question shape, not by reflex.

### Precision / structural navigation — prefer CodeLens

| Task                                    | Use                                              | Why                                                                |
| --------------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------ |
| Find function/class/type definition     | `mcp__codelens__find_symbol` (include_body=true) | Exact file/line/column + kind + signature + `suggested_next_tools` |
| File/directory structure                | `mcp__codelens__get_symbols_overview`            | AST-accurate, includes private symbols grep can miss               |
| Who calls / inherits X (real callsites) | `mcp__codelens__find_referencing_symbols`        | Rejects imports / strings / type annotations grep floods you with  |
| Smart context for a query               | `mcp__codelens__get_ranked_context`              | Bundled by importance + hybrid BM25 + semantic                     |
| What breaks if I change X               | `mcp__codelens__get_impact_analysis`             | Blast radius + importer evidence grep cannot produce               |
| Type errors after edit                  | `mcp__codelens__get_file_diagnostics`            | Machine-readable diagnostics stream                                |
| First look at unfamiliar repo           | `mcp__codelens__onboard_project`                 | Key files + structure + health in one call                         |
| Safe multi-file rename                  | `mcp__codelens__rename_symbol`                   | Verifier-gated; refuses broken renames                             |
| NL query over embeddings                | `mcp__codelens__semantic_search` (if indexed)    | Fallback to `bm25_symbol_search` when semantic index is absent     |
| Change impact report                    | `mcp__codelens__impact_report`                   | Bounded, summary + evidence                                        |

### Recall / text audits / fuzzy — prefer Grep (or specific CodeLens fuzzy tools)

| Task                                              | Use                                       | Why                                                                                  |
| ------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------ |
| "Where is this string mentioned at all?"          | **Grep**                                  | CodeLens's call-graph view intentionally drops imports / strings / comments          |
| Imports + comments + docstring audits             | **Grep**                                  | Tree-sitter does not index non-code mentions                                         |
| Fuzzy / partial name ("register…")                | `mcp__codelens__bm25_symbol_search`       | `find_symbol` requires exact name; BM25 tolerates partial or NL token shape          |
| LSP-aware workspace fuzzy (when LSP is available) | `mcp__codelens__search_workspace_symbols` | Needs `command` (e.g. rust-analyzer). Without it, handler returns a hint toward BM25 |
| Single-file known path, < 30 lines                | **Read**                                  | No need to pay index warm-up cost                                                    |
| Exact 1–2 string matches in 1–2 files             | **Grep**                                  | Often faster than CodeLens on small repos                                            |

### Scale dependency (measured)

| Repo size                    | CodeLens find_symbol advantage | Prefer                                |
| ---------------------------- | ------------------------------ | ------------------------------------- |
| Large monorepo (>100K files) | 100–500× faster                | CodeLens everywhere                   |
| Medium Python/TS (287 files) | ~1–2×, roughly tied            | CodeLens for structure, grep for text |
| Single file, < 30 lines      | n/a                            | Read / Grep                           |

### Known accuracy limits (2026-04-19)

- Python `find_referencing_symbols` misses imports + type annotations
  (tree-sitter extractor gap). Use Grep if you also want to audit them.
- Decorated classes (`@dataclass class X:`) may return two rows
  (decorator + body). Ignore the decorator row for navigation.
- `find_symbol` with a non-existent exact name now returns a
  `fallback_hint` pointing at `search_workspace_symbols`,
  `search_symbols_fuzzy`, and `bm25_symbol_search` — follow it.

**After ANY code mutation:** follow `suggested_next_tools` — always includes `get_file_diagnostics`.

## Problem-First Workflows (v1.7+)

Instead of choosing from 90 individual tools, use these **workflow patterns**:

| Workflow               | Tools Orchestrated                                                                      | When                                                            |
| ---------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| **Explore codebase**   | `onboard_project` → `get_symbols_overview` → `get_ranked_context`                       | First look at unfamiliar code                                   |
| **Plan safe refactor** | `analyze_change_request` → `verify_change_readiness` → `safe_rename_report`             | Before any multi-file rename/move                               |
| **Audit architecture** | `module_boundary_report` → `dead_code_report` → `find_misplaced_code` → `impact_report` | Architecture review / tech debt assessment                      |
| **Trace request path** | `find_symbol` → `find_referencing_symbols` → `get_impact_analysis`                      | "How does X work? What calls Y?"                                |
| **Review changes**     | `impact_report` → `diff_aware_references` → `get_file_diagnostics`                      | Pre-merge review                                                |
| **Cleanup duplicates** | `find_code_duplicates` → `find_similar_code` → `refactor_extract_function`              | DRY violation resolution                                        |
| **Assess security**    | `dead_code_report` → `find_annotations` → external CodeQL/Semgrep                       | Security audit (CodeLens provides context, not formal analysis) |

**Rule**: Start from the workflow, not from individual tools. Let CodeLens's `suggested_next_tools` guide the chain.

**Precision note**: For type-aware refactoring (rename across type hierarchies, find implementations), use `use_lsp=true` on `find_referencing_symbols`. tree-sitter alone may miss type-level relationships.

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

**Before CodeLens mutation tools** (`rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `delete_lines`, `add_import`, `refactor_*`), you SHOULD:

1. Run `verify_change_readiness` with the target file path(s)
2. Check `mutation_ready` field in the response:
   - `"ready"` → proceed with mutation
   - `"caution"` → proceed but run `get_file_diagnostics` after
   - `"blocked"` → resolve blockers before mutating
3. For `rename_symbol` specifically: run `safe_rename_report` instead of `verify_change_readiness`

**Fallback:** If CodeLens is unavailable or returns an error, proceed with native tools (Edit + cargo check/test). The harness MUST NOT block on CodeLens failures.

**After mutation:** follow `suggested_next_tools` from the response when available.

**Preflight TTL:** Override via `CODELENS_PREFLIGHT_TTL_SECS` env var (default 600s).

## Doom-Loop Protection

The server detects identical tool+args called 3+ times consecutively:

- `budget_hint` warns about the repetition
- `suggested_next_tools` switches to alternative high-level tools
- **Rapid burst detection**: 3+ identical calls within 10 seconds triggers async job fallback suggestions (`start_analysis_job`)
- Applies only in persistent MCP stdio mode (not CLI one-shot)

## Index Freshness Signal

The four read-hot symbol tools (`find_referencing_symbols`, `find_symbol`, `get_ranked_context`, `get_symbols_overview`) and `onboard_project` attach an `index_freshness` object to every response so callers can detect a stale daemon without diffing results against the working tree:

```json
{
  "newest_indexed_at_epoch_secs": 1779032712,
  "newest_indexed_age_secs": 642,
  "staleness_hint": "possibly_stale",
  "refresh_recommended": false
}
```

Buckets (newest `files.indexed_at` vs wall-clock): `fresh` < 60s · `recent` 60s..600s · `possibly_stale` 600s..3600s · `stale` ≥ 3600s. When `refresh_recommended: true`, the response also prepends `refresh_symbol_index` to `suggested_next_tools` so an agent doesn't need to know the recovery path — just follow the chain.

If you're driving the harness manually, call `refresh_symbol_index` once after a large file move/rename burst (e.g. multi-file refactor) — the daemon does not auto-watch for changes.

## Schema Pre-Validation

Dispatch validates `required` fields from `input_schema` before the handler runs.
Missing required params fail immediately with `MissingParam` error (no handler execution cost).

## MCP Response Annotations

Responses include `_meta["anthropic/maxResultSizeChars"]` per MCP spec (Claude Code v2.1.91+).
Values scale by tool tier: Workflow=200K, Analysis=100K, Primitive=50K chars.

## Effort Level

Controls compression aggressiveness. Set via `CODELENS_EFFORT_LEVEL` env var.

- `low` — compress earlier (thresholds -10pp), budget ×0.6
- `medium` — default thresholds
- `high` — compress later (thresholds +10pp), budget ×1.3 **(default, matching Claude Code v2.1.94)**

## Adaptive Token Compression (OpenDev 5-Stage)

Response payloads are compressed based on budget usage.
Thresholds are adjusted by effort level offset (Low=-10, Medium=0, High=+10):

- Stage 1 (<75%): pass through
- Stage 2 (75-85%): light structured content summarization
- Stage 3 (85-95%): aggressive summarization
- Stage 4 (95-100%): minimal skeleton + truncated flag
- Stage 5 (>100%): hard truncation with error payload
