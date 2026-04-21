# Managed Agents handoff protocol — `codelens-handoff-v1`

Phase O6 of `docs/plans/PLAN_opus47-alignment.md` elevates
`export_session_markdown` from a human-readable telemetry dump into a
machine-consumable handoff artifact for Anthropic's 2026-04 Managed
Agents pattern (Planner / Generator / Evaluator separated into their
own session / harness / sandbox layers).

This page pins the contract. Bumping the schema is additive in v1 and
requires a new `schema_version` string for any breaking change.

## Schema version

```
schema_version: "codelens-handoff-v1"
```

Consumers **must** gate on this exact string and reject unknown
versions at the boundary.

## Response shape

```jsonc
{
  "schema_version": "codelens-handoff-v1", // required, exact match
  "markdown": "...",                       // human-readable telemetry
  "markdown_bytes": 42831,                 // byte length of `markdown`
  "truncated": false,                      // true if cap fired
  "session_name": "session",               // caller-supplied label
  "session_id": "planner-md" | null,       // null for global scope
  "scope": "session" | "global",
  "tool_count": 4,                         // distinct tools observed
  "total_calls": 17,                       // total tool invocations
  "audit": {                               // null if no session_id
    "role": "planner" | "builder",
    "status": "ok" | "warn" | "fail" | "not_applicable",
    "score": 0.85,                         // 0.0 – 1.0
    "findings": [
      { "severity": "warn", "summary": "..." }
    ],
    "recommended_next_tools": ["review_changes"],
    "session_summary": { ... }             // see audit_common.rs
  }
}
```

### `audit` role tagging

The `audit.role` field tags which audit lane produced the payload so a
downstream Evaluator primitive can pick scoring rubric without re-
running surface detection:

- `"planner"` — session ran on a planner/reviewer surface
  (`reviewer-graph`, `planner-readonly`, etc.). The payload comes from
  `build_planner_session_audit`.
- `"builder"` — session ran on a builder/mutation surface
  (`builder-minimal`, `refactor-full`). The payload comes from
  `build_builder_session_audit`.

When `session_id` is not provided, `audit` is `null` — there is no
session-scoped audit to run.

## Size cap

`markdown` is capped at **50 KiB** (`HANDOFF_MAX_MARKDOWN_BYTES = 50 *
1024`) before the response is returned. If the session's raw markdown
exceeds the cap:

1. `markdown` is truncated at a UTF-8 char boundary at `cap - 96`
   bytes.
2. A sentinel of the form
   `\n\n_(handoff truncated at <cap>B; <dropped> bytes dropped)_\n`
   is appended.
3. `truncated` is set to `true`.
4. `markdown_bytes` reflects the **post-truncation** length so the
   Evaluator can decide whether to persist-and-replay vs inline.

Rationale: Opus 4.7 `output_config.task_budget` caps per-call tokens.
50 KiB fits comfortably under the inline-token cutoff while still
carrying roughly 1000 lines of telemetry + audit. A runaway session
should be persisted to disk (e.g., via
`collect-session.sh`) instead of replayed inline; the `truncated` flag
is the signal to do that.

### Test-only cap override

Tests may set `CODELENS_HANDOFF_MAX_BYTES` to force a smaller cap so
the truncation path is exercisable without synthesising 50+ KiB of
markdown (which would itself fight the outer MCP response
compression). Values below 64 are clamped to 64 so the sentinel
always fits. **Do not set this in production.**

## Consumer contract

Downstream consumers (Evaluator primitive, CLI tooling, external
orchestrators) should:

1. Read `schema_version` first — reject if unknown.
2. Consume the structured `audit` object — **never** regex-scrape the
   markdown body for status/score/findings.
3. Treat `truncated=true` as "persist-and-replay" signal; do not rely
   on the markdown being complete.
4. Use `tool_count` + `total_calls` + `markdown_bytes` for budget /
   effort routing decisions.

## Related code

- Schema constants + cap helper:
  `crates/codelens-mcp/src/tools/session/metrics_config/metrics.rs`
- Contract tests:
  `crates/codelens-mcp/src/integration_tests/handoff_protocol.rs`
- Planner audit: `crates/codelens-mcp/src/tools/session/planner_audit.rs`
- Builder audit: `crates/codelens-mcp/src/tools/session/builder_audit.rs`
