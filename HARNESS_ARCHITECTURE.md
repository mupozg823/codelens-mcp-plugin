# Harness Optimization Architecture

This document captures the harness optimization structure this repo should optimize toward.
It is informed by the current CodeLens MCP design and by useful patterns observed in large agent runtimes such as Claude Code.

The goal is not to copy a product runtime into this repo.
The goal is to keep CodeLens sharp as a harness optimization tool for agent runtimes.

## Architectural Position

CodeLens is not the interactive agent runtime and it is not the harness itself.

- The interactive runtime belongs to tools like Claude Code.
- CodeLens should remain the bounded analysis, verification, and artifact backend that optimizes those harnesses.
- Integration should stay call-based through MCP, not embedded into the main agent loop.

## Core Boundary

Keep these boundaries explicit:

- transport and session entry
- request normalization
- access and mutation policy
- tool and workflow execution
- artifact and job persistence
- benchmark and evaluation gates

If a change blurs two or more of those boundaries, prefer refactoring before adding more logic.

## Structure Patterns Worth Keeping

### 1. Thin Adapters, Thick Core

Claude Code keeps multiple entry paths and funnels them into a shared runtime loop.
CodeLens should do the same in server form:

- keep HTTP and stdio transports thin
- normalize request metadata once
- route into shared dispatch and workflow logic

Implication for this repo:

- `server/*` should stay focused on transport concerns
- `dispatch.rs` should stay the request execution boundary
- transport-specific branching should not reimplement policy or tool logic

### 2. Capability Assembly at the Edge

Claude Code assembles commands and tools dynamically instead of hard-coding one flat surface.
CodeLens should keep doing the equivalent:

- derive tool surface from profile, namespace, tier, and deferred-loading state
- avoid static always-on tool exposure
- keep surface decisions close to the request edge

Implication for this repo:

- preserve `tool_defs/*`, access filters, and deferred-loading semantics
- keep new workflow tools inside the same surface-selection model

### 3. Cheap Sync, Heavy Async

Claude Code keeps fast turns responsive and pushes heavy work into background tasks and handles.
CodeLens should preserve the same split:

- cheap reads stay synchronous
- expensive analysis returns handles
- repeated expansions use stored artifacts instead of recomputation

Implication for this repo:

- prefer `start_analysis_job -> get_analysis_job -> get_analysis_section`
- avoid turning heavyweight reports into mandatory synchronous calls
- keep artifact reuse and handle reuse first-class

### 4. Verifier Before Mutation

Claude Code benefits from a preflight stage before risky actions.
CodeLens already has this strength and should lean into it:

- preflight evidence before refactor-sensitive mutations
- mutation policy stays explicit and typed
- blocking conditions should be machine-readable, not only prose

Implication for this repo:

- keep `mutation_gate.rs` central
- do not bypass verifier flow for convenience paths
- prefer structured verifier outputs over freeform status text

### 5. Orchestrator vs Harness Optimizer

Claude Code is an orchestrator.
CodeLens is a harness optimization tool.

That means:

- Claude Code decides when to ask for context, impact, verification, or handles
- CodeLens returns bounded evidence, not giant open-ended transcripts
- CodeLens should optimize for precision, reuse, and predictable latency

Implication for this repo:

- response contracts should favor summaries, handles, and bounded sections
- avoid product-runtime concerns such as transcript UI, multi-agent chat state, or local shell orchestration inside CodeLens

## Anti-Patterns

Avoid these even if they reduce file count or make one workflow look simpler:

- turning transport files into policy engines
- merging stores back into one god-state file
- making every request go through heavyweight analysis
- embedding release packaging logic into local or CI parity gates
- replacing typed handles with large freeform blobs
- duplicating the same verification commands across multiple workflows

## Target Layout

The repo should continue optimizing toward this shape:

```text
transport/
  http, stdio, routing, session headers
request/
  session context, envelopes, request shaping
policy/
  access filters, mutation gates, deferred loading
execution/
  dispatch, workflow tools, report tools, response shaping
stores/
  artifact store, job store, preflight store, audit log
contracts/
  policy, harness modes, eval contract, development pipeline
```

Exact filenames may differ, but the boundaries should stay visible.

## Practical Optimization Priorities

### Near Term

- keep build and CI gates sharing repo-local scripts
- continue shrinking duplicated workflow logic
- preserve fast local gates
- keep HTTP and transport semantics explicit

### Mid Term

- reduce large multi-purpose files in transport and workflow layers
- keep report tools grouped by function instead of one catch-all module
- improve handle-first flows for repeated analysis use

### Long Term

- make external orchestrators consume CodeLens through stable call contracts
- measure latency by mode: fast read, verifier, async analysis
- optimize for bounded response cost, not for giant one-shot output

## Mapping: Claude Code Insight -> CodeLens Action

| Claude Code pattern | CodeLens action |
| --- | --- |
| Shared runtime behind multiple adapters | Keep HTTP and stdio thin and normalize once |
| Dynamic tool assembly | Keep surface/profile/deferred-loading decisions explicit |
| Background task runtime | Use artifact/job handles for heavy analysis |
| Main-loop responsiveness | Preserve fast local and sync read paths |
| Risky action gating | Keep verifier-first mutation flow central |
| Harness-optimizer-friendly bounded outputs | Prefer sections, handles, summaries, machine schemas |

## Decision Rule

When choosing between a simpler patch and a more powerful one, prefer the option that keeps CodeLens:

1. externally callable
2. bounded in latency
3. explicit in policy
4. reusable through handles and artifacts
5. separate from the interactive agent runtime
