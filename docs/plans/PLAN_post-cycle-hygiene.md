# PLAN — Post Cycle/Dead-Code Hygiene

**Created**: 2026-04-18
**Last Updated**: 2026-04-18
**Status**: approved, not started
**Approved order**: Phase 2 → 4 → 3 → 1

> **CRITICAL INSTRUCTIONS** — after each phase:
>
> 1. Check off completed checkboxes
> 2. Run phase quality gate commands
> 3. Verify ALL quality gate items pass (do not skip)
> 4. Update "Last Updated" date
> 5. Document learnings in "Notes & Learnings"
> 6. Only then proceed to next phase

## Context

- Architecture cycle `tool_defs/tool.rs ↔ tool_runtime.rs` was resolved on 2026-04-18 (`has_cycles=false`)
- `.claire` added to `EXCLUDED_DIRS` — dead-code noise removed
- 30-file uncommitted diff remains (symbiote-rebrand + host-adaptive-harness)
- `tests::workflow::resources_include_profile_guides_and_analysis_summaries` intermittently fails in `--features http` parallel run
- `McpTool` trait & `BuiltTool` have unused methods/fields (6 dead_code warnings)
- `.claire/` and `.claude/worktrees/` are currently untracked, not gitignored

## Global Invariants

All phases must preserve:

- `cargo check` clean (no errors)
- `cargo test -p codelens-engine` green
- `cargo test -p codelens-mcp` green (349 tests)
- `cargo test -p codelens-mcp --features http` green (404 tests)
- `onboard_project.has_cycles == false`
- No expansion of scope outside phase description

## Phase 2 — `tool.rs` 미소비 추상화 해소 (first, 1h)

**Goal**: remove 6 dead_code warnings by either wiring the methods into real call sites or removing the unused surface.

**Scope (single file)**:

- `crates/codelens-mcp/src/tool_defs/tool.rs`
- Possibly one call site if a method is actually needed (e.g., `is_concurrency_safe` for concurrent dispatch)

**Decision step (do first, before edits)**:

- Run `mcp__codelens__find_referencing_symbols` on each unused method/field to confirm zero real references
- Decide per-method:
  - `McpTool::name` — **keep**, it's fundamental; if unused, find why dispatch uses string keys instead of `.name()`
  - `McpTool::description` — **remove** if schema already carries description (likely redundant)
  - `McpTool::is_concurrency_safe` — **keep + wire** or **remove**; decide based on whether the dispatcher actually gates on it
  - `BuiltTool.{name,description,is_concurrency_safe}` fields — must match the trait decision above

**Tasks**:

- [ ] Check each unused method's call-graph (`find_referencing_symbols`)
- [ ] Decide per-symbol: **keep+wire** vs **remove**
- [ ] Write a test proving the concurrency-safety gate actually works (if keeping)
- [ ] Remove dead fields/methods OR add minimal wiring
- [ ] Confirm 0 dead_code warnings from tool.rs

**Quality Gate**:

- [ ] `cargo check -p codelens-mcp` — no warnings from tool.rs
- [ ] `cargo test -p codelens-mcp` — all existing tests pass
- [ ] If wiring added: new test proves behavior
- [ ] `onboard_project` — no new cycles

**Rollback**: revert tool.rs and optional call-site file.

**Risk**: low — isolated to one file + optional call-site.

---

## Phase 4 — 워크트리 위생 마무리 (30min)

**Goal**: gitignore the two worktree directories and lock in `.claire` exclusion with a regression test.

**Scope**:

- `.gitignore` — add `.claire/` and `.claude/worktrees/`
- `crates/codelens-engine/src/project.rs::tests` — add a unit test verifying `is_excluded` returns true for `.claire` and `.claude/worktrees`

**Tasks**:

- [ ] Append `.claire/` and `.claude/worktrees/` to `.gitignore`
- [ ] Add `#[test] fn excludes_claire_and_claude_worktrees()` in `project.rs`
- [ ] Run `git status` to confirm `?? .claire/` / `?? .claude/worktrees/` disappear

**Quality Gate**:

- [ ] `cargo test -p codelens-engine` — new test passes
- [ ] `git status` — no `??` entries for those dirs
- [ ] `cargo check` clean

**Rollback**: revert `.gitignore` and the one new test fn.

**Risk**: trivial.

---

## Phase 3 — Flaky test 근본 원인 추적 (2h)

**Goal**: identify the shared-state leak in `tests::workflow::resources_include_profile_guides_and_analysis_summaries` and make the test deterministic in parallel runs.

**Hypotheses to test**:

1. Global `OnceLock` for resource registry gets partially populated by a sibling test
2. Analysis cache key collision between tests (reused=true when not intended)
3. Env-var read during test init leaks across tests
4. Semantic ready-state toggle racing

**Scope (likely files)**:

- `crates/codelens-mcp/src/integration_tests/workflow.rs`
- `crates/codelens-mcp/src/resource_catalog.rs` (currently in uncommitted diff — must coordinate with Phase 1)
- `crates/codelens-mcp/src/resources.rs`

**Tasks**:

