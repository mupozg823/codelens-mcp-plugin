# CodeLens vs Serena

This document answers a narrower question than marketing:

> Is CodeLens already a strict superset of Serena?

Current answer: **no**.

CodeLens is already stronger as a harness-native MCP layer. Serena is still stronger as an IDE/LSP-centric semantic backend. A real superset requires merging those two shapes instead of pretending one replaces the other.

## 1. Current Verdict

| Axis | Current winner | Why |
| --- | --- | --- |
| Harness ergonomics | CodeLens | Role-based surfaces, deferred bootstrap, bounded reports, durable jobs |
| Semantic retrieval for NL queries | CodeLens | Bundled embedding model and hybrid ranking with measured external benchmarks |
| Offline setup and cold start | CodeLens | Single Rust binary, no per-language server requirement by default |
| Deep type-aware editing/refactoring | Serena | LSP-first backend and JetBrains-backed move/inline/declaration/implementation |
| Memory and long-lived knowledge | Serena | Mature project/global memory model and onboarding workflow |
| Broad language-backend coverage | Serena | 40+ LSP-backed languages plus JetBrains backend |
| Benchmark/eval discipline | CodeLens | Explicit evaluation contract and external retrieval matrix in-repo |

If the target definition of "superior" is:

- better harness behavior under token pressure: CodeLens already wins.
- better semantic IDE replacement for all agent coding tasks: CodeLens does not win yet.
- strict superset of Serena capabilities: CodeLens does not qualify yet.

## 2. CodeLens Advantages Today

### 2.1 Tool-surface shaping is a first-class runtime concept

CodeLens explicitly models profiles, tiers, preferred namespaces, and deferred bootstrap controls in the MCP runtime itself. See:

- `crates/codelens-mcp/src/tool_defs/mod.rs`
- `crates/codelens-mcp/src/server/transport_http.rs`

This matters because harnesses do not fail only on code intelligence quality. They also fail on bootstrap noise, oversized `tools/list`, and exposing the wrong mutation tools too early.

The design is visible in:

- `preferred_namespaces` and `preferred_tiers`, which differ by profile (`planner-readonly`, `builder-minimal`, `reviewer-graph`, `refactor-full`)
- `preferred_bootstrap_tools`, which keeps refactor flows preview-first
- deferred loading support for `tools/list` and resource discovery

This is a real architectural difference from Serena. Serena is configurable, but CodeLens is more explicitly optimized for harness token discipline at runtime.

### 2.2 Mutation safety is encoded as runtime policy, not just user guidance

CodeLens blocks mutation tools in `refactor-full` unless fresh preflight evidence exists and matches the target path and symbol. See:

- `crates/codelens-mcp/src/mutation_gate.rs`

That gate distinguishes:

- missing preflight
- stale preflight
- path mismatch
- symbol-aware preflight required
- verifier-blocked mutation

This is a stronger "fail-closed" harness contract than a plain symbolic edit surface.

### 2.3 CodeLens has a genuine workflow layer above primitive tools

CodeLens is not just a toolbox. It has:

- composite reports
- artifact handles
- durable jobs with progress/cancellation
- session-scoped analysis reuse

See:

- `crates/codelens-mcp/src/tools/report_jobs.rs`
- `crates/codelens-mcp/src/tools/reports/impact_reports.rs`
- `crates/codelens-mcp/src/state.rs`

Serena has strong tools. CodeLens has stronger harness-oriented orchestration around those tools.

### 2.4 Retrieval is hybrid and quantitatively tracked

CodeLens has a bundled ONNX embedding model and measured external quality results across eight datasets. See:

- `crates/codelens-engine/src/symbols/reader.rs`
- `crates/codelens-engine/src/symbols/ranking.rs`
- `benchmarks/embedding-quality-phase3-matrix.md`
- `EVAL_CONTRACT.md`

The current matrix shows mixed but real external wins, including strong positives on `ripgrep`, `jest`, and `typescript`, with flat or negative behavior on some Python/JS app datasets. That is the right level of honesty: measurable upside, but not universal superiority.

## 3. Serena Advantages Today

These come from Serena's current public repo and docs:

