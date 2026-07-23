# Runtime Convergence Execution Plan — Epics, Issues, Exit Criteria

Scope: 2026 Q3 "contract, security, and parallel-stability reset" per the runtime
convergence review
(claims verified 2026-07-21 at HEAD `91794a0f`; see ADR-0015…0018). New-tool freeze is in
effect for the quarter. No repo code was changed by this plan document.

Sequencing: **E1(contract scaffold) + E4(identity P0) first**, E3 next (largest risk),
E2 rides on E1/E3, E5 audits run parallel, E6 last (cosmetic).

## E1 — Host-Neutral Contract (ADR-0015)

- I1.1 Introduce `HostCapabilities` (negotiated in `prepare_harness_session`; stored per
  transport session). AC: capabilities echoed in prepare response; no model-name inputs.
- I1.2 `ExecutionPolicy` codegen from `tools.toml` (`execution_class`/`risk`/`cost_hint`/
  `concurrency_safe` columns; regen gate extended). AC: 100% of public tools carry policy.
- I1.3 Remove `preferred_executor_bias` + overlay `[bias]` labels + role prose; regen
  attach blocks. AC: `rg "claude|codex"` clean outside host_context enum (ADR-0015 test).
- I1.4 Remove `inject_delegate_to_codex_builder_hint`; add typed `recovery_actions` on
  errors only. AC: no synthetic tool names in any success envelope.
- I1.5 Gate `suggested_next_tools` on `native_tool_search == false`. AC: absent for
  claude-code sessions, present for generic.

## E2 — Surface ≤ 20 (ADR-0016, migration table)

- I2.1 Implement disposition table
  (`docs/design/workflow-first-tool-surface-migration.md`) in
  `presets.rs`: CORE-10/CORE-20 constants, alias layer (callable-unlisted), profile gates.
- I2.2 Complete `outputSchema` + read-only/idempotent/destructive annotations for all
  public tools. AC: CI schema-coverage gate 100%.
- I2.3 Array/cursor/snapshot inputs on read tools (`find_symbol`, `get_ranked_context`,
  `find_referencing_symbols`, `search`). AC: batch calls deterministic & idempotent
  (fixed snapshot ⇒ byte-identical results).
- I2.4 Disable CodeLens deferred-loading when host declares `native_tool_search`.
  AC: prepare no longer re-expands surfaces; parity test per host context.
- I2.5 Skill/agent reference audit in CI (no non-callable names). AC: gate red on the
  current `agents/codelens-explorer.md`, green after E6.

## E3 — Single-Writer Runtime (ADR-0017) — release blocker

- I3.1 One writable runtime per canonical `{project}` using a trusted-user-runtime OS
  lease; contenders fail with `project_writer_busy` (no read-only fallback). AC: two
  processes, one runtime; kill releases the lease and advances durable generation.
- I3.2 Observation generation + commit CAS; stale refresh/ensure rejected. Read handlers
  fence the exact active index and return retryable `index_generation_changed` if a
  commit crosses the request. AC: newest content wins and no mixed payload is emitted.
- I3.3 Singleflight at both safety boundaries: per-project runtime construction and
  per-file analysis keyed by `{path, mtime, content_hash}`. AC: same fingerprints
  coalesce across refresh/index/ensure and persistent instances; disjoint fingerprints
  remain parallel. Embedding/artifact coalescing stays a follow-up optimization.
- I3.4 Session-aware eviction: project binding and retirement are atomic; protected set =
  live bindings + in-flight `Arc` holders; idle shutdown completes before lease reuse.
  AC: five live bindings do not lose runtimes and a bind/evict race never returns busy.
- I3.5 Collapse dual daemon to single endpoint; mutation/readonly becomes per-session
  profile. AC: launchd ships one plist; migration note in
  `docs/operations/http-daemon.md`.
- I3.6 vm_stat admission gate on indexing jobs (backlog import from CodeGraph study).
  AC: heavy index defers under memory pressure warning level; no cpu_resource reports
  during soak.

## E4 — Identity & Coordination Hardening (ADR-0018) — P0

- I4.1 Transport-derived session identity; `_session_*` args ignored on HTTP. AC: forgery
  test rejects cross-session reads/writes.
- I4.2 Coordination fail-closed (`coordination_unavailable` typed error; delete in-memory
  fallback at `agent_coordination.rs:440,548,630`). AC: chaos test zero silent successes.
- I4.3 Deprecate register/claim/release/list_active_agents (telemetry + removal gate).
- I4.4 P0 audit trio: LSP exec allowlist / remote-root rejection / header-trust removal —
  each lands with a failing-then-passing regression test.

## E5 — Contract & Consistency Gates

- I5.1 One ToolCatalog source generating schema + handler registration + skill/doc
  references (extends ADR-0013 codegen). AC: drift gates cover skills and agents too.
- I5.2 Cross-host integration matrix (claude-code / codex / generic × CORE-20): first-step
  failure count 0 for every packaged skill × default profile.

## E6 — Hooks & Agents Cleanup

- I6.1 Remove Grep/Bash hook from default install (`hooks/hooks.json`); ship as opt-in
  extra. AC: fresh install has zero PreToolUse hooks; docs updated.
- I6.2 Regenerate `codelens-explorer` as model-unpinned read-only role on the CORE-20
  verb surface (current file lists non-listed tool names + `model: haiku`).
- I6.3 Shrink attach-generated CLAUDE/AGENTS routing blocks to invariants + test commands
  (40–60 lines), removing role assignments.

## Quarter Exit Criteria (roll-up)

1. 5 sessions · 5 worktrees share one daemon without active-runtime eviction or
   cross-project leakage; a second same-project process is rejected before WAL
   contention; newest content wins and mixed-generation responses are discarded.
2. Public tool schema coverage 100%; default surface ≤ 20; zero skill-referenced
   non-callable tools; zero mutations in default profile.
3. Zero model-name routing; zero synthetic delegation; zero `_session_*` trust on HTTP;
   zero coordination silent-success paths.
