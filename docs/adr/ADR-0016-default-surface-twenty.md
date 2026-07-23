# ADR-0016: Default Tool Surface ≤ 20

- **Status:** Accepted (workflow-first surface, 2026 Q3) — 2026-07-21
- **Date:** 2026-07-21
- **Builds on:** ADR-0010 (telemetry-driven tool diet), ADR-0013 (tools.toml codegen)

## Context

`tools.toml` registers 100 tools across 9 categories. The default listed surface is already
lean for Claude Code (9 verbs), but generic/Codex clients see far more, and CodeLens layers
its own deferred-loading gate (`_session_deferred_tool_loading`) on top of hosts that now
ship native tool search. Anthropic guidance places selection-accuracy degradation at
~30–50 concurrently loaded tools; Codex has no native tool search and relies on small
static surfaces plus skill progressive disclosure. Two systems solving the same problem
in different layers produce inconsistent visibility (`prepare_harness_session` re-expands
what the host deferred).

## Decision

1. **Always-loaded core (10):** `prepare_harness_session`, `search`, `overview`, `graph`,
   `diagnose`, `review`, `plan_safe_refactor`, `verify_change_readiness`,
   `get_changed_files`, `get_current_config`.
2. **Static default surface (≤ 20):** core plus `get_ranked_context`, `find_symbol`,
   `find_referencing_symbols`, `semantic_search`*, `refresh_symbol_index`,
   `get_watch_status`, `start_analysis_job`, `get_analysis_job`, `get_analysis_section`,
   `cancel_analysis_job`. (*present only when the semantic feature is active.)
3. Remaining registered names survive one compatibility release as **hidden aliases**
   (callable, not listed), then are removed on usage telemetry. The full disposition of
   all 100 names lives in `docs/design/workflow-first-tool-surface-migration.md`.
4. Host-native duplicates (`read_file`, `list_dir`, `find_file`) leave the default surface
   entirely; they remain behind the `generic` compatibility profile only.
5. CodeLens-side deferred loading is disabled when
   `HostCapabilities.native_tool_search == true`; the server then exposes the static ≤ 20
   and lets the host search the rest.
6. Every public tool ships complete `inputSchema`, `outputSchema`, and
   read-only/idempotent/destructive annotations. Read tools accept array inputs and
   cursors and honor snapshot pins, making them safe targets for programmatic tool
   calling and parallel fan-out. Mutation tools are excluded from programmatic/indirect
   invocation paths.

## Consequences

- One surface story per host class; `prepare_harness_session` stops re-expanding surfaces.
- The verb facade becomes the real API; mode-shaped former tools become documented modes.
- Skills and agent definitions must reference only callable-surface names (CI-checked).

## Verification

- Default `tools/list` ≤ 20 for every built-in profile; schema coverage 100% (CI gate).
- Zero skills/agents referencing non-callable names (`scripts/surface-manifest.py --check`
  extended with a reference audit).
- Token-cost snapshot per host profile recorded before/after in `benchmarks/`.