- [ ] Reproduce: run `cargo test -p codelens-mcp --features http` 5×; capture fail rate
- [ ] Bisect: run only workflow.rs tests vs full suite to isolate the leaker
- [ ] Use `RUST_TEST_THREADS=1` to confirm it's a concurrency issue (not time-dependent)
- [ ] Grep for `OnceLock` / `lazy_static` / static mut in touched paths
- [ ] Root-cause one of the hypotheses; fix with per-test isolation (fresh state, unique temp dir)
- [ ] Add `#[serial_test::serial]` ONLY if true per-process state (document why)
- [ ] Loop-run the suite 5× post-fix to confirm

**Quality Gate**:

- [ ] 5 consecutive `cargo test -p codelens-mcp --features http` runs all pass
- [ ] Fix is behavioral (not `#[ignore]` or serial-as-escape-hatch without justification)
- [ ] Root cause documented in memory (`feedback_flaky_resource_tests.md`)

**Rollback**: revert test isolation changes; the fix must stand on its own merits.

**Risk**: medium — may expose a deeper global-state issue; timebox to 2h and escalate if not found.

---

## Phase 1 — 미커밋 30-file diff 단계별 커밋 (2-3h, last)

**Goal**: land the uncommitted changes in reviewable logical groups, each with `--features http` passing.

**Why last**: Phases 2-4 tighten the baseline first so the diff lands on a clean, warning-free, flake-free main.

**Preparation**:

- `git status` review (current snapshot: 30 M files + 5 untracked, some already partially addressed by Phase 4)
- Group by theme:
  - **G1**: symbiote rebrand docs — `docs/adr/ADR-0007-*`, `docs/design/symbiote-*`, `docs/migrate-from-*`, `docs/design/symbiote-phase3-rename-plan.md`
  - **G2**: host-adaptive-harness docs — `docs/host-adaptive-harness.md`, `docs/harness-spec.md`, `docs/multi-agent-integration.md`, `docs/observability.md`, `docs/platform-setup.md`
  - **G3**: session/router runtime — `crates/codelens-mcp/src/server/{router,session}.rs`, `crates/codelens-mcp/src/state/{session_host,session_runtime}.rs`, `crates/codelens-mcp/src/dispatch/response.rs`
  - **G4**: surface/telemetry/resources — `crates/codelens-mcp/src/{surface_manifest,telemetry,resource_catalog,resources}.rs`, `docs/generated/surface-manifest.json`
  - **G5**: engine symbols — `crates/codelens-engine/src/symbols/mod.rs`, `crates/codelens-mcp/src/tools/symbols/handlers.rs`
  - **G6**: integration tests + misc — `crates/codelens-mcp/src/{integration_tests/workflow.rs,server/http_tests.rs,tools/query_analysis/tests.rs,tools/rules.rs,tools/session/metrics_config/*,telemetry.rs,state/*}`
  - **G7**: install/Formula — `Formula/codelens-mcp.rb`, `install.sh`
  - **G8**: benchmark/TUI/script — `benchmarks/embedding-quality-results.json`, `crates/codelens-tui/src/watch.rs`, new scripts
- For each group: stage → commit → push → verify full test suite

**Tasks**:

- [ ] Read `git diff` for each group; confirm no unintended edits
- [ ] For each group G1…G8 (in dependency order, docs first → runtime last):
  - [ ] Stage only that group
  - [ ] `cargo check && cargo test -p codelens-engine && cargo test -p codelens-mcp --features http`
  - [ ] Commit with Conventional Commits message
- [ ] Re-run `module_boundary_report` after final commit; confirm `has_cycles=false` preserved
- [ ] Re-run `impact_report` baseline (expected: 0 pending changes)

**Quality Gate**:

- [ ] Every intermediate commit builds and tests green
- [ ] No force-push; no amending public commits
- [ ] No scope expansion (only files already in the uncommitted diff)
- [ ] Post-commit `impact_report.readiness_score >= 0.85`

**Rollback**: each commit revertable via `git revert <sha>`; no destructive operations.

**Risk**: medium-high — router/transport changes (G3) have 7-file blast radius. Requires `--features http` gate per commit.

---

## Risk Assessment

| Risk                                       | Probability | Impact | Mitigation                                                               |
| ------------------------------------------ | ----------- | ------ | ------------------------------------------------------------------------ |
| Phase 2 removes needed abstraction         | Low         | Medium | Confirm zero references before deletion; wire if used elsewhere          |
| Phase 3 root cause not found in 2h         | Medium      | Low    | Timebox; if unbudgeted, park with `#[ignore]` + memory note and escalate |
| Phase 1 G3 router commit breaks http tests | Medium      | High   | Gate EVERY commit with `--features http`; split G3 further if needed     |
| Phase 1 scope creep — unrelated edits land | Medium      | Medium | Strict `git add <files>` per group; no `-A`                              |
| Cycle regression during Phase 1            | Low         | Medium | Re-run `onboard_project` after final commit                              |

## Rollback Strategy

- **Phase 2**: `git revert` single commit
- **Phase 4**: `git revert` single commit
- **Phase 3**: `git revert` single commit; flaky reverts to known state
- **Phase 1**: each G-commit revertable individually (`git revert <sha>`); no squash until end

## Notes & Learnings

_(append here per phase completion)_

- 2026-04-18: baseline established — cycle + dead-code hygiene complete, plan approved.
