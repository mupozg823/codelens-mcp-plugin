# Host-Orchestrated MCP Contract

## Purpose

CodeLens is not the agent runtime and it is not the workflow orchestrator.

The host owns orchestration. In Claude Code terms, `QueryEngine/query()` is the orchestrator and CodeLens is a bounded MCP sidecar that returns:

- compressed context
- evidence for decisions
- safe mutation preflight signals
- explicit recovery actions when the caller used the wrong surface or skipped a prerequisite

This boundary is the product contract. If CodeLens behaves like an execution engine, it becomes harder for hosts to reason about retries, recovery, and delegation.

## Product Rule

CodeLens must optimize for host predictability, not for autonomous tool choreography.

That means:

- host decides the next call
- CodeLens returns enough structure for the next call to be obvious
- CodeLens does not hide recovery steps behind prose-only guidance
- CodeLens does not assume a human is reading an interactive dashboard first

## Required Call Pattern

Recommended host flow:

1. `prepare_harness_session`
2. `tools/list` only if the host needs surface expansion detail
3. one workflow-oriented tool (`analyze_change_request`, `get_ranked_context`, `review_changes`, `prepare_harness_session` follow-up)
4. only then drop to lower-level symbol, reference, or edit tools if the task broadens

Mutation flow:

1. host selects a mutation tool
2. host runs verifier entrypoint first (`verify_change_readiness`, `safe_rename_report`, or targeted preflight)
3. CodeLens returns `ready`, `caution`, or `blocked`
4. host either proceeds or follows the returned recovery action

## Response Contract Requirements

Every host-facing response should help the orchestrator choose the next step without guesswork.

Required properties for recoverable failures:

- machine-readable error code
- compact summary of why the call failed
- `recovery_actions` with the next valid RPC or tool call
- `orchestration_contract` only when surface, tier, or host-routing context matters

Required properties for successful workflow responses:

- bounded summary first
- structured payload second
- stable identifiers for follow-up reads (`analysis_id`, `job_id`, resource handles)
- readiness/risk language that maps to agent policy rather than UI-only phrasing

## Anti-Patterns

The following are product regressions:

- CodeLens attempting to sequence a multi-step workflow internally instead of returning the next valid step
- tool responses that assume a human will inspect a TUI before the agent can continue
- long prose blocks without a machine-usable next action
- host-specific policy hidden inside tool descriptions instead of explicit contract fields
- shell-centric instructions when an MCP follow-up call is the correct recovery path

## Current Implementation Direction

The runtime should converge on three host-facing layers:

- bootstrap layer: discover active surface, budget, readiness, and transport/runtime health
- workflow layer: one-call summaries for analysis, review, refactor planning, and impact discovery
- primitive layer: symbol, reference, filesystem, and edit operations used only when workflow tools are insufficient

The server should stay strict about that layering even if more tools are added.

## Enterprise Readiness Implication

Enterprise quality here is not “more features.” It is:

- fewer ambiguous retries
- fewer shell fallbacks
- stable structured recovery
- bounded, inspectable contracts that multiple hosts can rely on

If a host can integrate CodeLens without custom heuristics for every failure path, the MCP layer is doing its job.
