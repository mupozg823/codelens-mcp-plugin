# CodeLens IntelliJ Independence Migration Plan

Updated: 2026-03-28

## Final Goal

CodeLens should stop being primarily an IntelliJ plugin with a standalone side path.

The final target is:

- Rust is the primary code intelligence runtime
- IntelliJ is an optional adapter that contributes PSI-only features
- CodeLens remains useful without IntelliJ installed
- build, test, and packaging do not require a local IntelliJ installation except for adapter-specific validation

This plan intentionally optimizes for IntelliJ independence first, not feature breadth first.

## Non-Goals

- Reproducing full IntelliJ PSI precision in every environment
- Removing all IntelliJ-specific features from the product
- Shipping a full editor adapter matrix before the Rust core becomes the source of truth

IntelliJ-native features may remain, but they must become additive rather than foundational.

## Current Coupling

### Runtime coupling

- Backend selection defaults to JetBrains unless `codelens.backend=workspace` is explicitly set
- `CodeLensBackendProvider` requires IntelliJ `Project` to select a backend
- `JetBrainsCodeLensBackend` directly owns PSI symbol retrieval, reference search, type hierarchy, refactoring-backed rename, file reads, search, and edit flows

Relevant files:

- [`src/main/kotlin/com/codelens/backend/CodeLensBackendProvider.kt`](/Users/bagjaeseog/codelens-mcp-plugin/src/main/kotlin/com/codelens/backend/CodeLensBackendProvider.kt)
- [`src/main/kotlin/com/codelens/backend/jetbrains/JetBrainsCodeLensBackend.kt`](/Users/bagjaeseog/codelens-mcp-plugin/src/main/kotlin/com/codelens/backend/jetbrains/JetBrainsCodeLensBackend.kt)

### Build coupling

- Gradle depends on `org.jetbrains.intellij.platform`
- local IntelliJ installation is referenced directly with `local("/Applications/IntelliJ IDEA.app")`
- plugin packaging and runtime assumptions still define the repo's primary build shape

Relevant file:

- [`build.gradle.kts`](/Users/bagjaeseog/codelens-mcp-plugin/build.gradle.kts)

### Product coupling

- README still frames the IntelliJ plugin as the primary product and standalone as secondary
- tool taxonomy and architecture description are still plugin-first

Relevant file:

- [`README.md`](/Users/bagjaeseog/codelens-mcp-plugin/README.md)

## Current Rust Position

The Rust path already has enough substance to serve as the seed of the new core:

- filesystem read/search/list tools
- tree-sitter-backed symbols
- in-memory and on-disk symbol index
- pooled stdio LSP references
- pooled stdio LSP diagnostics
- MCP stdio server

Relevant files:

- [`rust/crates/codelens-core/src/symbols.rs`](/Users/bagjaeseog/codelens-mcp-plugin/rust/crates/codelens-core/src/symbols.rs)
- [`rust/crates/codelens-core/src/lsp.rs`](/Users/bagjaeseog/codelens-mcp-plugin/rust/crates/codelens-core/src/lsp.rs)
- [`rust/crates/codelens-mcp/src/main.rs`](/Users/bagjaeseog/codelens-mcp-plugin/rust/crates/codelens-mcp/src/main.rs)

That is enough to shift the migration question from "should Rust exist?" to "when does Rust become primary?"

## Success Criteria

CodeLens can be considered IntelliJ-independent when all of the following are true:

1. A non-IntelliJ client can use the main symbolic workflow without degraded product identity
2. Core MCP tools run from Rust, not from Kotlin/IntelliJ
3. IntelliJ plugin delegates editor-independent work to Rust
4. CI can validate the main runtime without IntelliJ installed
5. IntelliJ-only features are clearly marked as adapter capabilities

## Capability Classification

### Must move to Rust core

- `read_file`
- `list_dir`
- `find_file`
- `search_for_pattern`
- `get_symbols_overview`
- `find_symbol`
- `find_referencing_symbols`
- `get_type_hierarchy` fallback path
- `get_ranked_context`
- import graph tools
- git-aware tools
- memory read/write/list operations
- onboarding/config/reporting that do not inherently require IDE state

### May remain adapter-only

- `get_open_files`
- `open_file_in_editor`
- `get_file_problems` with IntelliJ highlighting payloads
- `reformat_file` with IDE code style
- `get_run_configurations`
- `execute_run_configuration`
- PSI-backed safe rename execution

### Transitional dual-run candidates

- `rename_symbol`
- `get_type_hierarchy`
- `find_referencing_symbols`
- `find_symbol`

These need Rust-first paths but may temporarily fall back to IntelliJ when the Rust route lacks semantic confidence.

## Phase Plan

### Phase 0: Freeze the Target

Goal:

- make IntelliJ independence the explicit top-level product objective

Work:

- document the final goal and non-goals
- classify tools into Rust-core, adapter-only, and transitional buckets
- define what "product useful without IntelliJ" means

Exit criteria:

- roadmap approved in repo docs
- future work can be evaluated against the independence goal instead of feature accumulation

### Phase 1: Rust Core Contract Stabilization

Goal:

- make Rust the stable API surface for editor-independent code intelligence

Work:

- normalize Rust request/response models
- add shared metadata fields such as `backend_used`, `confidence`, `degraded_reason`
- separate transport-facing DTOs from internal engine types
- make error categories explicit rather than raw string-only failures

Exit criteria:

- Rust MCP and future Kotlin bridge can depend on stable core contracts
- new core tools do not require IntelliJ-shaped types

