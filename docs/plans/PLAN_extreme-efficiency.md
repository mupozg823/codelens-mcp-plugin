# CodeLens Extreme Efficiency Plan

**Status**: Draft — awaiting user approval
**Last Updated**: 2026-04-21
**Branch**: feat/transparency-phase1
**Preceding commit**: `3bda831` (Serena-parity + lean envelope, Phases 1-7 complete)
**Owner**: single maintainer (rotate if team size grows)

---

## ⛔ CRITICAL INSTRUCTIONS

After completing each phase:

1. ✅ Check off completed task checkboxes inline
2. 🧪 Run **all** quality gate validation commands listed for the phase
3. ⚠️ Verify **every** quality gate item passes — do not proceed on partial success
4. 📅 Update "Last Updated" date at the top
5. 📝 Append learnings to the "Notes & Learnings" section (what surprised you, what measurement changed your mind)
6. 🧾 Record the bench delta (before/after metrics) per phase
7. ➡️ Only then move to the next phase

⛔ Do not skip quality gates, merge with failing checks, or proceed when a metric regresses without rolling back or flagging the regression in the Learnings section.

---

## Overview

**Goal**: Push CodeLens from "selective checkpoint tool" to "primary agent-loop tool" — subsuming Serena's fast-lookup niche while preserving CodeLens's unique workflow intelligence. Success means an agent loop can call CodeLens continuously (not selectively) without paying prohibitive latency, bytes, or interpretation costs.

**Non-goal**: replace native Read/Grep for truly trivial work (non-code files, <30 LOC single-file edits). Those remain native.

**Reference analysis**: see upstream strategic discussion captured in session transcript (2026-04-21) — "CodeLens Extreme Efficiency Roadmap" with 6 strategic pillars.

---

## Baseline Metrics (2026-04-21)

| Metric                                                           |                   Current |            Target | Phase              |
| ---------------------------------------------------------------- | ------------------------: | ----------------: | ------------------ |
| `find_symbol` response bytes (Claude session, include_body=true) |                     2,767 |              ≤900 | P1                 |
| `find_symbol` p95 latency                                        |                     ~4 ms |            ≤10 ms | P1 (no regression) |
| `review_architecture` p95 latency (cold)                         |             **47,000 ms** |         ≤3,000 ms | P2                 |
| `review_architecture` p95 latency (warm cache)                   |            n/a (no cache) |           ≤500 ms | P2                 |
| `impact_report` p95 latency                                      |                 ~1,200 ms |           ≤500 ms | P2                 |
| `prepare_harness_session` p95 latency                            |                   ~300 ms |           ≤150 ms | P2                 |
| `tools/list` default bytes                                       |                    54,429 |           ≤30,000 | P1                 |
| `semantic_ready` cross-tool consistency                          | partial mismatch observed |              100% | P3                 |
| `mutation_ready=caution` responses w/ actionable `blockers[]`    |                        0% |              ≥95% | P4                 |
| `next_actions` entries with executable `command` field           |                        0% |              ≥90% | P4                 |
| Parallel 5-agent mutation collision rate                         |                unmeasured | 0 per 1k requests | P5                 |
| Serena vs CodeLens bytes ratio (primitive mode)                  |                      4.2x |             ≤1.5x | P1                 |
| 10-task token cost vs current                                    |                  baseline |              −40% | P6                 |

Baseline measurements recorded in `/tmp/direction-analysis/` and `/tmp/mcp-bench-p5/`; reproducible via `benchmarks/serena-vs-codelens-efficiency.py` after binary rebuild.

---

## Architecture Decisions

### D-1: Response shape tiering

Three response shapes emitted per tool, selectable via `detail` parameter:

- `detail="primitive"` — Serena-class bytes (≤ 1.5× Serena). Drop all orchestration scaffolds, `_meta`, `structuredContent` duplication, pretty-print. Keep body, container, snippet, basic status.
- `detail="core"` (existing Phase 5 "lean") — include signals and `suggested_next_tools` list; drop Codex delegation scaffold, suggestion rationales.
- `detail="rich"` (existing) — full envelope for workflow/orchestration consumers.

