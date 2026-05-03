# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


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
