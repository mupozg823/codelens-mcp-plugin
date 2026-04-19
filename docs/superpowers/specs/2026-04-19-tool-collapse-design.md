# Tool Collapse — 10 Primitives over 86 Advanced Handlers

**Date**: 2026-04-19
**Status**: Design — awaiting user review
**Scope**: CodeLens MCP surface
**Origin**: Architecture audit follow-up to the 2026-04-19 transparency layer (Phases 1-3) and the Downstream Quality mini A/B, which established that compression is context-dependent and agents benefit from uniform, self-explaining surfaces.

## 1. Problem

CodeLens currently exposes 86 MCP tools. An agent must:

- load all 86 tool schemas into initial context (~15-20 KB of tool definitions),
- pick the right tool for each step (large decision space → wrong-tool errors),
- re-pick correctly when the first choice fails to retrieve (Downstream Quality Q1: ambiguous disambiguation, Q2: branch-local retrieval failure).

Transparency work (Phase 1-3) closed the "silent decisions" class of bugs but did not reduce the surface. The agent still has to choose from 86. Two failure modes persist:

1. **Surface cost**. Every cold session pays the full 86-tool schema tax, much of which the agent never uses in a given task.
2. **Decision cost**. Agents confronted with near-duplicates (`find_symbol` vs `bm25_symbol_search` vs `search_workspace_symbols`) pick reflexively, often sub-optimally.

## 2. Goal

Expose a 10-primitive surface by default. Each primitive is a semantic unit (ask / find / read / edit / verify / explore / impact / session / diagnose / analyze). Primitives dispatch internally to the existing 86 handlers. The 86 stay intact, reachable via an explicit `advanced` preset.

Outcome:

- Default surface ≈ 10 schemas. Initial prompt cost drops ~80 % for the typical session.
- Fewer wrong-tool errors because the agent's choice space contracts to one-per-intent.
- Power-user and migration-safe: `set_preset advanced` restores the full surface in one call.
- Transparency guarantees from Phase 1-3 carry through: primitives aggregate and forward `decisions[]` from their internal calls.

Non-goal: changing internal handler logic, the engine, or the decision kinds. This is a surface-layering change.

## 3. The 10 primitives

| #   | Primitive      | Input shape (summary)                                                                                                                                                                                                                                              | Dispatches to                                                                                                                                                                              |
| --- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | `ask_codebase` | `{query, max_tokens?, include_body?}`                                                                                                                                                                                                                              | `get_ranked_context`                                                                                                                                                                       |
| 2   | `find`         | `{query, type: "symbol"\|"ref"\|"pattern"\|"file", file_path?, max_results?}`                                                                                                                                                                                      | `find_symbol` / `bm25_symbol_search` / `find_referencing_symbols` / `search_for_pattern` / `find_file_tool`                                                                                |
| 3   | `read`         | `{location, include_body?}` where `location` is one of: `"path/to/file.rs"` (whole file), `"path/to/file.rs:42"` (line — numeric suffix after last colon), `"path/to/file.rs#SymbolName"` (symbol — `#` disambiguator chosen to avoid colliding with line numbers) | `read_file` / `find_symbol include_body`                                                                                                                                                   |
| 4   | `edit`         | `{mode: "rename"\|"replace"\|"insert_before"\|"insert_after", location, change}`                                                                                                                                                                                   | `rename_symbol` / `replace` / `insert_before_symbol` / `insert_after_symbol`, preceded by `mutation_gate`                                                                                  |
| 5   | `verify`       | `{target_files: [...]}`; optional `symbol` triggers rename check                                                                                                                                                                                                   | `verify_change_readiness` / `safe_rename_report` / `get_file_diagnostics`                                                                                                                  |
| 6   | `explore`      | `{path?, depth?}`; empty = onboarding                                                                                                                                                                                                                              | `onboard_project` / `get_symbols_overview` / `get_project_structure`                                                                                                                       |
| 7   | `impact`       | `{symbol, file_path, depth?}`                                                                                                                                                                                                                                      | `get_impact_analysis` / `find_referencing_symbols` / `diff_aware_references`                                                                                                               |
| 8   | `session`      | `{op: "prepare"\|"claim"\|"release"\|"register"\|"audit_planner"\|"audit_builder"\|"export"\|"list", …}`                                                                                                                                                           | `prepare_harness_session` / `claim_files` / `release_files` / `register_agent_work` / `audit_planner_session` / `audit_builder_session` / `export_session_markdown` / `list_active_agents` |
| 9   | `diagnose`     | `{scope: "file"\|"lsp"\|"index", path?}`                                                                                                                                                                                                                           | `get_file_diagnostics` / `check_lsp_status` / `diagnose_issues`                                                                                                                            |
| 10  | `analyze`      | `{op: "start"\|"get"\|"section"\|"list", …}`                                                                                                                                                                                                                       | `start_analysis_job` / `get_analysis_job` / `get_analysis_section` / `list_analysis_artifacts`                                                                                             |