Routing: lookup tools default to `primitive` for Claude Code clients, `core` otherwise; workflow tools default to `rich`.

### D-2: Latency cache architecture

Introduce `WorkflowAnalysisCache` keyed on `(tool_name, canonical_input, content_hash)`:

- Content hash = blake3 of tracked file set (git-tracked + dirty fs-watcher set).
- TTL 5 minutes; invalidated on fs-watcher dirty events.
- Cold miss triggers background compute, returns stale (`staleness_ms`) if available or blocks with deadline 3 s for `review_architecture`, 500 ms for `impact_report`.
- Shared across sessions (process-global), not per-session.

### D-3: Prescriptive blockers

`readiness` stays a 4-axis enum (ready / caution / blocked) but adds sibling `blockers: [Blocker]` array. `Blocker` schema:

```rust
struct Blocker {
    kind: BlockerKind,          // blast_radius_high | caller_count_high | diagnostics_present | test_target_missing | custom
    file: Option<String>,
    line: Option<usize>,
    symbol: Option<String>,
    consumers: Option<usize>,
    required_action: String,    // imperative sentence
    command: Option<String>,    // exact shell invocation if applicable
    severity: Severity,
    auto_resolvable: bool,
}
```

`caution` with empty `blockers` is forbidden (test-enforced).

### D-4: State unification — single embedding status source

`AppState::embedding_status()` returns `EmbeddingStatus { loaded, indexed_symbols, model, last_refresh_at }`. All tools reading semantic lane health use this — no local `is_indexed()` calls from handler code. `review_architecture` and `get_ranked_context` emit the same status object.

### D-5: Parallel primitives via shared analysis pool

Analysis cache from D-2 exposes `pool_snapshot()` in `prepare_harness_session` response so sibling agents can reuse existing analyses by id. `claim_files` gets a 10 min TTL with heartbeat-renewal.

### D-6: Routing policy as code

Routing rules live in `crates/codelens-mcp/src/routing_policy.rs` as a single declarative table. CLAUDE.md section is generated from this table via `scripts/generate-routing-policy.py` (pre-commit hook enforces regeneration).

---

## Phase Breakdown

**Large scope** plan per feature-planner guidelines: 7 phases. Each phase 8–24 hours (Rust work), total ~80 hours real-time. Every phase must ship working binary, passing tests, updated benchmarks.

---

### Phase 0: Bench harness hardening (blocker for all measurement)

**Goal**: `benchmarks/serena-vs-codelens-efficiency.py` produces a JSON artifact usable as CI gate; baseline numbers locked into the plan.

**Test Strategy**: Python bench harness currently hand-run. Add a `cargo test` wrapper that:

- Spawns a test CodeLens server on ephemeral port
- Initializes a session, runs the 4 benchmark tasks
- Asserts `response_bytes` + `response_time_ms` against baseline thresholds stored in `benchmarks/baselines/extreme-efficiency.json`

**Test location**: `crates/codelens-mcp/tests/bench_gate.rs` (new, integration test under `tests/` dir).

**RED tasks**:

- [ ] Write `bench_gate_asserts_find_symbol_under_3kb_baseline` (will fail against real server until cache seeded — OK for RED)
- [ ] Write `bench_gate_asserts_review_architecture_under_60s_baseline` (lax, guardrail against regression)
- [ ] Write `bench_gate_asserts_cross_tool_semantic_ready_consistency` (will FAIL today — locks in Phase P3 target)

**GREEN tasks**:

- [ ] Implement ephemeral-port server spawn helper (reuse `test_helpers::`)
- [ ] Parse Python script JSON output
- [ ] Wire thresholds from baseline JSON file

**REFACTOR tasks**:

- [ ] Extract bench assertion helpers for reuse in Phases P1-P6
- [ ] Add `--update-baseline` flag (admin-only) to rewrite baselines

**Coverage target**: the 3 RED tests above; no percentage metric (infra phase).

**Quality Gate**:

