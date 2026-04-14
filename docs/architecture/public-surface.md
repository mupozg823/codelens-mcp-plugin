# Public Surface Model

## Goal

CodeLens should expose a public surface that is easy for orchestrators to consume and easy for maintainers to version.

The stable unit is not the raw file layout. The stable unit is the host-facing tool surface and its response contract.

## Surface Layers

### 1. Bootstrap Surface

Purpose:

- establish runtime mode
- expose active project and index state
- tell the host what surface it should start from

Primary entrypoints:

- `prepare_harness_session`
- `get_capabilities`
- `tools/list`
- resource summaries that mirror active surface and health

### 2. Workflow Surface

Purpose:

- answer high-value harness questions in one bounded call
- minimize host-side fanout

Primary entrypoints:

- `analyze_change_request`
- `review_changes`
- `impact_report`
- `verify_change_readiness`
- `refactor_safety_report`
- `prepare_harness_session` follow-up flows

### 3. Primitive Surface

Purpose:

- provide narrow building blocks when the workflow layer is insufficient

Primary entrypoints:

- `find_symbol`
- `find_referencing_symbols`
- `get_symbols_overview`
- `get_ranked_context`
- `get_file_diagnostics`
- filesystem and edit primitives

### 4. Mutation Surface

Purpose:

- apply edits only after explicit host intent and valid preflight

Rules:

- mutation tools remain gated
- rename has its own safety path
- response payloads must preserve why a mutation was blocked

## Stability Rules

The public surface is product-grade only if these rules hold:

- workflow tool names stay stable unless there is a versioned break
- outputs evolve additively by default
- machine-readable identifiers remain stable across transports
- surface gating behavior is deterministic for the same profile/session context
- errors differentiate invalid request, denied surface, missing prerequisite, and internal failure

## What Should Not Leak

The following should remain internal implementation detail:

- exact `AppState` field layout
- whether data came from tree-sitter, LSP, SCIP, or cache unless confidence/provenance matters
- internal queue mechanics
- ad hoc benchmark-specific retrieval bridges

Public payloads may expose provenance when it helps a host choose precision, but they should not require the host to understand internal module boundaries.

## Response Shape Priorities

For agents, the preferred order is:

1. decision-ready summary
2. machine-usable structured payload
3. optional explanatory detail

This is the reverse of prototype-style “narrate everything” output. Enterprise hosts need concise contract-first responses.

## Immediate Hardening Priorities

- keep workflow responses bounded and schema-backed
- ensure recoverable errors carry `recovery_actions`
- keep tool surfaces role-aware but explainable
- prefer stable follow-up handles over repeated bulky payloads
- avoid introducing new public tools when an existing workflow tool can be strengthened instead
