# ADR-0011: Control-Plane Sprawl Resolution

## Status

Accepted

## Date

2026-05-02

## Context

Two consecutive audits — `docs/architecture-audit-2026-04-24.md` and the
2026-05-02 external audit landed in this branch — converged on the same
diagnosis: the workspace is sound at the top level (two crates,
single dispatch table, generated surface manifest), but the
`codelens-mcp` crate has accumulated layered indirection, dead feature
flags, and presentation drift that the build pipeline does not catch.

Concrete findings reproduced in the 2026-05-02 audit:

1. `tool_defs::tool::McpTool` was a single-method `Send + Sync` trait
   with one implementor (`BuiltTool`). Its `is_enabled` default was
   never overridden, and the only call-site in `dispatch::query_engine`
   was a dead `if !tool.is_enabled(state)` branch.
2. `Cargo.toml` declared `audit = []` as an empty feature, default-on,
   with one consumer (`#[cfg(feature = "audit")] mod audit_sink;`). The
   gate could not meaningfully be off, but the CI matrix used it as
   part of a `--no-default-features --features audit` combination.
3. `crates/codelens-mcp/src/artifact_store.rs` test fixture used
   `std::env::temp_dir().join(format!("codelens-test-{}", pid))` as a
   shared per-pid path. Cargo's default test parallelism caused 5
   tests to race on the same directory; one of them (`tiered_exact_
preferred_over_warm`) flaked with `Io(Os{ code: 22, kind:
InvalidInput })` on macOS APFS.
4. `crates/codelens-tui/` was a Cargo crate (Cargo.toml, src/, README,
   LICENSE) excluded from `[workspace] members` even though
   `release-plz.toml` still tracked it for changelogs and
   `docs/release-distribution.md` still documented `cargo publish -p
codelens-tui`. `cargo build --workspace` silently skipped it.
5. `docs/architecture.md` advertised `Workspace members: 3
(...codelens-tui)` and `Workspace version: 1.9.59` while the README
   already advertised `members: 2` and version `1.9.60` — the canonical
   surface-manifest JSON had not been refreshed.
6. `cargo fmt --all -- --check` flagged 4 files; CI had no fmt gate, so
   the drift accumulated without surfacing.
7. Of the 29 `// Composite (multi-step workflows)` tools registered in
   `tool_defs/build.rs`, three (`explain_code_flow`,
   `find_minimal_context_for_change`, `summarize_symbol_impact`)
   overlapped meaningfully with the seven workflow-first entry points
   already published in `tools/workflows.rs`.
8. `crates/codelens-mcp/Cargo.toml` did not exclude in-source test
   trees (`src/integration_tests/**`, `src/server/http_tests/**`,
   `src/cli/startup_tests.rs`). `cargo package` therefore shipped
   ~14k LOC of `cfg(test)` code to crates.io consumers who never
   build it.

ADR-0001 already established that the two-crate split is correct and
the right move is intra-layer simplification, not a rewrite. ADR-0010
established a telemetry-driven retirement pipeline for tools but did
not address structural sprawl in the dispatch layer or feature gates.
ADR-0011 closes the structural-sprawl gap that ADR-0001 left as a
direction without concrete decisions, and reuses ADR-0010's
deprecation pipeline for the three composite tools identified above.

## Decision

The branch `feature/phase-1-cleanup` lands the following nine changes
as discrete, individually-revertible commits. Every change is verified
by `cargo build --workspace --release`, `cargo clippy --workspace --
-D warnings`, and `cargo test --workspace` (565 mcp + 339 engine + tui

- doctests).

1. **`McpTool` trait removed** in favour of a flat
   `pub type ToolHandler = Arc<dyn Fn(&AppState, &Value) -> ToolResult
   - Send + Sync>`. The dead `is_enabled`branch in`dispatch::query_engine` is also dropped. Net −40 lines.
2. **`audit` feature flag deleted** from `crates/codelens-mcp/Cargo.toml`,
   the `#[cfg(feature = "audit")]` guard above `mod audit_sink;`
   removed, and `--features audit` excised from the four CI
   `semantic-off` steps.
3. **`artifact_store` tests isolated** behind `tempfile::TempDir`
   instances returned from `make_store()`, eliminating the per-pid
   directory race. Test wall-time drops from ~34 s to ~10 ms.