Each `{op, type, mode, scope}` discriminator chooses exactly one downstream handler; ambiguity is refused at the dispatch layer with a structured error pointing at the valid discriminator values.

## 4. Architecture

### 4.1 File map

```
crates/codelens-mcp/src/tools/primitives/
├── mod.rs            # registry, dispatcher trait, common types
├── ask_codebase.rs   # primitive #1
├── find.rs           # #2
├── read.rs           # #3
├── edit.rs           # #4  (mutation_gate integration)
├── verify.rs         # #5
├── explore.rs        # #6
├── impact.rs         # #7
├── session.rs        # #8
├── diagnose.rs       # #9
└── analyze.rs        # #10
```

`mod.rs` exposes a `Primitive` trait with:

```rust
pub(crate) trait Primitive {
    fn name(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;
    fn dispatch(&self, state: &AppState, args: &serde_json::Value) -> ToolResult;
}
```

Plus a `registry()` function returning a closed enum of the ten primitives.

### 4.2 Surface manifest

`crates/codelens-mcp/src/tool_defs/presets.rs` adds one preset variant:

- `Minimal` — unchanged (core tools — symbol/file/search + safe edits; legacy)
- `Balanced` — **NEW default** — 10 primitives + `set_preset` + metadata tools (`check_lsp_status`, `get_lsp_recipe`, `get_capabilities`)
- `Full` — primitives + all 86 existing tools
- Existing Profile variants unchanged, each pins a tool set independently of the preset lattice

`set_preset` is always visible regardless of preset so an agent can escalate.

### 4.3 Data flow

```
agent → MCP call (e.g. find)
  → dispatch (tool_defs) — verifies primitive visible in active preset
  → Primitive::dispatch(state, args)
    → discriminator switch (type / mode / op)
    → internal handler call (existing implementation)
    → internal handler returns (data, ToolResponseMeta {decisions, ...})
  → primitive wraps:
      • routing_provenance: {routed_to: "bm25_symbol_search", discriminator: {type: "symbol"}}
      • decisions[]: forwarded from internal meta
      • LimitsApplied::primitive_routed (new kind): "primitive routed to N internal tools"
  → returns (wrapped_data, merged_meta)
```

### 4.4 Transparency integration

- Phase 1-3's universal `decisions[]` field on the response root stays. A primitive forwards every `LimitsApplied` emitted by its internal call.
- New `LimitsKind::PrimitiveRouted` entry is added and populated automatically by the dispatcher, so every primitive response lists which underlying handler actually ran. `param` = `"routed_to=<handler_name>"`.
- `attach_decisions_to_meta` is reused from Phase 2 unchanged.

## 5. Error handling

