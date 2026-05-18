# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.13.29] - 2026-05-18

### Fixed

- **macOS test flake root cause (#332)**: 6 PRs (PR-F..PR-J) had been failing the same macOS-only check (`codelens-engine search::tests::semantic_low_scores_filtered_out`) since 2026-04. systematic-debugging tracked it to a path-collision bug in the shared search-test fixture: `subsec_nanos()` quantises to ~1 µs on macOS vs 1 ns on Ubuntu, so the 9 tests sharing `make_project_with_symbols` regularly hit the same temp-dir path. Two tests racing into the same SQLite file then hit `journal_mode = WAL`'s schema-level lock before the batch's own `busy_timeout = 5000` PRAGMA was applied (it was the 6th PRAGMA). Fixed by switching the fixture to `tempfile::TempDir`.
- **db (defense in depth) (#333)**: reorder `IndexDb::open` and `SqliteVecStore::new` PRAGMA batches so `busy_timeout = 5000` precedes `journal_mode = WAL`. Any production caller that ever opens the same DB from two threads now waits up to 5 s instead of erroring out instantly.

### Added

- **transparency: index_freshness signal (PR-J, #329)**: `find_referencing_symbols`, `find_symbol`, `get_ranked_context`, `get_symbols_overview` (and `onboard_project` via PR-L) now attach an `index_freshness` object with `newest_indexed_at_epoch_secs`, `newest_indexed_age_secs`, a 4-bucket `staleness_hint` (`fresh`/`recent`/`possibly_stale`/`stale`), and `refresh_recommended`. Backed by new `IndexDb::max_files_indexed_at()` / `SymbolIndex::max_indexed_at()` queries.
- **auto-suggest refresh on stale index (PR-L, #331)**: when `refresh_recommended: true`, `refresh_symbol_index` is prepended to `suggested_next_tools` so an agent can recover without knowing the call name. Idempotent. Sits after doom-loop override (heavy retry guidance still wins) but before the calls-builder.
- **onboard_project carries `index_freshness` (PR-L, #331)**: the first call into a new MCP session reports its own index staleness up front.

### Changed

- **deps: workspace pin for `codelens-engine` (PR-K, #328)**: moved the `version = "=…"` pin into `[workspace.dependencies]`. Future releases bump a single version string at the workspace root; member crates use `codelens-engine = { workspace = true }`. release-plz can now propagate version bumps without a member-crate hand-edit.
- **docs: Index Freshness Signal section in CLAUDE.md (#334)**: documents the four-bucket staleness contract and the human-driven `refresh_symbol_index` workflow for large multi-file renames.

### Infrastructure

- **release-plz workflow permission**: GitHub Actions can now create release PRs (`default_workflow_permissions=write` + `can_approve_pull_request_reviews=true`). Closes the recurring 403 in the release-plz workflow that had been blocking automation since the PR-F..PR-I sequence.

## [1.13.28] - 2026-05-18

### Refactored

- **arch (PR-A through PR-H)**: deep-module deepening of the symbol-query path.
  - PR-A/B/C/D/E (#317, #319, #320, #321, #322 → bulk #323): `SymbolQueryPipeline` introduced at `crates/codelens-mcp/src/tools/symbol_query/`. The three symbol-shape tools (`get_ranked_context`, `find_symbol`, `get_symbols_overview`) now dispatch through a single seam; each `pub fn` in `tools/symbols/handlers.rs` is a 3-line `SymbolQueryRequest::* → SymbolQueryPipeline::run` stub.
  - **PR-F (#324)**: cycle `mod.rs → ranked_context.rs → handlers.rs` removed by extracting `sparse_retriever.rs`. Dependency flow is now one-way: `symbols::*` → `symbol_query::*`. `review_architecture` live verify: `cycle hits = 0`.
  - **PR-G (#325)**: `tools/symbols/handlers.rs` split by responsibility into `bm25_search.rs` + `fuzzy_search.rs` + `inventory.rs`. `handlers.rs` collapsed from 448 → **31 LOC** (-93%).
  - **PR-H (#326)**: stage-4 rank-fusion extracted to `symbol_query/rank_fusion.rs` — 5 helpers + `RankFusionPolicy` are `pub(super)`, `ranked_context.rs` is the only legitimate caller. `ranked_context.rs` 958 → 665 LOC (-31%).

### Fixed

- **transport (#318 → bulk #323)**: `unknown_session` envelope unified across POST and GET-SSE paths via the new `unknown_session_response()` funnel. Both transports now return JSON + `x-codelens-session-rotate: 1` header.
- **#179 (#314)**: SCIP `end_line` propagation for precise body slicing in `find_symbol`.
- **#268 (#315)**: tree-sitter structural orphan files downgraded from `unused` to `low_confidence` in the dead-code report.
- **#299 (#316)**: `cleanup_duplicate_logic` guards signature-only false positives with body-token Jaccard distance.

### Docs

- **PR-I (#327)**: refresh `CLAUDE.md` "Symbol-query path lives behind one seam" with the post-deepening module tree (11 files across `tools/symbol_query/` + `tools/symbols/`). Update `docs/architecture.md` tool-handler diagram to reflect the deep pipeline + per-tool split + `semantic_retriever` cross-cutting seam.

## [1.9.59] - 2026-04-30

### Added
- **Benchmarks**: `search_paths` benchmark for exact/FTS5/fuzzy/no-match search paths.
- **Benchmarks**: Cache hit/miss benchmark for `ranked_context_cached` (75µs hit vs 413µs miss).
- **Benchmarks**: Large-project indexing benchmark (100 modules, 500+ symbols, ~7.3ms).
- **Tests**: 27 unit tests across `eval_reports`, `report_jobs`, and `semantic_edit_args`.
- **CI**: `semantic-off` build verification (`--no-default-features --features audit`) on every PR.
- **CI**: Slim binary artifact upload (`codelens-mcp-slim`, 58MB) alongside default build.

### Changed
- **Binary Size**: `semantic` feature-gate reduces binary size by 22.7% (75MB → 58MB) when disabled.
- **Architecture**: `SemanticMatch` and related data types moved to unconditional `embedding_types.rs` for graceful degradation.
- **Coverage**: `workflows.rs` line coverage improved from 34% to 65.68% via integration tests.
- **Coverage**: Overall line coverage improved from 82.31% to 82.90%.

### Fixed
- `semantic-off` builds now compile and pass 494 tests without feature-gate regressions.
- `cargo clippy --workspace -D warnings` remains at zero warnings.

## [1.9.58] - 2026-04-28

### Added
- **SCIP Backend**: Initial SCIP index integration with `get_callers` and `get_callees` support.
- **SCIP**: Startup probe with `scip_status` and setup hint surfacing.
- **SCIP**: Stale index detection against `Cargo.lock`/`Cargo.toml` mtime.
- **Call Graph**: Rust macro invocation edges (C-1).
- **Call Graph**: Java constructors and method references (C-2 + C-3).
- **Call Graph**: Python decorators and JSX/TSX component edges.
- **Dispatch**: `limit`/`top_k` argument aliases with unknown-arg surfacing.
- **Dispatch**: Grep-fallback recovery hint when call graph is unresolved.
- **Dispatch**: Compression truncation surfaced at top level.

### Changed
- **Refactor**: Tree-sitter heuristic honesty pass on 4 refactor tools.
- **Tools**: Deprecated v2.0 aliases removed from 5 tools.
- **Audit**: Single audit sink with retention sweep and per-project principals cache.
- **Capabilities**: `model_status` and honest model-sidecar messaging.

### Fixed
- Refactor handlers retain tree-sitter honesty surfaces (CI lint gate).
- `file_path` ↔ `path` bidirectional alias support.

## [1.9.57] - 2026-04-25

### Added
- **Mutation Primitives**: Atomic 2-file substrate for `move_symbol`.
- **Audit**: `audit_log_query` tool and lifecycle state machine.
- **Cache**: Cache invalidation contract with `evidence_hash`.
- **Coordination**: Agent work registration and file claim/release tracking.

### Changed
- Phase 2 close: ADR-0009 self-consistency across M1/M4/M6/M2/L3/L1.
- `get_capabilities` `detail=compact` opt-in.

### Fixed
- Test race conditions and Clippy warnings.
