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

### Phase O1 — Per-symbol compression L0-L3 (P0, 4h)

- [ ] RED: `identifier_lookup_emits_l0_or_l1_per_symbol` (avg ≤ 300B)
- [ ] RED: `body_requested_explicit_emits_l2_per_symbol` (per-symbol 300-1500B)
- [ ] RED: `impact_report_embeds_l3_for_top_blast_radius_symbol`
- [ ] Unit: `SymbolPresentation::from_intent` mapping 4 cases
- [ ] GREEN: `enum SymbolPresentation { IdOnly, Signature, SignatureBody, FullContext }`
- [ ] GREEN: `select_presentation(intent, explicit_body, rank)` in `formatter.rs`
- [ ] GREEN: rewrite `compact_symbol_bodies` → `apply_presentation_per_symbol`
- [ ] GREEN: wire 3 handlers to pass intent
- [ ] Quality Gate: `cargo test -p codelens-mcp --features semantic` pass

### Phase O2 — MCP spec annotations (P0, 2h)

- [ ] RED: `all_tool_responses_declare_max_result_size_chars`
- [ ] RED: `response_exceeding_task_budget_emits_budget_exhausted_flag`
- [ ] RED: `workflow_tools_declare_200k_limit_vs_primitive_50k`
- [ ] GREEN: `_meta["anthropic/maxResultSizeChars"]` per tool tier
- [ ] GREEN: `budget_exhausted: bool` field on truncated responses
- [ ] Quality Gate: schema round-trip test, v2.1.91+ spec compliance

### Phase O3 — Tool surface 35→12 primary (P0, 4h)

- [ ] RED: `default_visible_tools_stays_under_twelve_primary`
- [ ] RED: `method_param_dispatch_covers_deferred_tools`
- [ ] RED: `tool_search_surfaces_deferred_tools`
- [ ] GREEN: 12 primary set in surface_manifest
- [ ] GREEN: method-param wrappers (search/analysis_job/coordination)
- [ ] GREEN: deprecation aliases for backward-compat
- [ ] Quality Gate: `visible_tools.tool_count == 12` for reviewer-graph

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

### Phase O5 — Stage 0 Dense gate for identifiers (P1, 3h)

- [ ] RED: `identifier_query_emits_null_semantic_score_for_all_results`
- [ ] RED: `natural_language_query_still_fires_semantic_lane`
- [ ] Unit: `analyze_retrieval_query` returns `RetrievalLane::LexicalOnly`
- [ ] GREEN: `RetrievalLane` enum + short-circuit `semantic_results_for_query`

### Phase O6 — Managed Agents handoff protocol (P1, 3h)

- [ ] RED: `export_session_markdown_produces_handoff_schema_v1`
- [ ] RED: `handoff_artifact_consumable_by_evaluator_primitive`
- [ ] RED: `export_session_markdown_within_size_limit` (≤50KB)
- [ ] GREEN: `schema_version: "codelens-handoff-v1"` field
- [ ] GREEN: `docs/harness/handoff-protocol.md`

### Phase O7 — xhigh effort level tier (P1, 2h)

- [ ] RED: `xhigh_effort_level_raises_budget_multiplier`
- [ ] RED: `xhigh_compression_threshold_higher_than_high`
- [ ] RED: `unknown_effort_level_falls_back_to_high`
- [ ] GREEN: `EffortLevel::XHigh` variant + multiplier table

---

## Tier C — Architecture Correctness

### Phase O8 — recency + Stage 5 split + reset hint (P2, 4h)

- [ ] RED: `recency_uses_git_commit_time_not_filesystem_mtime`
- [ ] RED: `get_impact_analysis_tags_neighbors_as_graph_expansion`
- [ ] RED: `session_continuation_hint_flips_on_many_blockers`
- [ ] GREEN: `git_commit_unix_seconds` + LRU cache
- [ ] GREEN: `TraversalKind` enum + `traversal_kind` field on responses
- [ ] GREEN: `session_continuation_hint` logic

---

## Risk Matrix

| Phase | Risk                               | Prob | Impact | Mitigation                                |
| ----- | ---------------------------------- | ---- | ------ | ----------------------------------------- |
| O1    | Breaking find_symbol body contract | M    | H      | schema_version bump, backward-compat flag |
| O1    | L3 callers/callees cost            | M    | M      | L3 only for top-1 symbol                  |
| O2    | `_meta` change breaks client       | L    | M      | additive only                             |
| O3    | Deprecated alias regression        | M    | H      | 2-phase rollout (alias → removal)         |
| O3    | 12 primary misses use case         | M    | M      | tool_search fallback                      |
| O4    | ANTHROPIC_API_KEY absent           | H    | L      | dry-run support, CI skip                  |
| O5    | NL misclassified as identifier     | L    | M      | 2 RED coverage                            |
| O6    | Handoff schema breaking            | L    | H      | v1 locked, v2 additive                    |
| O7    | XHigh over-compression             | L    | M      | threshold rollback path                   |
| O8    | git subprocess fail                | M    | L      | mtime fallback                            |

## Rollback Strategy

All 8 phases additive + feature-flagged where invasive. Full rollback =
`git revert` single commit per phase. O1 bumps schema_version — set
`CODELENS_SYMBOL_PRESENTATION=legacy` to disable.

## Progress Tracking

- Started: 2026-04-21
- Last Updated: 2026-04-21
- Current Phase: **O1 (pending)**
- Completed Phases: _none_

## Notes & Learnings

_To be filled in as phases complete._