### Phase 2: Rust Semantic Baseline

Goal:

- cover the minimum symbolic workflow needed to replace standalone Kotlin/workspace mode

Work:

- add `workspace symbol` and `document symbol` flows
- add `type hierarchy` fallback path
- add `rename planning` via LSP `prepareRename` or equivalent
- improve symbol model parity across tree-sitter and LSP

Exit criteria:

- a non-IntelliJ client can search, inspect, cross-reference, and preflight edits through Rust alone

### Phase 3: Rust Index and Analysis Maturity

Goal:

- remove the remaining "prototype" feel from Rust core data handling

Work:

- decide whether to move from JSON cache to SQLite-backed structured index
- store file hash, mtime, symbol spans, imports, and derived metadata in structured form
- support incremental refresh and stale-file detection
- prepare import graph, ranked context, and git-aware features on top of the same store

Exit criteria:

- Rust indexing is suitable for large repositories and repeated sessions
- core analysis tools no longer depend on ad hoc rescans

### Phase 4: Kotlin to Rust Bridge

Goal:

- make Kotlin stop owning editor-independent behavior

Work:

- define bridge boundary between Kotlin plugin and Rust runtime
- route editor-independent requests through Rust
- keep IntelliJ-only behavior in Kotlin
- add fallback rules for Rust unavailable / adapter unavailable cases

Exit criteria:

- Kotlin plugin behaves as a host and adapter, not the main engine
- duplicate standalone/workspace implementations begin to disappear

### Phase 5: Backend Inversion

Goal:

- invert product default from JetBrains-first to Rust-first

Work:

- change backend selection policy so editor-independent work prefers Rust
- keep IntelliJ only for PSI-native capabilities or high-confidence upgrades
- expose capability metadata so clients can see which backend answered

Exit criteria:

- the default conceptual backend is Rust
- JetBrains becomes an enhancement path instead of the baseline

### Phase 6: Build and CI Independence

Goal:

- stop making IntelliJ installation a prerequisite for mainline development

Work:

- make Rust test/build pipeline the primary CI lane
- isolate IntelliJ adapter validation into a separate lane
- remove assumptions that a local IntelliJ app exists on contributor machines for core work

Exit criteria:

- contributors can build and validate the main runtime without IntelliJ
- IntelliJ verification remains, but as adapter coverage

### Phase 7: Product Surface Reframing

Goal:

- align packaging and docs with the new architecture

Work:

- rewrite README and install guidance around `Rust engine + adapters`
- make standalone/client integrations first-class
- describe IntelliJ plugin as the premium adapter for IDE-native actions

Exit criteria:

- product messaging no longer implies IntelliJ is the root product form

### Phase 8: Editor Expansion After Independence

Goal:

- prove the new architecture by making non-IntelliJ usage first-class

Work:

- validate Cursor, Claude Code, Codex, and other MCP clients against the Rust runtime
- ship configuration helpers and launcher flows
- add smoke tests for real client attachment paths

Exit criteria:

- CodeLens is clearly usable and valuable outside IntelliJ

## Recommended Execution Order

1. Phase 0
2. Phase 1
3. Phase 2
4. Phase 3
5. Phase 4
6. Phase 5
7. Phase 6
8. Phase 7
9. Phase 8

This ordering is deliberate:

- do not bridge Kotlin to Rust before the Rust contract is stable
- do not invert backend defaults before Rust can handle the baseline semantic workflow
- do not rebrand the product before runtime ownership actually moves

## Detailed Backlog

### Track A: Rust-first semantic parity

- add `documentSymbol`
- add `workspace/symbol`
- add `typeHierarchy`
- add `prepareRename`-based rename planning
- unify symbol ID and name-path representation
- add confidence scoring per semantic response

### Track B: Rust-first analysis substrate

- choose JSON vs SQLite index direction
- add structured index schema if SQLite is chosen
- add incremental invalidation
- add import graph storage
- add ranked context on top of structured symbol metadata

### Track C: Kotlin adapter slimming

- map current Kotlin tool implementations to Rust-first or adapter-only buckets
- stop adding editor-independent logic to Kotlin
- define bridge invocation model
- route workspace/standalone logic through Rust

### Track D: Build and product inversion

- split CI into Rust-core and IntelliJ-adapter lanes
- remove local IntelliJ assumptions from mainline contributor flow
- update docs, examples, and install paths

## Decision Gates

### Gate 1: Rust baseline ready

Before Phase 4 starts, confirm:

- Rust supports the baseline symbolic workflow
- test coverage exists for symbols, references, diagnostics, and semantic fallbacks

### Gate 2: Kotlin bridge ready

Before Phase 5 starts, confirm:

- bridge overhead is acceptable
- fallback behavior is explicit
- adapter-only tools remain functional

### Gate 3: Product inversion ready

Before Phase 7 starts, confirm:

- Rust is the default runtime in practice
- docs will reflect reality rather than aspiration

## Risks

- LSP server behavior varies significantly by language and implementation
- structured index migration can consume time without visible user-facing wins if not tightly scoped
- Kotlin and Rust may drift if dual-run lasts too long
- premature rebranding can create a product story that the runtime does not yet support

## Immediate Next Actions

1. formalize the Rust-core vs adapter-only tool matrix
2. stabilize Rust core response metadata
3. add `workspace/symbol` and `typeHierarchy` to the pooled LSP path
4. draft the Kotlin-to-Rust bridge boundary

Until those are done, feature work that increases IntelliJ centrality should be treated as off-strategy.