- [ ] `cargo test --test bench_gate` runs in <5 minutes
- [ ] `benchmarks/baselines/extreme-efficiency.json` committed with current measured values
- [ ] CI workflow runs bench_gate on PR; failure blocks merge
- [ ] `cargo clippy -p codelens-mcp --tests` 0 warnings

**Dependencies**: none

**Rollback**: delete `tests/bench_gate.rs` + baselines json + CI workflow change

**Estimated effort**: 6 h

---

### Phase P1: Primitive response mode (Pillar 1)

**Goal**: lookup tools emit ≤1.5× Serena bytes under `detail="primitive"`, defaulting to primitive for Claude Code clients.

**Test Strategy**: Integration tests in `crates/codelens-mcp/src/integration_tests/readonly.rs` + `lsp.rs` validate byte and field contracts. Add unit tests in `dispatch/response_support.rs` for the detail classifier.

**RED tasks**:

- [ ] `find_symbol_primitive_mode_matches_serena_byte_envelope` — expects < 900 bytes for single-symbol body response (will fail: current lean is 2,767 b)
- [ ] `find_symbol_primitive_mode_drops_structured_content` — assert `structuredContent is None` (will fail)
- [ ] `find_symbol_primitive_mode_preserves_body_and_container` — invariants preserved
- [ ] `lookup_tools_default_detail_primitive_for_claude_code_client` — envelope parsing test (will fail: no classifier yet)
- [ ] `workflow_tools_keep_rich_detail_by_default` — negative assertion on impact_report

**GREEN tasks**:

- [ ] Add `DetailLevel { Primitive, Core, Rich }` enum to `dispatch/envelope.rs`
- [ ] Extend `is_lean_default_tool` → `default_detail_level(tool, client_profile)` mapping
- [ ] Add `compact_primitive_payload(resp)` in `response_support.rs` — drops `_meta`, `structuredContent`, scaffold, metadata
- [ ] Wire into `build_success_response` path
- [ ] Teach `response_support::text_payload_for_response` to emit compact-print JSON under primitive
- [ ] Add `ClientProfile::Claude` → primitive default for lookup tools (but preserve `_compact`/`detail` arg override)

**REFACTOR tasks**:

- [ ] Collapse Phase 5 `_compact` plumbing into `detail` (deprecate `_compact`, keep as alias for 1 release)
- [ ] Add `DetailLevel::from_args` helper
- [ ] Document the three levels in `docs/response-contract.md`

**Coverage target**: 8 new tests (5 RED, 3 additional around detail classifier edge cases). Overall MCP crate coverage must not decrease.

**Quality Gate**:

- [ ] `cargo test -p codelens-mcp` all pass
- [ ] `cargo test -p codelens-mcp --features http` all pass
- [ ] `cargo clippy --all-features` 0 warnings
- [ ] `cargo test --test bench_gate` — primitive baseline < 900 b pass
- [ ] Live A/B run: `python3 benchmarks/serena-vs-codelens-efficiency.py` T1 `find_symbol` bytes ≤ 900
- [ ] Existing Serena-parity tests (Phases 1–7) still pass
- [ ] Update `PLAN_extreme-efficiency.md` metrics table with post-P1 numbers
- [ ] Commit message references baseline delta (e.g. `find_symbol 2767→873b (-68%)`)

**Dependencies**: P0 (baselines in CI gate)

**Rollback**:

- revert envelope.rs default changes
- `_compact` alias remains; `detail` param becomes no-op but not an error

**Estimated effort**: 10 h

---

### Phase P2: Latency floor via `WorkflowAnalysisCache` (Pillar 2)

**Goal**: `review_architecture` p95 ≤ 3 s cold, ≤ 500 ms warm. `impact_report` p95 ≤ 500 ms. `prepare_harness_session` p95 ≤ 150 ms.

**Test Strategy**: unit tests on the cache struct (hit/miss/invalidate), integration tests on tool wrappers measure wall-clock. Bench-gate adds latency thresholds.

**RED tasks**:

