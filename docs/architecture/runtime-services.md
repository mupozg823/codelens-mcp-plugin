# Runtime Services Map

## Goal

This document describes the runtime service decomposition that should exist behind the MCP boundary.

The point is not abstraction for its own sake. The point is to stop `codelens-mcp` from becoming a single monolithic server crate that mixes transport, host policy, project runtime, and analysis state in one place.

## Current Service Groups

### Project Runtime

Owns:

- active/default project selection
- symbol index, graph cache, LSP pool, watcher
- analysis/audit/memory directory selection

Implementation anchor:

- `crates/codelens-mcp/src/state/project_runtime.rs`

### Session Runtime

Owns:

- logical session metadata
- per-session surface overrides
- per-session token budgets
- deferred namespace/tier expansion tracking

Implementation anchor:

- `crates/codelens-mcp/src/state/session_runtime.rs`

### Preflight Runtime

Owns:

- recent preflight cache
- mutation readiness checks
- stale verifier evidence handling

Implementation anchor:

- `crates/codelens-mcp/src/state/preflight.rs`

### Analysis Runtime

Owns:

- analysis artifact store
- durable job store
- background analysis queue
- mutation audit persistence

Implementation anchors:

- `crates/codelens-mcp/src/state/analysis.rs`
- `crates/codelens-mcp/src/analysis_queue.rs`

## AppState Decomposition Rule

`AppState` should be an assembly root, not the place where every runtime concern is modeled directly.

The first decomposition step is to group process-wide runtime concerns into:

- `RuntimePolicyState`
- `RuntimeActivityState`

`RuntimePolicyState` owns:

- transport mode
- daemon mode
- client profile
- effort level
- active default surface
- default token budget

`RuntimeActivityState` owns:

- metrics registry
- recent tools/files/analysis IDs
- doom-loop detection

This keeps the public `AppState` methods stable while shrinking the number of unrelated top-level fields.

## Target End State

Longer term, the MCP crate should read like this:

- transport bootstrap
- host contract shaping
- runtime service assembly
- tool dispatch

Not like this:

- transport
- state storage
- policy rules
- telemetry
- mutation gates
- analysis queueing
- response narration

all interleaved in a handful of large files.

## Concrete Next Steps

1. Keep `AppState` as the assembly root but continue moving field groups behind service structs.
2. Reduce direct field access from sibling modules; prefer narrow methods where the boundary matters.
3. Keep transport-specific session mechanics out of generic response shaping.
4. Keep host-contract policy in one place so Claude Code, Codex, and other orchestrators see the same semantics.
5. Separate product-critical workflow tools from optional accelerators such as semantic or precise backends.

## Enterprise Standard

An enterprise MCP server should let operators answer these questions quickly:

- what is the active runtime mode
- what state is process-wide versus session-local
- what services are optional
- what surfaces are safe for mutation
- what evidence a host should trust before editing code

If those answers require reading half the crate, the runtime service model is still too implicit.
