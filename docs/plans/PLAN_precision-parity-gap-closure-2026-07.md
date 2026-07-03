# Implementation Plan: Precision Parity & Gap Closure (next phase, 2026-07)

**Status**: 📋 Proposed
**Source**: 5-lens adversarial architecture eval vs Serena (2026-07-03) × open backlog cross-map
**Verdict being answered**: precision Serena 8.5 > CodeLens 6.5 (maturity gap, not ceiling);
evolution Serena 8.0 > 6.5 (language marginal cost = structural); economics/ops/governance
CodeLens 8.5 already superior — maintain, don't over-invest.

**Strategy**: invest where we LOSE (precision, language economics), finish where we WIN
(governance/ops closure items), and never regress the axes already won (lean contract,
MRR/perf gates stay mandatory).

---

## P0 — Deploy hygiene (immediate, <1h)

| Item | Seam / issue | Acceptance |
|---|---|---|
| Merge + deploy #358 fix (dot-dir zero-file indexing) | branch `fix/root-relative-excludes-358` @0805aa36, already green | PR→CI→merge→`redeploy-daemons.sh --build --probe`; proof-gin e2e indexes >0 files |
| Bootstrap race retry | #356; `scripts/redeploy-daemons.sh` | script retries `bootstrap`+`kickstart -k` on error 5; 3 consecutive redeploys succeed unattended |

## P1 — Precision parity (highest leverage; the one axis we lose on product identity)

Goal: "delivered precision" ≥ Serena on the default path. Target score 6.5 → 8+.

1. **LSP client protocol parity** — `crates/codelens-engine/src/lsp/session.rs`
   - Answer server→client requests (`workspace/configuration`, `client/registerCapability`,
     `window/workDoneProgress`) instead of discarding (session.rs:535-578).
   - Per-server `initializationOptions`/settings table (start: rust-analyzer, pyright, ts-ls, gopls).
   - Quiescence readiness handshake: rust-analyzer `experimental/serverStatus` quiescent;
     equivalent signals per server; replace `CODELENS_LSP_STARTUP_GRACE_MS` blind sleep.
   - Acceptance: requests during indexing wait for quiescent (integration test with rust-analyzer);
     zero protocol-violation discards in a warm session trace.
2. **Confidence calibration** — closes #295 (11-day stale SCIP still 0.95) + panel condition 4
   - SCIP staleness + LSP readiness feed `confidence_basis`; stale precise tier caps ≤0.6
     with explicit `degraded_reason`.
   - Acceptance: regression test — stale index.scip vs newer source ⇒ confidence ≤0.6 + warning.
3. **Python default-path precise routing** — the one MEASURED recall gap (imports/type annotations)
   - Option A: opt-in pre-warm LS pool (daemon detects project languages at bind → warms pyright);
     Option B: scip-python auto-generation (see P2.1). Ship A first (smaller), B supersedes.
   - Acceptance: Python `find_referencing_symbols` returns import + annotation references on the
     DEFAULT path; add benchmark dataset asserting it (recall floor in CI).

## P2 — SCIP multi-language + language-scaling economics (structural weakness)

1. **SCIP auto-generation pipeline**: language detect → run indexer (scip-python /
   scip-typescript / rust-analyzer scip) → partial publish → mtime freshness watch.
   Today: load-only, generation script Rust-only. Interlocks with #295/#298 (hot-reload
   session invalidation must not lock out clients).
2. **Grammar ABI unblock**: tree-sitter 0.25→0.26 unified upgrade releases the 5 blocked
   languages (make/dockerfile/vim/fsharp/perl — `lang_config.rs:85`); evaluate dynamic
   grammar loading (libloading) as the durable fix for the ABI lattice class.
3. **New-language policy pivot**: LSP/SCIP-first for semantics of any NEW language; freeze
   growth of per-language regex import extractors (`import_graph/parsers.rs`). Pipeline a
   dataset generator so each new language lands WITH an embedding-quality dataset + MRR floor.

## P3 — Governance closure (win the axis fully; small)

1. **Secure-by-default**: mutation-capable daemon without `principals.toml` → startup warning;
   `install-http-daemons-launchd.sh --principals-scaffold` writes a starter file
   (planner=ReadOnly / builder=Refactor cross-principal split as the documented default).
2. **#347 promotion**: unbound-project calls on a shared daemon warn→block (advisory hint today).

## P4 — Ops/scale closure

1. Watcher start failure: silent `.ok()` degrade → explicit `watcher_unavailable` warning in
   bootstrap + freshness path.
2. Direct 100K-file benchmark (39K measured today — confirm the extrapolated claim once).
3. #300/#301: session token survives daemon restart → graceful re-init instead of
   'Unknown session' lockout.
4. #342: analysis cache invalidation on file move + redeploy.

## P5 — Mutation tier completion (extend the verified safety lead)

1. **pending-D3 promotion decision**: `symbolic_edit_core` 4 tools + `refactor_substrate`
   5 tools out of allowlisted_dispatch_only — together with #287 (LSP code-action backends
   for replace_symbol_body / insert_*_symbol), i.e. mutation goes LSP-first too.
2. **#341 hash-anchored semantic_edit** — widens the defensive-mutation lead the eval
   already credited (old_text verify + overlap reject + dry-run).

---

## Sequencing & effort

| Phase | Effort | Depends on |
|---|---|---|
| P0 | <1h | — |
| P1 | 3–5 sessions | P0 (deployed baseline) |
| P2 | 2–3 sessions | P1.3 informs P2.1 |
| P3 | 1 session | — (parallel-safe) |
| P4 | 1–2 sessions | — (parallel-safe) |
| P5 | 2 sessions | P1.1 (LSP parity groundwork) |

Quality gates per phase: existing infra (fmt/clippy matrix, nextest, regen/surface drift,
MRR floor + benchmark CI). New: Python reference-recall dataset (P1.3), stale-SCIP
confidence regression (P1.2), quiescence integration test (P1.1).

## Non-goals (explicitly deferred)

- Tool-count growth: 93 tools is already past the "consolidate" guidance; new capability
  should land inside existing workflow entrypoints, not as new top-level tools.
- Re-litigating won axes: lean contract, budget/compression, RBAC design are done — only
  the closure items above remain.
