# PLAN: CodeLens Opus 4.7 Harness Alignment

**Status:** in progress
**Branch:** `feat/transparency-phase1`
**Total:** 8 phases, ~24h, split across 3 tiers

**CRITICAL INSTRUCTIONS**: After completing each phase:

1. ✅ Check off completed task checkboxes
2. 🧪 Run all quality gate validation commands
3. ⚠️ Verify ALL quality gate items pass
4. 📅 Update "Last Updated" date
5. 📝 Document learnings in Notes section
6. ➡️ Only then proceed to next phase

⛔ DO NOT skip quality gates or leave a phase partially complete. Each commit must
ship a fully-working increment — no stub handlers, no TODO placeholders.

## Overview

Align CodeLens MCP with **Claude Opus 4.7 (released 2026-04-16)** and
**Anthropic 2026-04 harness engineering** (3-agent Planner/Generator/Evaluator,
Managed Agents session/harness/sandbox split, context resets via handoff
artifacts, task budgets, xhigh effort level, Tool Search canonical pattern).

Derived from the 2026-04-21 senior architecture review that surfaced 5 gaps
(recency/compression/Stage0/Stage5/tool-surface) and the subsequent Opus 4.7
fit audit that identified 4 missing model-optimization phases (MCP annotations,
handoff protocol, xhigh effort, MCP-Atlas-style benchmark).

## Priority Tiers

- **Tier A (P0, 14h)** — Opus 4.7 model-critical: O1-O4
- **Tier B (P1, 8h)** — Harness pattern alignment: O5-O7
- **Tier C (P2, 4h)** — Architecture correctness: O8

Each phase ≤ 4h, TDD red-green-refactor, independent commit.

---

## Tier A — Opus 4.7 Model-Critical

### Phase O1 — Per-symbol compression L0-L2 (P0, 3-4h)

Scope note: L3 (FullContext with callers/callees refs) is deferred to
O8b (Stage 5 structural traversal) since it reuses the same LSP/refs
infrastructure. O1 ships L0/L1/L2 fully; nothing stubbed.

- [ ] RED: `identifier_lookup_defaults_to_l1_signature_per_symbol`
- [ ] RED: `body_requested_explicit_emits_l2_per_symbol` (300-1500B per top symbol)
- [ ] RED: `symbols_beyond_cap_drop_to_l0_id_only`
- [ ] Unit: `select_presentation` mapping 4 cases
- [ ] GREEN: `enum SymbolPresentation { IdOnly, Signature, SignatureBody }`
- [ ] GREEN: `select_presentation(explicit_body, rank, cap)` in `formatter.rs`
- [ ] GREEN: `apply_presentation_per_symbol` replaces `compact_symbol_bodies`
- [ ] GREEN: `find_symbol` handler uses new function + emits `presentation_level` per symbol
- [ ] Quality Gate: `cargo test -p codelens-mcp --features semantic` pass, bench_gate primitive payload still ≤ 1100B

### Phase O2 — MCP spec annotations (P0, 2h)

- [ ] RED: `all_tool_responses_declare_max_result_size_chars`
- [ ] RED: `response_exceeding_task_budget_emits_budget_exhausted_flag`
- [ ] RED: `workflow_tools_declare_200k_limit_vs_primitive_50k`
- [ ] GREEN: `_meta["anthropic/maxResultSizeChars"]` per tool tier
- [ ] GREEN: `budget_exhausted: bool` field on truncated responses
- [ ] Quality Gate: schema round-trip test, v2.1.91+ spec compliance

### Phase O3a — Tool surface 35→12 primary + tool_search (P0, 3-4h)

Split from the original O3: ships the Anthropic-canonical pattern
(bounded default + deferred via tool_search) without the invasive
method-param consolidation. Deferred tools remain **directly
callable** so existing integration tests and external clients don't
regress — they just don't appear in the default visible set.

- [ ] RED: `default_visible_tools_stays_at_twelve_primary`
- [ ] RED: `tool_search_discovers_deferred_tool_by_keyword`
- [ ] RED: `deferred_tool_still_callable_by_name`
- [ ] GREEN: new `tool_search` tool (BM25-style over tool registry)
- [ ] GREEN: reviewer-graph profile's default visible_tools trimmed
- [ ] GREEN: all 35 tools remain in the dispatch table (no removal)
- [ ] Quality Gate: `visible_tools.tool_count == 12` for reviewer-graph

### Phase O3b — Method-param dispatch consolidators (deferred, 4h)

- [ ] `search({kind: "symbol"|"fuzzy"|"workspace"|"bm25"})` consolidation
- [ ] `analysis_job({kind: "start"|"get"|"cancel"})` consolidation
- [ ] `coordination({kind: "claim"|"release"|"list_agents"|"register"})`
- [ ] Deprecation aliases preserved until next minor

