# CodeLens MCP — Multi-Agent Integration

> Public integration pattern for planner/reviewer and builder/refactor agents that share one CodeLens project scope.

This document describes how to use released CodeLens features to coordinate multiple agents on one repository. It does **not** require a custom `codex-builder` agent file, and it does **not** assume Claude/Codex-specific local harness hacks.

For the higher-level operating shapes that sit above these primitives, see [Harness modes](harness-modes.md). For the machine-readable contract that a host can reuse directly, see [Portable harness spec](harness-spec.md).

## What CodeLens Provides vs What the Host Provides

CodeLens provides:

- shared stdio or HTTP MCP access
- role-based profiles such as `reviewer-graph` and `refactor-full`
- bounded bootstrap via `prepare_harness_session`
- advisory coordination via `register_agent_work`, `claim_files`, `list_active_agents`, and `release_files`
- mutation safety via `verify_change_readiness`, `safe_rename_report`, and runtime mutation gates
- session-scoped builder audit via `audit_builder_session`, `get_tool_metrics(session_id=...)`, and `export_session_markdown(session_id=...)`
- session-scoped planner/reviewer audit via `audit_planner_session`
- canonical runtime/doc surface inventory via `codelens://surface/manifest`

The host or harness still provides:

- the orchestrator loop
- the decision to dispatch one agent to another
- branch policy, merge policy, and release policy
- any Claude-specific or Codex-specific custom agent wrapper

CodeLens is the shared coordination and verification layer, not the orchestrator.

## Delegate Scaffold Correlation

When CodeLens prepends the synthetic host action
`delegate_to_codex_builder` in `suggested_next_calls`, the scaffold now
includes a stable `handoff_id` in three places:

- top-level `handoff_id`
- `delegate_arguments.handoff_id`
- `carry_forward.handoff_id`

Hosts should preserve that value when they replay the delegated builder
call. This does not create a new runtime contract or hard block. It
simply lets the append-only telemetry log correlate planner-side
delegate emission with later builder-side execution, even when those
steps happen under different logical sessions.

## Recommended Role Split

For multi-agent HTTP deployments, keep the surfaces asymmetric:

- planner / reviewer agents -> `reviewer-graph` on a read-only daemon
- builder / refactor agents -> `refactor-full` on a mutation-enabled daemon

Typical split:

```bash
# read-only planner / reviewer daemon
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# mutation-enabled builder / refactor daemon
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

Those are the public generic example ports. If you are using this repository's
local launchd installer, the repo-local dual-daemon shape is `:7839`
read-only plus `:7838` mutation-enabled.

Operational rule:

- one mutation-enabled agent per worktree
- additional agents stay read-only

## Fixed Preflight Order

Before dispatching a builder/refactor agent, run the CodeLens preflight in this order.

### 1. Bootstrap the session

```json
{
  "name": "prepare_harness_session",
  "arguments": {
    "profile": "reviewer-graph",
    "detail": "compact"
  }
}
```

Why first:

- establishes the active project/session view
- returns bounded surface and health state
- exposes current coordination counts for the session snapshot

### 2. Inspect each target file structurally

```json
{
  "name": "get_symbols_overview",
  "arguments": {
    "path": "src/example.rs"
  }
}
```

Run once per target file.

### 3. Inspect diagnostics for each target file

```json
{
  "name": "get_file_diagnostics",
  "arguments": {
    "file_path": "src/example.rs"
  }
}
```

Run once per target file.

### 4. Run the verifier across the full change set

```json
{
  "name": "verify_change_readiness",
  "arguments": {
    "task": "Refactor example flow without changing behavior",
    "changed_files": ["src/example.rs", "src/lib.rs"],
    "profile_hint": "refactor-full"
  }
}
```

Run this once for the whole intended change set, not file-by-file.

### Dispatch Gate

Interpret the verifier result conservatively:

- `mutation_ready == "blocked"` -> stop and report blockers
- `mutation_ready == "caution"` with `overlapping_claims` -> stop and report the conflicting session, branch, and claimed paths
- otherwise -> dispatch the builder/refactor agent

Why this order matters:

- skipping `prepare_harness_session` weakens the session-local view that the host sees during bootstrap
- skipping per-file structure/diagnostics makes the builder brief less precise
- running the verifier on partial file sets hides cross-file overlap and readiness evidence

## Coordination Discipline

If the host is about to start a builder/refactor pass, publish that intent first.

### 1. Register the agent intent

```json
{
  "name": "register_agent_work",
  "arguments": {
    "agent_name": "builder-agent",
    "branch": "feature/refactor-example",
    "worktree": "/abs/path/to/worktree",
    "intent": "Refactor example flow after preflight",
    "ttl_secs": 600
  }
}
```

### 2. Claim the mutation targets

```json
{
  "name": "claim_files",
  "arguments": {
    "paths": ["src/example.rs", "src/lib.rs"],
    "reason": "planned refactor pass",
    "ttl_secs": 600
  }
}
```

Use the same TTL for registration and claims.

### 3. If overlap appears, stop

Do not auto-dispatch through an overlap. Report it back to the orchestrator.

- overlapping claims are advisory, not hard locks
- the correct policy decision still belongs to the orchestrator

### 4. Release claims explicitly on completion

```json
{
  "name": "release_files",
  "arguments": {
    "paths": ["src/example.rs", "src/lib.rs"]
  }
}
```

TTL expiry is only a safety net. Explicit release is better because it keeps the shared view current.

## Builder Session Audit

`audit_builder_session` is the public session-level check for builder/refactor discipline. It does not add new hard blocks. It scores whether a builder session used CodeLens in the expected order and with enough evidence.

Typical compact call:

```json
{
  "name": "audit_builder_session",
  "arguments": {
    "session_id": "builder-session-id",
    "detail": "compact"
  }
}
```

Expected status meanings:

- `pass` -> bootstrap, preflight, diagnostics, and coordination evidence are all present
- `warn` -> mutation succeeded, but the session skipped bootstrap, diagnostics, or coordination discipline
- `fail` -> mutation/preflight contract was violated
- `not_applicable` -> no builder/refactor flow was recorded for that session

Related session tools:

- `get_tool_metrics({"session_id":"..."})` -> machine-readable per-session telemetry
- `export_session_markdown({"session_id":"..."})` -> human-readable markdown with the same session's audit summary appended

Current scope:

- builder / refactor sessions only
- planner / reviewer sessions use `audit_planner_session`

## Planner / Reviewer Session Audit

`audit_planner_session` is the read-side companion to `audit_builder_session`. It stays audit-only: no new runtime hard blocks are added.

Typical compact call:

```json
{
  "name": "audit_planner_session",
  "arguments": {
    "session_id": "planner-session-id",
    "detail": "compact"
  }
}
```

Expected status meanings:

- `pass` -> bootstrap, workflow-first routing, and read-side evidence all line up
- `warn` -> the session skipped bootstrap, lacked change evidence, fell into low-level chains, or reviewed files without symbol/diagnostic evidence
- `fail` -> the planner/reviewer session attempted content mutation or shows mutation-gate denial traces
- `not_applicable` -> the session never entered a planner/reviewer read-side flow

Related session tools:

- `get_tool_metrics({"session_id":"..."})` -> machine-readable per-session telemetry for either builder or planner sessions
- `export_session_markdown({"session_id":"..."})` -> appends the role-appropriate builder or planner audit summary automatically

## Canonical Surface Manifest

CodeLens now publishes a single machine-readable surface manifest at `codelens://surface/manifest`.

Use it when you need the authoritative source for:

- workspace version and members
- live registered tool count and output-schema coverage
- profile/preset membership
- supported language families and extensions
- server-card summary inputs

Repository docs consume the generated form of the same manifest at [`docs/generated/surface-manifest.json`](generated/surface-manifest.json).

## TTL Guidance

Recommended rule:

- `ttl_secs = expected_duration * 1.5`
- default to `600` seconds when unsure
- cap long claims at `3600` seconds unless there is a concrete reason to hold them longer

Short TTLs are safer than stale claims. Renew intentionally if the work is still active.

## Rename and Broad Refactor Notes

For rename-heavy changes, do not jump straight from `verify_change_readiness` to mutation.

Use:

- `safe_rename_report`
- or `unresolved_reference_check`

before:

- `rename_symbol`
- or any broad rename/refactor pass that depends on symbol identity

## Minimal Planner -> Builder Pattern

The public pattern is:

1. planner attaches to the read-only surface
2. planner runs bootstrap + structure + diagnostics + verifier
3. planner registers intent and claims files
4. builder attaches to the mutation-enabled surface
5. builder performs the bounded change
6. planner or builder runs post-edit diagnostics
7. builder releases claims

This pattern works with any orchestrator that can call MCP tools. Custom agent wrappers can improve ergonomics, but they are not required for the core CodeLens integration.