- [ ] `workflow_cache_returns_fresh_on_hit_without_recompute` — cache backed by a `FakeComputeFn` counter; second call count == 1
- [ ] `workflow_cache_invalidates_on_fs_watcher_dirty_event`
- [ ] `workflow_cache_returns_stale_with_staleness_ms_metadata_when_recompute_pending`
- [ ] `review_architecture_cold_under_3_seconds` — spawns ephemeral server, imports small synthetic corpus, asserts latency budget
- [ ] `review_architecture_warm_under_500ms`
- [ ] `impact_report_warm_under_500ms`

**GREEN tasks**:

- [ ] Implement `WorkflowAnalysisCache` under `crates/codelens-mcp/src/state/workflow_cache.rs` with dashmap backing
- [ ] Cache key = `(tool_name, canonical_args_hash, project_file_set_hash)`
- [ ] Add project file-set hasher using blake3 over indexed file paths + mtimes
- [ ] Wire `review_architecture` handler to cache
- [ ] Wire `impact_report` handler to cache
- [ ] Add `staleness_ms` to response `_meta` when stale returned
- [ ] Subscribe to existing fs-watcher channel for dirty events
- [ ] Add incremental PageRank hook for `review_architecture` (fallback to full recompute if incremental disagrees > 0.01 MRR on regression fixture)

**REFACTOR tasks**:

- [ ] Extract cache metric reporting (hit_rate, avg_compute_time) into `state::metrics_host`
- [ ] Add `_meta.cache_refresh: "pending"` when background refresh triggered
- [ ] Publish telemetry counter `codelens.cache.hit` / `codelens.cache.miss`

**Coverage target**: 6 new tests + coverage for incremental PageRank agreement (existing engine test corpus). `state::workflow_cache` module at ≥ 85 %.

**Quality Gate**:

- [ ] Incremental vs full PageRank MRR diff ≤ 0.01 on `benchmarks/self-small.json`
- [ ] `cargo bench --bench workflow_cache` (new) shows cache hit < 10 ms
- [ ] `cargo test -p codelens-mcp` — all pass
- [ ] `cargo test -p codelens-engine` — 300 pass (no regression)
- [ ] Live A/B: `review_architecture` p95 ≤ 3 s cold on this repo
- [ ] Live A/B: second call ≤ 500 ms
- [ ] No memory leak: 1000-call cache churn test peak RSS ≤ 200 MB
- [ ] Update baseline JSON with new latency thresholds

**Dependencies**: P0, P1 (primitive mode reduces baseline test payload size)

**Rollback**: feature-flag cache via env `CODELENS_WORKFLOW_CACHE=0`; disabling reverts to pre-P2 behavior

**Estimated effort**: 20 h (largest phase)

---

### Phase P3: State unification (Pillar 4 — before P4 because signals need it)

**Goal**: every tool reading embedding/index state sees the same `EmbeddingStatus`.

**Test Strategy**: cross-tool consistency test calling `review_architecture` and `get_ranked_context` in same session, asserting their reported semantic readiness match.

**RED tasks**:

- [ ] `review_architecture_and_get_ranked_context_report_identical_semantic_status`
- [ ] `embedding_status_snapshot_is_pinned_during_tool_call` (atomicity)
- [ ] `embedding_status_visible_in_prepare_harness_session_response`

**GREEN tasks**:

- [ ] Add `AppState::embedding_status()` returning `EmbeddingStatus` struct (one source)
- [ ] Replace 3 existing ad-hoc calls (`analyzer::semantic_lane_ready`, `review_architecture` inline, `prepare_harness_session` inline)
- [ ] Expose `state_scope: "project"` tag on the emitted status blocks

**REFACTOR tasks**:

- [ ] Remove dead `semantic_lane_ready` free function once call sites converted
- [ ] Document state scope matrix in `docs/response-contract.md`

**Coverage target**: 3 new integration tests; `embedding_status` helper ≥ 95 % branch coverage.

**Quality Gate**:

- [ ] cross-tool consistency test pass
- [ ] `cargo test -p codelens-mcp` — all pass
- [ ] Live run: `review_architecture.data.semantic.loaded == get_ranked_context.retrieval.semantic_ready`
- [ ] CI bench gate adds consistency check

**Dependencies**: P1 (envelope may change during primitive work)