- GitHub repo: [oraios/serena](https://github.com/oraios/serena)
- Tools docs: [List of Tools](https://oraios.github.io/serena/01-about/035_tools.html)
- Workflow docs: [The Project Workflow](https://oraios.github.io/serena/02-usage/040_workflow.html)
- Memories docs: [Memories & Onboarding](https://oraios.github.io/serena/02-usage/045_memories.html)
- JetBrains backend docs: [The Serena JetBrains Plugin](https://oraios.github.io/serena/02-usage/025_jetbrains_plugin.html)

### 3.1 Serena has a broader semantic backend story

Serena is built around LSP by default and can switch to a JetBrains-backed language intelligence backend. In its own docs, Serena explicitly positions the JetBrains plugin as the preferred option for IDE users and lists capabilities that CodeLens does not yet match consistently:

- type hierarchy
- declaration lookup
- implementation lookup
- move refactor
- inline symbol
- dependency/library indexing through the IDE

At code level this is not just documentation. The repo has:

- `src/solidlsp/language_servers/*`
- `src/serena/tools/symbol_tools.py`
- `src/serena/project_server.py`

That is a materially broader semantic-integration layer than CodeLens currently has.

### 3.2 Serena's memory layer is more mature

Serena has project and global memories, read-only and ignored patterns, onboarding, and explicit memory tools. See:

- `src/serena/project.py`
- `src/serena/tools/memory_tools.py`

CodeLens does have memory support in the repo, but memory is not yet as central to the public product shape as it is in Serena.

### 3.3 Serena's current README-level feature comparison is stronger than the old CodeLens table admitted

The old `README.md` "vs Serena" table in CodeLens understated Serena's editing/refactoring depth. Serena is no longer just "replace symbol body". It exposes rename, safe delete, insertion around symbols, and JetBrains-only advanced refactors.

That discrepancy is exactly why this document exists.

## 4. The Real Architectural Difference

The cleanest way to describe the two systems is:

- **Serena** = semantic backend for coding agents
- **CodeLens** = harness optimization layer for coding agents

Serena starts from IDE/LSP semantics and then exposes tools.
CodeLens starts from MCP/harness constraints and then compresses code intelligence into bounded answers.

Neither fully subsumes the other.

## 5. What CodeLens Must Add To Become A True Superset

### 5.1 Introduce a pluggable semantic backend interface

CodeLens already mixes tree-sitter, optional LSP, graph, and embeddings, but the abstraction boundary is still too engine-internal.

The missing move is a single backend contract for:

- symbol lookup
- overview
- references
- declaration
- implementation
- type hierarchy
- rename preview
- workspace edit application
- safe delete
- move / inline / change signature

Recommended shape:

- `TreeSitterBackend` for fast always-on fallback
- `LspBackend` for language-server semantics
- future `IdeBackend` for JetBrains-like remote semantic adapters

Then tool capabilities become backend-derived rather than hard-coded product claims.

### 5.2 Separate "retrieval backend" from "edit backend"

Right now CodeLens is strongest when doing:

- candidate collection
- bounded ranking
- graph-backed compression
- semantic reranking

It is weaker when asked to guarantee IDE-grade edits.

Do not force one subsystem to be both.

Recommended split:

- `RetrievalOrchestrator`: tree-sitter + DB + graph + embeddings
- `SemanticEditBackend`: LSP or IDE-backed workspace-edit engine
- `WorkflowLayer`: reports, jobs, sections, policies, gating

That keeps CodeLens fast by default and deep when needed.

### 5.3 Add capability-driven routing per tool, per project, per language

`get_capabilities` already exposes part of this story. Push it further so every high-level tool can answer:

- which backend is active
- whether the result is syntax-grade or semantic-grade
- confidence and fallback reason
- whether preview/apply are both supported

Then the agent can choose:

- fast approximate answer
- slower semantic answer
- block because no safe semantic backend exists

That would be a real superset move, because it lets CodeLens keep its harness edge without bluffing about semantic completeness.

### 5.4 Expand mutation gating into workspace-edit transactions

The current mutation gate is good. The next step is transactionality:

- preview edit set
- deterministic apply
- rollback metadata
- post-apply diagnostics/references verification

Serena wins today partly because IDE/LSP edits naturally come as structured workspace edits. CodeLens should adopt that as the semantic edit substrate instead of treating each mutation mostly as a separate tool call.

### 5.5 Upgrade memory from utility to system layer

To beat Serena cleanly, CodeLens needs a first-class long-lived knowledge layer:

- project memory
- global memory
- read-only memory policies
- ignored/archive patterns
- artifact-linked memory entries
- session-derived memory suggestions

The important distinction is that CodeLens should connect memory to:

- analysis handles
- benchmark evidence
- mutation audit
- project activation

That would make memory more than notes. It becomes harness state.

### 5.6 Add a Serena-class semantic benchmark suite

The current evaluation stack is good for retrieval and harness promotion, but not sufficient for a "better than Serena" claim.

Add benchmark families for:

- rename correctness
- safe delete precision
- declaration/implementation accuracy
- type hierarchy coverage
- multi-project query latency
- dependency/library symbol retrieval
- edit-preview/apply fidelity

Until those exist, "strictly better than Serena" is not a scientific statement. It is only a preference.

## 6. Concrete Roadmap

### Phase A — Stop overstating

- keep the README comparison honest
- report capability gaps explicitly via `get_capabilities`
- expose backend/fallback provenance in tool outputs

### Phase B — Superset substrate

- add `SemanticBackend` and `SemanticEditBackend` traits
- route rename/type/declaration/implementation through backend capability checks
- keep tree-sitter retrieval as the fast fallback

### Phase C — Transactional refactors

- unify preview/apply/verify into one workspace-edit pipeline
- make mutation gate consume backend capability and transaction evidence

### Phase D — Memory + project fabric

- promote memory to a first-class public surface
- bind memories to projects, artifacts, and sessions
- make cross-project querying a durable subsystem, not a side feature

### Phase E — Prove it

- run semantic-refactor benchmarks against representative repos
- run harness token/latency comparisons against Serena-equipped workflows
- publish the failure cases, not just the wins

## 7. Bottom Line

If the question is:

> "Is CodeLens already clearly superior to Serena?"

The answer is:

- **for harness-native bounded workflows**: mostly yes
- **for deep semantic IDE behavior**: no
- **as a strict overall superset**: no

If the question is:

> "Can CodeLens be designed into a superior superset?"

The answer is yes, but only if it stops treating tree-sitter, LSP, embeddings, memory, and refactoring as one flat tool pile and instead becomes:

- a fast retrieval layer
- a pluggable semantic backend layer
- a transactional workflow/policy layer
- a persistent project knowledge layer

That is the architecture that can actually surpass Serena instead of only beating it on one axis.
