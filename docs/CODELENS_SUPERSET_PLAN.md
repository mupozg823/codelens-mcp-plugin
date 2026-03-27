# CodeLens Superset Plan

Updated: 2026-03-27

## Decision

CodeLens can realistically become a superset of Serena only in this narrower sense:

- a Serena-compatible semantic MCP backend
- plus JetBrains-native IDE operating capabilities
- plus a standalone workspace mode for non-JetBrains clients

CodeLens should not try to be a superset of Serena as a full end-user agent product.
It should become the stronger backend substrate that multiple agents can use.

## Objective Feasibility

### Feasible

- Serena-compatible tool contract
- JetBrains-enhanced symbolic retrieval and refactoring
- same public tool names across JetBrains and standalone modes
- IDE-native tools that Serena does not generally expose
- external client support through stdio and JetBrains transport bridges

### Feasible with staged investment

- workspace-mode semantic precision that is good enough for day-to-day edits
- capability negotiation between lean Serena-compatible and full CodeLens profiles
- thin clients for VS Code, Cursor, and terminal-first workflows
- contract tests that guarantee Serena baseline compatibility

### Not realistic as a hard guarantee

- JetBrains PSI precision in every environment
- identical language coverage and precision across all backends
- identical performance in standalone mode and IntelliJ mode

## Current Strategic Advantage

CodeLens already has three assets Serena alone does not combine in one product:

1. JetBrains-native IDE operations
   - open files
   - file problems
   - run configurations
   - project modules and dependencies
   - open file in editor

2. Dual backend direction
   - JetBrains backend for precision
   - workspace backend for standalone usage

3. Transport diversity
   - JetBrains MCP registration
   - standalone stdio MCP server
   - Serena-compatible HTTP/SSE compatibility layer

## Objective Gaps

### Gap A: Workspace precision

Workspace mode still relies on filesystem and regex-heavy parsing.
It is useful, but it is not yet a semantic peer of the JetBrains backend.

### Gap B: Language breadth

Current practical strength is Java and Kotlin.
Other languages are not yet on par with Serena's broader LSP-oriented reach.

### Gap C: Product shaping

CodeLens currently exposes a large tool surface, but it still needs clearer capability profiles:

- Serena baseline
- CodeLens workspace superset
- CodeLens JetBrains superset

### Gap D: Contract safety

Serena compatibility is improving, but it still needs explicit contract tests and profile-based validation so future changes do not drift.

## Recommended Stack

### JetBrains backend

- Kotlin
- JDK 21
- IntelliJ Platform SDK
- PSI
- JetBrains refactoring APIs
- JetBrains inspections/highlighting APIs

### Workspace backend

Short term:

- Python 3.10+
- stdio MCP server
- filesystem scanning
- ripgrep for search
- declaration-range editing

Medium term:

- tree-sitter or LSP-backed symbol provider for major languages
- optional per-language indexers

### Shared product contract

- stable MCP tool names
- JSON schema parity for Serena baseline tools
- capability profiles
- contract tests across backends

## Phase Plan

### Phase 0: Product Contract Foundation

Goal:

- define what "superset" means without hand-waving

Deliverables:

- capability profiles
- Serena baseline contract list
- backend-specific recommended profiles
- contract-oriented config output

Exit criteria:

- `get_current_config` reports profile metadata
- workspace and JetBrains modes expose the same baseline contract names

### Phase 1: Serena Baseline Hardening

Goal:

- make Serena-oriented clients work with minimal adaptation

Deliverables:

- `jet_brains_*` aliases
- `.serena/project.yml` support
- `~/.serena/serena_config.yml` support
- name-path disambiguation on symbol tools
- contract tests for must-match tools

Exit criteria:

- Serena-first prompts run with minimal remapping
- baseline symbol editing contract stays stable under test

### Phase 2: Workspace Precision Upgrade

Goal:

- make standalone mode safe enough for real daily editing

Deliverables:

- project-scope rename precision upgrades
- better reference tracking
- better nested symbol targeting
- minimal usable type hierarchy fallback
- ripgrep-backed search fast path where appropriate

Exit criteria:

- workspace backend handles common multi-file edits without obvious false positives

### Phase 3: JetBrains Superset Depth

Goal:

- widen the gap over Serena in JetBrains-hosted workflows

Deliverables:

- richer file problem payloads
- quick-fix metadata
- editor context and selection-aware actions
- stronger run/debug orchestration
- repository/module/dependency graph improvements

Exit criteria:

- CodeLens offers a clearly stronger IDE operating surface than Serena baseline

### Phase 4: Multi-Client Packaging

Goal:

- make adoption simple outside IntelliJ

Deliverables:

- Cursor/Codex/Claude configuration examples
- launcher scripts
- optional thin VS Code client
- smoke tests against external MCP clients

Exit criteria:

- a user can attach CodeLens to at least one JetBrains path and one standalone path without custom glue code

### Phase 5: Headless JetBrains Backend

Goal:

- approach JetBrains precision without interactive IDE UI

Deliverables:

- feasibility spike for headless JetBrains runtime
- packaging and licensing review
- background indexing lifecycle design

Exit criteria:

- clear go/no-go decision backed by a working spike

## Immediate Development Order

1. Phase 0
   - capability profiles in config output
   - profile-based tests

2. Phase 1
   - baseline contract tests
   - lingering Serena parity cleanup

3. Phase 2
   - workspace project-scope semantic precision

4. Phase 3
   - JetBrains-only differentiators

## Success Metrics

- Serena-oriented clients work with minimal prompt changes
- JetBrains users get strictly more operating power than Serena baseline
- standalone users can edit safely enough to trust workspace mode
- profile metadata allows clients to pick lean vs full tool surfaces deliberately
