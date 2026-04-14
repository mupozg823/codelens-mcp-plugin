# ADR-0003: Centralize Registry-Derived Guidance And Thin Workflow Entrypoints

- Status: Accepted
- Date: 2026-04-15

## Context

CodeLens already has a good top-level split between engine and MCP runtime.
The current maintenance risk is now mostly intra-layer drift:

- user-facing guidance can diverge from the registry that actually owns the truth
- workflow entrypoints can accumulate as thin aliases over existing tools
- architecture docs can become stale faster than the implementation

This review found one concrete example in live code:

- LSP recipes are authoritative in `codelens-engine/src/lsp/registry.rs`
- MCP-facing install guidance in `crates/codelens-mcp/src/tools/lsp.rs` was maintained separately

This is exactly the kind of duplication that looks harmless at first and then becomes silent product drift.

## Decision

We will treat registry-derived guidance and workflow alias layers as explicit architectural concerns.

### 1. Registry-derived guidance must come from the owning registry

Rule:

- if runtime guidance is a formatted view of an existing registry, the formatting layer must derive from that registry rather than restating the same mapping

Initial application:

- LSP install/error guidance derives from the engine LSP recipe registry

### 2. Workflow tools must justify their existence

A workflow entrypoint stays only if it adds at least one of:

- better routing defaults
- safer argument shaping
- better bounded output for agents
- policy-aware orchestration across multiple lower-level tools

A workflow entrypoint should not survive as a permanent public layer if it only renames another tool without improving behavior.

### 3. Architecture snapshots must be easy to refresh

Facts that drift frequently should be treated as generated or easily refreshable:

- release note pointers
- tool counts
- schema counts
- workspace version references

## Consequences

### Positive

- fewer silent drift points
- clearer ownership boundaries
- less duplicated product logic
- easier review of AI-generated changes

### Negative

- some helper APIs must move to the engine even when they seem presentation-oriented
- a few workflow tool names may need to be deprecated or documented as aliases

## Non-Goals

- rewriting the transport layer
- replacing workflow-first UX with primitive-only UX
- removing all aliasing regardless of user value

## Immediate Follow-Ups

1. Keep LSP install guidance registry-derived. **Done** — MCP side now calls `codelens_engine::get_lsp_recipe_for_command` instead of a hardcoded map.
2. Review `tools/workflows.rs` and classify wrappers. **Done** — 7 canonical workflows retained, 3 alias wrappers (`audit_security_context`, `analyze_change_impact`, `assess_change_readiness`) demoted to a `DEPRECATED_ALIASES` compatibility shim that attaches `deprecated: true`, `replacement_tool`, and `removal_target` to every response. Aliases are removed from preset recommendation surfaces (planner / builder / reviewer / refactor / CI audit / workflow-first) but remain callable until the removal target release.
3. Make directory-scope vs file-scope diagnosis explicit in `diagnose_issues`. **Done** — `path` is directory-only (validation error on file input), `file_path` is file-only, `symbol` requires `file_path`; missing-all returns `MissingParam`.
4. Reduce manual snapshot drift in `README.md` and `docs/architecture.md`. **Done** — snapshot counts/version links are now covered by `integration_tests::docs`, and version-pinned runtime-shape wording was removed from `docs/platform-setup.md` so the surrounding guidance no longer needs per-release manual edits.

## Related Work (2026-04-15 Workflow Thin Phase)

- `crates/codelens-mcp/src/tools/workflows.rs` — `attach_workflow_metadata` auto-attaches deprecation metadata by consulting `crate::tool_defs::deprecated_workflow_replacement`; `review_changes` now fails fast if both `changed_files` and `path` are absent instead of silently falling back to `impact_report`.
- `crates/codelens-mcp/src/tool_defs/presets.rs` — planner / builder / reviewer / refactor / CI-audit / workflow-first preset arrays no longer recommend the 3 deprecated aliases; canonical replacements (`semantic_code_review`, `impact_report`, `verify_change_readiness`) added where they were missing.
- `crates/codelens-mcp/src/resource_analysis.rs` — analysis-artifact tool counts are now aggregated via `canonical_tool_name`, so deprecated alias calls roll up under their replacement tool in resource / session dashboards.
- `crates/codelens-mcp/src/integration_tests/docs.rs` — public-doc drift guards now assert the current workspace version, tool/schema counts, release-note pointers, canonical workflow exposure, and the absence of version-pinned runtime-shape wording in platform setup guidance.
- Tests: `deprecated_workflow_aliases_return_replacement_metadata`, `review_changes_without_scope_returns_validation_error`, and the existing `workflow_alias_tools_return_structured_content_and_delegate` now asserts `deprecated: false` for canonical workflows.