**Rollback**: keep new helper but revert call-site changes via feature gate

**Estimated effort**: 6 h

---

### Phase P4: Prescriptive signals (Pillar 3)

**Goal**: 95 % of `mutation_ready=caution` responses carry actionable `blockers[]`; 90 % of `next_actions` include `command` field.

**Test Strategy**: scenario-based tests feed impact_report with known-high-blast-radius files (e.g., `output_schemas.rs`) and assert blockers mention specific consumers + suggested `cargo test` command.

**RED tasks**:

- [ ] `impact_report_caution_on_high_blast_radius_emits_blockers_with_consumer_count`
- [ ] `impact_report_blockers_reference_actual_dependent_file_paths`
- [ ] `next_actions_entries_include_executable_command_when_known`
- [ ] `verifier_checks_include_pass_condition_with_command`
- [ ] `readiness_rationale_explains_caution_reason`

**GREEN tasks**:

- [ ] Extend `state::AnalysisReadiness` with `blockers: Vec<Blocker>` + `rationale: HashMap<Axis, String>`
- [ ] Teach `impact_report` handler to synthesize blockers from existing impact_rows (consumer counts, diagnostics presence, test-target absence)
- [ ] Teach `verify_change_readiness` to emit matching blockers
- [ ] Update `tool_defs::output_schemas::impact_output_schema` with the new fields
- [ ] Backfill `command` suggestions: cargo test invocations keyed on touched crate

**REFACTOR tasks**:

- [ ] Extract blocker-synthesizer into `state::readiness::blockers` submodule
- [ ] Document blocker kinds enum in schema (JSON `enum`)
- [ ] Backport blocker field emission to `review_changes` + `summarize_symbol_impact`

**Coverage target**: 5 new scenario tests + coverage of new blocker synthesizer ≥ 85 %.

**Quality Gate**:

- [ ] `cargo test -p codelens-mcp` — all pass
- [ ] Real scenario: `impact_report` on `crates/codelens-mcp/src/tool_defs/output_schemas.rs` emits blockers naming specific consumer files (e.g., `tool_defs/build.rs`) + `command: "cargo test -p codelens-mcp workflow_contract"`
- [ ] Bench gate: 10 synthetic `caution` scenarios → 95 % carry non-empty `blockers[]`
- [ ] 90 % of `next_actions` entries across 10 scenarios have non-null `command`
- [ ] Update schema doc + JSON schemas

**Dependencies**: P3 (state unification) and P2 (readiness computation must be fast post-cache)

**Rollback**: blockers vec defaults to empty; rollback = revert synthesizer module

**Estimated effort**: 16 h

---

### Phase P5: Parallel-agent primitives (Pillar 5)

**Goal**: 5 parallel agents can run without mutation collision, sharing analyses via pool.

**Test Strategy**: integration tests simulate 5 concurrent sessions calling impact_report + mutation tools. Use tokio test runtime + barrier synchronization.

**RED tasks**:

- [ ] `five_parallel_sessions_complete_without_collision` (TTL-based locks)
- [ ] `analysis_pool_snapshot_is_visible_in_prepare_harness_session`
- [ ] `agent_can_reuse_peer_analysis_id_without_recompute` (round-trip count = 1 for second agent)
- [ ] `claim_files_auto_release_after_ttl_when_heartbeat_missing`

**GREEN tasks**:

- [ ] Expose `AnalysisCache::pool_snapshot()` returning `Vec<PoolEntry { id, tool, created_ms, files_touched }>`
- [ ] Wire into `prepare_harness_session` response as `shared_analysis_pool`
- [ ] Add TTL + heartbeat to `claim_files` store (module `agent_coordination`)
- [ ] Broadcast cache invalidation to sibling sessions on mutation success

**REFACTOR tasks**:

- [ ] Tag cache entries with originating session id for audit
- [ ] Add metrics: `codelens.agent.collisions_avoided`, `codelens.agent.pool_reuses`

**Coverage target**: 4 new concurrency tests + stress test (100 iters).

**Quality Gate**:

- [ ] `cargo test -p codelens-mcp --features http` — all pass under tokio test runtime
- [ ] 100-iter concurrency stress test: 0 collisions, 0 deadlocks
- [ ] Live 5-agent simulation script (`benchmarks/parallel_agent_simulation.py`): 0 mutation collisions across 1000 operations
- [ ] TTL-expired claims auto-released within 60 s of heartbeat silence

**Dependencies**: P2 (shared cache), P3 (state consistency across agents), P4 (blockers inform collision avoidance)

**Rollback**: gate behind `CODELENS_PARALLEL_PRIMITIVES=1` env flag; disabling reverts to per-session behavior

**Estimated effort**: 18 h

---

### Phase P6: Routing policy + eval harness (Pillar 6)

**Goal**: routing decisions for "CodeLens vs native vs Serena" live as code; 10-task eval shows ≥ 40 % token reduction vs baseline.

**Test Strategy**: eval harness launches 3 arms (native-only, CodeLens-only, hybrid-policy) over 10 synthetic tasks, asserts hybrid wins on composite metric.

**RED tasks**:

- [ ] `routing_policy_table_compiles_to_markdown_identical_to_checked_in_file`
- [ ] `eval_hybrid_arm_tokens_below_native_only`
- [ ] `eval_hybrid_arm_time_below_codelens_only`
- [ ] `eval_hybrid_arm_correctness_ge_max_of_other_arms`

**GREEN tasks**:

- [ ] Create `crates/codelens-mcp/src/routing_policy.rs` with declarative rule table
- [ ] Write `scripts/generate-routing-policy.py` to regen CLAUDE.md section
- [ ] Add pre-commit hook (`scripts/pre-commit-routing-policy.sh`) enforcing regeneration
- [ ] Build eval harness `benchmarks/extreme-efficiency-eval.py` (10 task scenarios, 3 arms)
- [ ] Update `~/.claude/codelens-routing-policy.md` with generated content

**REFACTOR tasks**:

- [ ] Extract scenario fixtures into `benchmarks/scenarios/` JSON files for reuse
- [ ] Publish eval results as `benchmarks/results/extreme-efficiency-YYYY-MM-DD.md`

**Coverage target**: 4 new gating tests; eval harness treated as CI job (not coverage metric).

**Quality Gate**:

- [ ] Routing table test pass (markdown regen idempotent)
- [ ] Pre-commit hook active
- [ ] Eval harness runs in < 10 minutes
- [ ] Hybrid arm beats both others on composite score (`tokens * time / correctness^2`)
- [ ] Publish result doc and link in `benchmarks/README.md`

**Dependencies**: P0–P5 complete and stable

**Rollback**: routing table stays; revert CLAUDE.md generation hook — table remains advisory

**Estimated effort**: 12 h

---

## Total estimate: ~88 h (7 phases × 1–20 h each)

At ~10 h/week, ~9 weeks. At ~20 h/week, ~4–5 weeks.

---

## Risk Assessment

| Risk                                                                                    | Probability | Impact | Mitigation                                                                           |
| --------------------------------------------------------------------------------------- | ----------- | ------ | ------------------------------------------------------------------------------------ |
| Incremental PageRank diverges from full PageRank (P2.1)                                 | M           | H      | Agreement gate ≤ 0.01 MRR diff; fallback to full recompute on fixture regression     |
| Primitive mode (P1) breaks newer MCP clients that rely on `structuredContent`           | M           | M      | Detect client capabilities; polyfill `structuredContent` when required               |
| Cache invalidation misses (P2) leading to stale readiness                               | M           | H      | TTL default 5 min; fs-watcher belt-and-suspenders; `staleness_ms` visible in `_meta` |
| Prescriptive blockers (P4) produce noise that re-introduces interpretation cost         | M           | M      | Severity threshold; dedupe on (kind, file); 95 % actionability gate in CI            |
| Parallel primitives (P5) introduce race conditions                                      | L           | H      | tokio test harness with 100-iter stress + miri if feasible                           |
| Routing policy table (P6) drifts from CLAUDE.md                                         | M           | L      | pre-commit regeneration hook; CI fails on diff                                       |
| Scope creep — 88 h overshoots to 150 h+                                                 | H           | M      | Phase-level rollback + explicit "Pause after phase" checkpoints                      |
| Binary deployment friction (codesign, launchctl) slows iteration                        | M           | L      | `scripts/dev-deploy.sh` wrapper for rebuild → codesign → kickstart → health check    |
| Existing advisory tests (e.g. readiness=caution w/ empty blockers) must be updated (P4) | H           | L      | Sweep tests in Phase P4 RED stage; fail loudly                                       |

