# ADR-0008 — Serena upper-compatible absorption (P1-P4 passive halves)

Date: 2026-04-19
Status: Accepted
Supersedes: None
Related: ADR-0005 (harness-v2), ADR-0007 (symbiote rebrand)

## Context

Serena (oraios/serena) is a widely-used Python-based symbolic agent
toolkit that ships as an MCP server. On 2026-04-18 a local clone of
Serena at commit `37d40d6659fabc3b1a297ba21f28cd373e9502c1` was
inspected and compared against the current CodeLens architecture.
The full comparison lives in `docs/design/serena-comparison-2026-04-18.md`.

**Summary of delta:**

Serena is stronger in three areas that CodeLens does not yet cover:

1. declarative context + mode composition (host / task overlays)
2. pluggable semantic backend strategy (LSP + JetBrains abstraction)
3. packaged user-facing operator experience (dashboard + analytics)

CodeLens is stronger in four areas that should **not** be regressed:

1. role-scoped harness surfaces with bounded tool exposure
2. mutation preflight + verifier-gated editing discipline
3. session-scoped audit / export / aggregate evaluation
4. host-adaptive delegation contracts + explicit planner-builder
   separation

A naive "copy Serena" would collapse CodeLens into Serena's monolithic
agent+server+prompt model and sacrifice its harness-contract strengths.
A naive "ignore Serena" would leave four genuine gaps unaddressed.

## Decision

Absorb Serena's strongest ideas as **passive halves first** under
CodeLens's existing runtime gates and substrate contract. Concretely:

| Phase | Serena idea                | CodeLens landing (passive)                                                                                                                                       | Active rerouting                                                                                        |
| ----- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| P1    | context + mode composition | `HostContext` × `TaskOverlay` overlays on top of existing role profiles, compiled into a `SurfaceOverlayPlan`. Resource: `codelens://surface/overlay`.           | `prepare_harness_session` accepts the two args and compiles the plan — plan is advisory, not enforcing. |
| P2    | backend abstraction        | `BackendCapability` enum + `SemanticBackend` trait with passive Rust engine / LSP bridge / SCIP bridge descriptors. Resource: `codelens://backend/capabilities`. | **Not yet.** Dispatch still calls engines directly.                                                     |
| P3    | project + memory registry  | `MemoryScope::{Project, Global}` enum + `global_memory_dir()` + snapshots. Resources: `codelens://registry/projects`, `codelens://registry/memory-scopes`.       | **Not yet.** `write_memory`/`read_memory` still operate on project scope only.                          |
| P4    | operator dashboard         | `build_operator_dashboard()` aggregator. Resource: `codelens://operator/dashboard`.                                                                              | Pure aggregator — no active rerouting planned.                                                          |

**Layering contract:**

```
Layer 1  Substrate kernel          (session, mutation gate, audit, handoff)
Layer 2  Semantic backend adapters (this ADR, passive)
Layer 3  Surface compiler          (profile × host × task, this ADR)
Layer 4  Host adapter contract     (attach/detach templates, replay)
Layer 5  Operator plane            (dashboard, this ADR, passive)
```

Workflow + audit stay **above** the backend line. Retrieval/edit
operations will eventually compile down to backend capabilities, but
this ADR does not mandate the dispatch rewiring.

## Consequences

### Positive

- CodeLens gains the composition + observability surface Serena
  demonstrated without absorbing Serena's runtime coupling.
- The passive-first shape keeps the public API stable; agents can
  adopt `host_context` / `task_overlay` / new resources incrementally.
- Each passive half ships with `note: "Passive scaffold (Pn)…"` in
  its resource payload so downstream agents cannot mistake contract
  for active routing.
- Test posture stays green: +25 tests added across P1-P4,
  `cargo test --features http` → 444/444 deterministic.

### Negative

- Two resource URIs (`codelens://backend/capabilities`,
  `codelens://registry/memory-scopes`) report capabilities the
  runtime does not yet honour. Agents that assume "listed ⇒ routed"
  will be surprised. Mitigation: explicit `note` field + `mutation_wired`
  boolean on memory scopes.
- The `SurfaceCompilerInput` builder API duplicates what
  `compile_surface_overlay(surface, host, task)` already does. Two
  entry points for the same compile step until P2-active lands and
  consolidates them.
- Deferred work (P2/P3 active) has no deadline. The passive halves
  could drift if dispatch evolves faster than the trait.

### Rejected alternatives

1. **Port Serena wholesale.** Rejected — would collapse the substrate
   vs agent-toolkit boundary that gives CodeLens its harness-native
   advantage.
2. **Enforce overlay `avoid_tools` at the dispatcher.** Rejected —
   would make overlay a hard gate, duplicating mutation gate
   responsibilities. Kept advisory, per Serena §Adopt 1 "compiled hints,
   not the final safety boundary".
3. **Build P1-P4 active halves in one sweep.** Rejected — repository-
   wide refactor risk without a driving customer. Each active half can
   land independently once a caller needs it (e.g. JetBrains bridge
   triggers P2-active; global memory tool triggers P3-active).

## Verification

- `cargo test -p codelens-mcp --features http` → 444/444, 5/5
  consecutive runs deterministic (after v1.9.49 flake fix)
- `onboard_project.has_cycles` → `false`
- Release cadence: v1.9.47 (P1-P3) → v1.9.48 (P4) → v1.9.49 (CI fix)

## References

- Serena repo: `https://github.com/oraios/serena`
- Comparison: `docs/design/serena-comparison-2026-04-18.md`
- Plan artifact: `docs/plans/PLAN_post-cycle-hygiene.md`
- Memory: `project_serena_absorption_2026_04_19`
- Releases:
  - v1.9.47: `https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.47`
  - v1.9.48: `https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.48`
  - v1.9.49: `https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.49`
