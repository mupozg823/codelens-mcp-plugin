# ADR-0001: Runtime Boundaries And Single-Source Registries

- Status: Proposed
- Date: 2026-04-12

## Context

CodeLens already has a sound top-level split:

- `codelens-engine` as the code-intelligence engine
- `codelens-mcp` as the harness-facing MCP runtime

The current problem is not top-level architecture failure.
The current problem is **intra-layer concentration and drift**:

- oversized files such as `state.rs`, `tools/symbols.rs`, and `embedding/mod.rs`
- duplicated registries such as LSP defaults in both engine and MCP layers
- retrieval/query policy concentrated inside one tool module

This creates three recurring costs:

1. maintenance and review become harder
2. AI-generated incremental code is harder to judge for necessity
3. behavior drift can occur without an explicit architectural decision

## Decision

We will keep the existing two-crate boundary and simplify **inside** it.

### 1. Preserve The Two-Crate Split

Keep:

- `codelens-engine` = parsing, indexing, search, graph, semantic backend
- `codelens-mcp` = transport, runtime state, tool exposure, workflow policy

We are explicitly rejecting a ground-up rewrite or a re-merge into one crate.

### 2. Make Registries Single-Source

Authoritative registries must live in exactly one place.

Immediate target:

- LSP recipes live in engine `lsp/registry.rs`
- MCP uses engine-derived defaults instead of maintaining its own parallel mapping

Rule:

- no second extension-to-command table is allowed unless the owning ADR explicitly justifies it

### 3. Split Query Analysis From Tool Handler Code

`tools/symbols.rs` should stop owning every part of:

- lexical-vs-NL classification
- identifier splitting
- query expansion
- semantic priors
- response shaping

Target shape:

- `tools/query_analysis.rs` or equivalent extracted module
- handler file keeps orchestration only

### 4. Shrink AppState By Responsibility

`AppState` remains the runtime root, but its internal responsibilities should be extracted into focused units:

- project runtime context/cache
- session/runtime surface state
- analysis artifact/job coordination
- watcher maintenance and health

The goal is not an abstraction explosion.
The goal is fewer unrelated responsibilities per file.

### 5. Prefer Measured Simplification Over New Abstractions

New modules are only justified if they reduce one of:

- duplicated logic
- blast radius
- review difficulty
- state coupling

We explicitly reject interface-heavy layering that adds names without reducing coupling.

## Consequences

### Positive

- fewer silent drift points
- smaller review surfaces
- clearer ownership boundaries
- easier alignment with harness-oriented skills and agent workflows

### Negative

- short-term refactor cost
- temporary churn around imports and test placement
- some historic files will need careful blame-aware extraction

## Non-Goals

- rewriting the project around a new framework
- replacing tree-sitter-first retrieval with LSP-first retrieval
- removing profile/surface shaping
- merging all workflow reports into one generic engine layer

## Migration Plan

### Phase 1

- remove version/path drift from monorepo dependency declarations
- centralize LSP defaults

### Phase 2

- extract query analysis from `tools/symbols.rs`
- keep tests close to the extracted behavior

### Phase 3

- extract the smallest low-risk `AppState` slice first
- likely candidates: watcher maintenance or project-context cache helpers

### Phase 4

- unify candidate fan-out logic between cached and non-cached symbol retrieval

### Phase 5

- split semantic backend internals inside `embedding/`

## Acceptance Signals

- fewer duplicated registries
- smaller top-risk files
- no regression in `cargo check` and targeted MCP tests
- documentation can describe one authoritative source for each major runtime registry
