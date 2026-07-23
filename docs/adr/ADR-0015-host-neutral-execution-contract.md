# ADR-0015: Host-Neutral Execution Contract

- **Status:** Accepted (runtime convergence, 2026 Q3) — 2026-07-21
- **Date:** 2026-07-21
- **Supersedes:** ADR-0006 (server-side `preferred_executor` metadata); amends ADR-0014 wording
- **Evidence base:** external runtime-convergence review (pinned at `a47701a`), claims
  verified against HEAD `91794a0f` on 2026-07-21

## Context

CodeLens currently encodes a fixed model-role split ("Claude plans, Codex builds") in three
independent places: routing overlays carry `[bias: claude]` labels and a
`preferred_executor_bias` field (`tool_defs/presets/overlay.rs:107`), the response envelope
injects a synthetic `delegate_to_codex_builder` hint
(`dispatch/response_support/delegate_builder.rs:39`), and the attach-generated host docs fix
agent roles in prose. Meanwhile 2026-era hosts own orchestration natively: GPT-5.6 Ultra
coordinates parallel agents and calls tools programmatically (PTC); Claude Fable-class
harnesses run multi-day loops with native subagents and Tool Search. A server that
re-orchestrates model roles now fights the host instead of serving it.

## Decision

1. Remove all model/vendor identifiers from routing metadata. `preferred_executor_bias`,
   overlay `[bias: …]` labels, and the role-fixing prose blocks are deleted.
2. Remove the synthetic `delegate_to_codex_builder` hint. Error responses may carry
   `recovery_actions` (deterministic, tool-name-only); success responses carry no
   next-step steering for hosts that declare native orchestration.
3. Introduce a `HostCapabilities` contract, negotiated once in `prepare_harness_session`
   and derived from client-declared facts — never from model-name sniffing:
   `native_tool_search`, `native_subagents`, `nested_subagents`, `native_worktrees`,
   `native_edit`, `mcp_tasks`, `dynamic_tool_list`, `workspace_binding`,
   `approval_or_elicitation`.
4. Introduce per-tool `ExecutionPolicy` metadata generated from `tools.toml`:
   `execution_class` (`read | analyze | mutate`), `risk`, `cost_hint`,
   `concurrency_safe`. Hosts route with this; CodeLens never assigns executors.

## Consequences

- Model refreshes (new tiers, new modes) no longer require server changes.
- `suggested_next_tools` stays only for hosts without native tool search, and is gated by
  `HostCapabilities.native_tool_search == false`.
- ADR-0006 enforcement machinery is retired; its telemetry remains for surface analytics.

## Verification

- `rg -n "claude|codex" crates/codelens-mcp/src/tool_defs crates/codelens-mcp/src/dispatch`
  returns only the `host_context` enum (transport identification, not routing).
- Attach-regenerated CLAUDE.md/AGENTS.md routing blocks contain no role or bias claims.
- Integration test: identical tool surface and identical responses for a fixed request
  sequence across `claude-code`, `codex`, and `generic` host contexts, differing only by
  declared capabilities.
