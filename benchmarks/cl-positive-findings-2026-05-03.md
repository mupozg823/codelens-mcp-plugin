# CodeLens — Positive Findings (dogfood 2026-05-03)

Source: Depixelate V3 (`/Users/bagjaeseog/3.0`, ~7K LOC Python) parent harness
session. Build under test: `e45f1ceb` v1.13.22. Profile mostly
`reviewer-graph` (token_budget 2400) with one `planner-readonly` bootstrap.

This file accompanies the `## What CodeLens Does Well` README section. Goal:
record concrete situations where CodeLens replaced a multi-step native flow
with a single tool call so they can be surfaced to new agents/users.

## 1. `prepare_harness_session` — single-call bootstrap

```jsonc
mcp__codelens__prepare_harness_session({
  "project": "/Users/bagjaeseog/3.0",
  "profile": "planner-readonly",
  "detail": "compact"
})
```

Returned in one response:

- `activated: true`, `project_name: "3.0"`, `indexed_files: 137`
- `index_recovery: { status: "not_needed" }` (auto-refresh logic ran first)
- `capabilities: { intelligence_sources: ["tree_sitter"], available: [symbols, imports, calls, rename, search, blast_radius, dead_code, type_hierarchy_native] }`
- `visible_tools.tool_count: 34` with full tool name list
- `daemon_binary_drift: { status: "ok" }`
- `health_summary: { status: "ok" }`

Native equivalent ≈ `ls` + `cargo build --version` + `python -c 'import …'` +
"check if MCP daemon matches binary" — multi-call, multi-second, requires
shell knowledge.

**Caveat**: subsequent `get_symbols_overview` calls hit the 2400-token budget
and clipped 95-symbol files to 3 visible entries. See dogfood [#170 follow-up](https://github.com/mupozg823/codelens-mcp-plugin/issues/170#issuecomment-comments)
for the in-array truncation marker proposal.

## 2. `find_symbol(name=…)` — definition + tests in one shot

Used to locate `diagnose_system` and `evaluate_mosaic_observability`:

```jsonc
mcp__codelens__find_symbol({ "name": "diagnose_system" })
// → 1 def (mosaic_equation_accumulator.py:111) + 2 unit tests in 1 response
```

Native equivalent ≈ `grep -rn "diagnose_system" depixelate_v3/` (returns
13+ candidates including doc strings/comments to filter manually).

**Caveat**: `name_path` parameter is silently rejected (must be `name`),
`include_body=true` did not return body on the tree-sitter backend during
this session — see [#170](https://github.com/mupozg823/codelens-mcp-plugin/issues/170)
and [#172](https://github.com/mupozg823/codelens-mcp-plugin/issues/172).

## 3. `review_changes` — quantified pre-merge gate

Across three Depixelate V3 changesets the tool returned a single
verifier verdict per change instead of requiring manual diff inspection.

| Changeset (project SHA)             | Files | `readiness_score` | `risk_level` | `blocker_count` |
| ----------------------------------- | ----- | ----------------- | ------------ | --------------- |
| F4 overlay export (aed74aa)         | 3     | 1.0               | low          | 0               |
| F6 adapter hardening (82bee8a)      | 2     | 0.75              | medium       | 0               |
| diagnose↔gate integration (547c19a) | 1     | 1.0               | low          | 0               |
| F5 mode recommendation (8ecc1f2)    | 3     | 1.0               | low          | 0               |

The 0.75 / `medium` case was a heuristic false positive (backend Python
classified as Browser/SSR-sensitive) — see [#171](https://github.com/mupozg823/codelens-mcp-plugin/issues/171).
Once the file-extension gate lands, this score should reach 1.0 too.

Native equivalent: read every changed file, run tests/lint manually,
guess at blast radius, look for callers — no quantified comparable.

## How to update this file

Append a new dogfood session as `## N. <title>` whenever a CodeLens call
replaces ≥ 3 native steps. Cross-link to dogfood issues when behaviour
diverges from expectations. Keep the existing `bench-accuracy-and-usefulness-*`
files for retrieval-quality benchmarks; this file is for _workflow_ wins.