---

## Rollback Strategy Summary

Each phase is independently revertible:

- **P0**: delete tests/bench_gate.rs + baselines + CI workflow
- **P1**: `detail` arg becomes no-op; revert envelope default
- **P2**: `CODELENS_WORKFLOW_CACHE=0` flag
- **P3**: revert call-site conversions; helper retained for future
- **P4**: blockers vec empty; revert synthesizer module
- **P5**: `CODELENS_PARALLEL_PRIMITIVES=1` flag off
- **P6**: revert CLAUDE.md generation; routing table file remains

Every phase ends in a commit; rollback = `git revert <phase commit>`.

---

## Progress Tracking

- [ ] Phase 0 — Bench harness hardening
- [ ] Phase P1 — Primitive response mode
- [ ] Phase P2 — Latency floor via WorkflowAnalysisCache
- [ ] Phase P3 — State unification
- [ ] Phase P4 — Prescriptive signals
- [ ] Phase P5 — Parallel-agent primitives
- [ ] Phase P6 — Routing policy + eval harness

Each phase entry above gets:

- ✅ check when all sub-tasks done and Quality Gate passes
- 📝 learnings appended below

---

## Notes & Learnings

### Learning log (append after each phase)

_…to be populated during execution._

### Pre-execution observations (2026-04-21)

- `benchmarks/` directory is already 3-4× larger than the source in symbol count — P6 eval harness should live in a sub-folder and be clearly tagged to avoid adding more stale data.
- `review_architecture`'s 47 s cold time is dominated by full PageRank over 47 directories; the import graph is already available via `import_graph` module — cache the graph, not the result.
- `impact_report` 32-file blast radius on `output_schemas.rs` surfaced in session direction analysis — Phase P4's blocker synthesizer can test against this exact fixture.
- Serena has no workflow tools; hybrid routing rule in P6 lists Serena only as fallback for `find_symbol` when CodeLens fails to start.
- Phase P5 parallel primitives rely on `agent_coordination` module which already exists but is under-tested (5 existing tests). P5 RED stage should lift coverage there first.

---

## Appendix A — Measurement Commands

```bash
# Rebuild + deploy + health check (reused every phase)
cargo build --release -p codelens-mcp --features http \
  && cp target/release/codelens-mcp .codelens/bin/codelens-mcp-http \
  && codesign --force --deep --sign - .codelens/bin/codelens-mcp-http \
  && launchctl kickstart -k gui/$(id -u)/dev.codelens.mcp-readonly

# Bench run (Phase 0 onwards)
python3 benchmarks/serena-vs-codelens-efficiency.py \
  > benchmarks/results/extreme-efficiency-$(date +%F).json

# Cross-tool state consistency check (Phase 3)
python3 benchmarks/state-consistency-check.py

# Parallel agent collision test (Phase 5)
python3 benchmarks/parallel_agent_simulation.py --agents 5 --ops 1000

# Routing policy regen (Phase 6)
python3 scripts/generate-routing-policy.py --write
```

## Appendix B — CI gate wiring

`.github/workflows/ci.yml` additions (suggested):

```yaml
- name: bench_gate
  run: cargo test --test bench_gate -- --include-ignored

- name: state_consistency
  run: python3 benchmarks/state-consistency-check.py --max-drift 0

- name: extreme_efficiency_eval
  if: github.event_name == 'push' && github.ref == 'refs/heads/main'
  run: python3 benchmarks/extreme-efficiency-eval.py --fail-on-regression
```

Only `bench_gate` and `state_consistency` gate PRs; `extreme_efficiency_eval` runs on main merges only (expensive).