4. **`crates/codelens-tui/` re-attached to the workspace** as a member
   plus default-member. Build/clippy/test sweep all pass; tui
   regressions now share the same CI gate as the rest of the tree.
5. **`docs/architecture.md` SNAPSHOT block re-synced** to `members:
2` / version `1.9.60`, and `docs/generated/surface-manifest.json`
   refreshed so the next `scripts/surface-manifest.py --write` stays
   consistent.
6. **`cargo fmt --check` added as the first step in `.github/
workflows/ci.yml`** so future drift fails before any other gate.
7. **Three composite tools deprecated** through
   `tool_defs::presets::tool_deprecation`: `explain_code_flow` →
   `trace_request_path`; `find_minimal_context_for_change` →
   `analyze_change_request`; `summarize_symbol_impact` →
   `impact_report`. Removal scheduled for v2.0 alongside the existing
   alias purge.
8. **`crates/codelens-mcp/Cargo.toml exclude` extended** to drop
   `src/integration_tests/**`, `src/server/http_tests/**`, and
   `src/cli/startup_tests.rs` from the published tarball.
9. **The pre-existing in-flight working-tree changes** (envelope
   `max_tokens` precedence, workflow `project_root` passthrough,
   four clippy hygiene fixes) are preserved and split into two
   discrete commits (`feat(envelope)` and `chore: clippy hygiene`)
   instead of being rolled into the audit changes.

## Consequences

### Positive

- One less indirection layer in the dispatch hot path.
- Test surface honestly mirrors product surface: in-tree tests still
  run on local checkout, but published tarballs no longer ship
  `cfg(test)` code.
- TUI regressions are no longer invisible to `cargo build --workspace`.
- The fmt gate eliminates an entire class of pre-PR cleanup churn.
- Surface manifest drift is detectable through a single source of
  truth file (`docs/generated/surface-manifest.json`), with both
  README and `architecture.md` re-derived from it.

### Negative

- Cargo.lock grows by ~877 lines from the tui re-attachment
  (ratatui 0.30 + crossterm 0.28 + wezterm-\* transitive deps). Build
  artefact size for `codelens-tui` is borne by anyone running the
  default `cargo build --workspace`, not just `cargo build -p
codelens-tui`. Mitigation: tui is already its own bin target so
  `cargo install codelens-mcp` users are unaffected.
- The three composite-tool deprecations widen the warning surface in
  `tools/list` for harnesses that still call the old names.

### Neutral / Deferred

The 2026-05-02 audit identified a further set of items that this ADR
does **not** resolve:

- `cargo clippy --workspace --all-targets -- -D warnings` surfaces 16
  pre-existing warnings (digits-grouping in test fixtures,
  `very-complex-type` in `engine::edit_transaction`,
  `module-inception` in `integration_tests/workflow/mod.rs`,
  `items-after-test-module` in two locations, `Error::other` and
  `if-let` lint, FRAC_1_PI literal). CI only runs `cargo clippy
--workspace`, so these are silent. **Action:** queued as a
  follow-up PR; do not bundle into this ADR's scope.
- `AppState` god-struct (25+ fields, 14 host sub-modules). **Action:**
  separate ADR, after telemetry shows which fields are session-scoped
  vs daemon-global.
- `tool_defs/output_schemas.rs` (1484 LOC) and
  `tool_defs/presets.rs` (1470 LOC). **Action:** `build.rs`-based
  generation evaluated separately; risk of churn is high.
- `dispatch/response.rs` + `response_support.rs` (≈1900 LOC combined)
  carrying the 5-stage compression pipeline. **Action:** evaluate
  alongside the AppState split.
- `docs/benchmarks.md` at 213 KB, single file. **Action:** split into
  `docs/benchmarks/<date>.md` when next benchmark cycle lands.
- ONNX semantic sidecar default-on vs `cargo install` UX. **Action:**
  separate ADR — flipping the `semantic` default has user-visible
  download-size implications and deserves its own decision.

## Cross-Reference

- ADR-0001 establishes the two-crate substrate and "simplify inside,
  don't rewrite" stance.
- ADR-0010 defines the deprecation pipeline reused for the three
  composite-tool retirements above.
- `docs/architecture-audit-2026-04-24.md` is the prior internal audit;
  the 2026-05-02 audit is logged in this commit's PR description.
- The deferred items above are tracked as the Phase 3 backlog of the
  2026-05-02 audit report.