### Phase O4 — MCP-Atlas-style 2-arm benchmark (P0, 4h)

- [ ] RED: `opus_47_benchmark_runs_both_arms_on_fixture`
- [ ] RED: `with_arm_reports_success_rate_per_task`
- [ ] RED: `benchmark_writes_dated_result_to_results_dir`
- [ ] GREEN: `benchmarks/opus-47-mcp-tasks.py` (extend extreme-efficiency-eval.py)
- [ ] GREEN: 10 tasks × 2 arm (with CodeLens / native only)
- [ ] GREEN: Anthropic API call with Opus 4.7 model id
- [ ] Quality Gate: dry-run supports no-API-key, 1 live run committed

---

## Tier B — Harness Pattern Alignment

### Phase O5 — Stage 0 Dense gate for identifiers (P1, 3h) — ✅ `3797c6a`

- [x] RED: `identifier_query_emits_null_semantic_score_for_all_results`
- [x] RED: `natural_language_query_still_fires_semantic_lane`
- [x] Unit: `analyze_retrieval_query` returns `RetrievalLane::LexicalOnly`
- [x] GREEN: `RetrievalLane` enum + short-circuit `semantic_results_for_query`
- [x] Quality Gate: `cargo test -p codelens-mcp --features semantic` → 494 passed

### Phase O6 — Managed Agents handoff protocol (P1, 3h) — ✅ `db82bcf`

- [x] RED: `export_session_markdown_produces_handoff_schema_v1`
- [x] RED: `handoff_artifact_consumable_by_evaluator_primitive`
- [x] RED: `export_session_markdown_within_size_limit` (≤50KB)
- [x] GREEN: `schema_version: "codelens-handoff-v1"` field
- [x] GREEN: `docs/harness/handoff-protocol.md`
- [x] Quality Gate: `cargo test -p codelens-mcp --features semantic` → 497 passed

### Phase O7 — xhigh effort level tier (P1, 2h) — ✅ (commit follows)

- [x] RED: `xhigh_effort_level_raises_budget_multiplier`
- [x] RED: `xhigh_compression_threshold_higher_than_high`
- [x] RED: `unknown_effort_level_falls_back_to_high`
- [x] GREEN: `EffortLevel::XHigh` variant + multiplier table
- [x] Quality Gate: `cargo test -p codelens-mcp --features semantic` → 500 passed
- ⚠️ Follow-up: `budget_multiplier()` remains dead code — wire-up
  requires touching 7+ callsites (main.rs, state.rs build, set_preset,
  envelope parse, transport_http, http_tests). Deferred to a dedicated
  phase so O7 stays a single-file contract delta.

---

## Tier C — Architecture Correctness

### Phase O8a — traversal_kind + session_continuation_hint (P2, 1h) ✅

- [x] RED: `get_impact_analysis_tags_neighbors_as_graph_expansion`
- [x] RED: `session_continuation_hint_flips_on_many_blockers`
- [x] GREEN: `traversal_kind` field on `get_impact_analysis`
      (`"import_graph"` top-level + `"direct_import"`/`"graph_expansion"`
      per blast_radius entry)
- [x] GREEN: `session_continuation_hint` flips on `blockers.len() >= 3`
      on every `build_handle_payload` response

### Phase O8b — git-commit recency (P2, ~3h, **parked: requires measurement**)

- [ ] RED: `recency_uses_git_commit_time_not_filesystem_mtime`
- [ ] GREEN: `git_commit_unix_seconds` + LRU cache

Parked reason: the original justification (fs-mtime noise from
checkout/rebase) is a plausible hypothesis, not a measured regression.
Before paying the git-subprocess cost + LRU cache complexity, we need a
benchmark arm (mtime vs commit-time on a ranked retrieval dataset) that
shows MRR uplift or hit-rate stability. Unblock O8b only after that
measurement lands.

---

## Risk Matrix

| Phase | Risk                                 | Prob | Impact | Mitigation                                |
| ----- | ------------------------------------ | ---- | ------ | ----------------------------------------- |
| O1    | Breaking find_symbol body contract   | M    | H      | schema_version bump, backward-compat flag |
| O1    | L3 callers/callees cost              | M    | M      | L3 only for top-1 symbol                  |
| O2    | `_meta` change breaks client         | L    | M      | additive only                             |
| O3    | Deprecated alias regression          | M    | H      | 2-phase rollout (alias → removal)         |
| O3    | 12 primary misses use case           | M    | M      | tool_search fallback                      |
| O4    | ANTHROPIC_API_KEY absent             | H    | L      | dry-run support, CI skip                  |
| O5    | NL misclassified as identifier       | L    | M      | 2 RED coverage                            |
| O6    | Handoff schema breaking              | L    | H      | v1 locked, v2 additive                    |
| O7    | XHigh over-compression               | L    | M      | threshold rollback path                   |
| O8a   | traversal_kind mis-tag on deep graph | L    | L      | depth<=1 rule is pure, RED covers both    |
| O8b   | git subprocess fail                  | M    | L      | mtime fallback (parked — see Tier C)      |