- **Unknown discriminator** (e.g. `find` called with `type="magic"`): return `CodeLensError::Validation` listing the valid set. No internal dispatch attempted.
- **Internal handler error**: the primitive returns the error wrapped with `advanced_fallback_hint` = `{reason, try: [{tool, preset: "advanced", arguments}]}` so the agent can call `set_preset advanced` then invoke the handler directly.
- **Preset mismatch** (agent calls an advanced-only tool in `balanced`): existing surface-validation error now also carries `advanced_fallback_hint`.
- **Mutation gate** (`edit`): same as before — runs before any advanced mutation handler; failures surface as `mutation_gate` errors.

## 6. Rollout & breaking changes

One PR. Change notes (repo-local, no external consumers):

- Default `preset:balanced` now exposes **10 primitives + set_preset + metadata**, down from whatever the pre-collapse `balanced` held.
- Every existing tool continues to work under `preset:full` or after `set_preset advanced`.
- `benchmarks/transparency-reproducer.sh` and any other bench script that directly invokes an advanced tool prepends `export CODELENS_PRESET=full` (already done on 5571e15 for two tools; extend to all).
- `docs/release-verification.md` / changelog entry documents the surface change.

## 7. Testing

Three layers:

1. **Per-primitive unit test**: given each discriminator value, the dispatcher routes to the expected internal handler; invalid discriminators raise the right error. Mock-free; uses the real handlers with a small fixture.
2. **Dispatch-boundary integration**: 10 primitives × happy-path call on a synthetic project; asserts `success=true`, `decisions` field present (Phase 3 contract), `routing_provenance.routed_to` populated with an expected handler name.
3. **Advanced-preset regression**: pre-collapse integration tests (all 400+) must still pass when `CODELENS_PRESET=advanced` is active at their call site.
4. **Reproducer**: new `benchmarks/tool-collapse-reproducer.sh` exercises one invocation per primitive against `/tmp/serena-oraios`, verifies each prints an `ok routed_to=...` line.

## 8. Open questions / deliberate non-scope

- **`set_preset` auto-escalation.** Should a primitive's `advanced_fallback_hint` be permitted to auto-call `set_preset advanced` when the client opts in? Left for a follow-up; the manual path is sufficient for the initial landing.
- **Composite primitives for Phase 2 composites.** CodeLens already has `composite.rs` (e.g. `review_changes`, `explore_codebase` workflows). These stay as-is under advanced; primitives intentionally don't re-wrap them.
- **`find type=code_duplicates` / `type=dead_code`**. Semantic but niche; deferred.
- **Token-cost measurement.** A follow-up harness will quantify the initial-prompt saving on a real agent run.

## 9. Success criteria

- `cargo test -p codelens-mcp --all-features` passes at ≥ 428 + new primitive tests.
- `get_capabilities` under default preset returns exactly the 10 primitives + set_preset + metadata tools.
- `benchmarks/tool-collapse-reproducer.sh` prints 10 `ok` lines against the Serena fixture.
- `benchmarks/transparency-reproducer.sh` (Phase 1-3) keeps printing its 6 `ok` lines; reproducers either run under `preset=full` or invoke primitives instead of advanced tools directly.
- Pre-collapse integration tests that hit advanced tools work unchanged when migrated to `CODELENS_PRESET=advanced`.
- Every primitive response's `decisions[]` contains at least one `{"kind":"primitive_routed","param":"routed_to=<handler>"}` entry so agents can audit which internal handler actually served the call.

## 10. References

- `docs/superpowers/specs/2026-04-19-transparency-fields-design.md` — Phase 1-3 transparency layer this design stacks on.
- `docs/superpowers/plans/2026-04-19-transparency-layer-phase1.md`, `…-phase2.md` — implementation precedents for the shared emitter + dispatcher patterns.
- `crates/codelens-mcp/src/tool_defs/presets.rs` — preset / profile infrastructure this design extends.
- `crates/codelens-mcp/src/limits.rs` — `LimitsKind` enum that gains `PrimitiveRouted`.
- `benchmarks/downstream-quality-mini-results-2026-04-19.md` — the A/B whose retrieval-failure findings justify the "keep advanced reachable" constraint.