## Rollback Strategy

All 8 phases additive + feature-flagged where invasive. Full rollback =
`git revert` single commit per phase. O1 bumps schema_version — set
`CODELENS_SYMBOL_PRESENTATION=legacy` to disable.

## Progress Tracking

- Started: 2026-04-21
- Last Updated: 2026-04-21
- Current Phase: **v1.9.52 release prep (Tier A 3/4 + Tier B 3/3 + Tier C 2/3)**
- Completed Phases: **O1, O2, O3a, O5, O6, O7, O8a** (Tier A 3/4, Tier B 3/3 ✅, Tier C 2/3)
- Parked:
  - **O4** — requires `ANTHROPIC_API_KEY` for SDK-direct 2-arm measurement;
    resumes when key is available or we swap the arm runner for Agent-tool dispatch
  - **O8b** — git-commit recency needs a measured retrieval-quality uplift
    before paying the subprocess+LRU cost; see Tier C for the gating rule

## Session Handoff Notes (2026-04-21)

**Completed this session:**

- **O1** `feat(o1): per-symbol compression L0/L1/L2` — commit `e54a6bd`.
  `SymbolPresentation { IdOnly, Signature, SignatureBody }` per-symbol
  in `tools/symbols/formatter.rs`. `find_symbol` handler emits
  `presentation_level` + `presentation_summary`. 3 RED→GREEN tests at
  `integration_tests/per_symbol_compression.rs`. L3 deferred to O8b.
- **O2** `feat(o2): primitive responses keep anthropic/maxResultSizeChars`
  — commit `48ee65c`. Primitive `_meta` now carries the MCP-spec
  annotation so Claude Code v2.1.91+ picks inline vs disk-persist
  correctly. 3 RED→GREEN tests at `integration_tests/mcp_annotations.rs`.
- **O3a** `feat(o3a): tool_search + 12-primary reviewer-graph surface` —
  NOT YET COMMITTED at handoff time. Staged files:
  - `tool_defs/presets.rs` — `REVIEWER_GRAPH_PRIMARY_TOOLS` (12),
    `primary_tools_for_surface`, `is_tool_primary_in_surface`
  - `tool_defs/mod.rs` — export `is_tool_primary_in_surface`,
    `raw_visible_tool_entries` now filters by primary set
  - `tool_defs/build.rs` — new `tool_search` Tool::new
  - `tools/session/tool_search.rs` — handler with scoring
  - `tools/session/mod.rs` — export
  - `tools/mod.rs` — dispatch entry
  - `integration_tests/tool_surface_lean.rs` — 3 RED→GREEN
  - `integration_tests/workflow.rs`,
    `integration_tests/protocol.rs` — 3 existing tests updated
    to reflect 12-primary reality

**Test counts:**

- cargo test -p codelens-mcp --features semantic → **491 passed**
  (was 488 pre-O3a, +3 new tests).

**Remaining tier A — for next session:**

- **O4**: MCP-Atlas-style 2-arm benchmark (4h).
  `benchmarks/opus-47-mcp-tasks.py` — 10 task × 2 arm (with CodeLens /
  native only) calling Opus 4.7 API. Extends
  `benchmarks/extreme-efficiency-eval.py` pattern.

**Tier B/C** (for follow-up sessions): O5 Stage-0 Dense gate, O6
Managed Agents handoff, O7 xhigh effort, O8 git-mtime+Stage5+reset.

**Known flaky tests:** `prepare_harness_session_honors_lsp_auto_opt_out_env`
(feature=http+semantic only; env var pollution; guard isolated in
`workflow_contract.rs` via LspAutoEnvGuard).

## Notes & Learnings

- Opus 4.7 `output_config.task_budget` enforces hard ceilings ⇒ per-symbol
  compression (O1) + `_meta["anthropic/maxResultSizeChars"]` (O2) are
  directly prescribed by the 2026-04 model spec, not generic
  best-practice.
- The reviewer-graph preset `REVIEWER_GRAPH_TOOLS` list (35 tools)
  stays untouched so deferred tools remain directly callable by name.
  O3a only changed the default visible set via
  `primary_tools_for_surface` + `is_tool_primary_in_surface`. This
  pattern means any future profile can opt into a primary set without
  removing tools from its callable list.
- `tool_search` uses a simple token-overlap scorer (name weight 100/20,
  description weight 3) sorted by score then name. No embedding or
  BM25 — the tool registry is small enough that O(N) linear scoring
  is trivial per call.
