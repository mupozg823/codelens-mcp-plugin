# Serena-Pattern Harness Alignment — Design

- **Date**: 2026-06-10
- **Status**: Proposed (docs-first; implementation gated on user approval — SP-3c candidate scope)
- **Scope**: symbolic read core completion, tool-surface hygiene, gated symbolic edit re-listing, plugin-side enforcement layer, memory reference conventions
- **Evidence base**: Serena v1.5.3 source audit (tag checkout), live serena MCP surface in a Claude Code session, `docs/serena-comparison.md` (2026-06-10 revision), ADR-0008/-0009/-0010/-0013
- **Related**: `docs/adr/ADR-0008-serena-upper-compatible-absorption.md`, `docs/adr/ADR-0009-mutation-trust-substrate.md`, SP-4 plugin packaging (`2026-06-08-codelens-claude-plugin-packaging-design.md`)

## Problem

Serena v1.3→v1.5 executed a harness-engineering program (tool diet, enforcement hooks, host-context
prompts, memory conventions) on top of its symbolic core. CodeLens meanwhile carries an
**inconsistent middle state**: ~20 tools are dispatched but absent from `tools.toml` (no schema, no
`tools/list` visibility, no schema pre-validation), six of them sit in `BUILDER_MINIMAL_TOOLS` marked
"deprecated, will be removed in v2.0", three workflow tools (`analyze_change_request`,
`onboard_project`, `orchestrate_change`) are referenced by preset constants and by the generated
routing blocks in CLAUDE.md/AGENTS.md yet can never be discovered by an agent, and the public
`Mutation Gate Protocol` documents tools that do not exist on any surface.

The goal is to be *deliberately* one thing: a symbolic-first code-intelligence coprocessor with
harness-grade bounding — not a read-only report engine with ghost limbs.

## Design principles (inherited, non-negotiable)

1. **tree-sitter-first, LSP opt-in** — measured choice; no LSP-everything pivot.
2. **tools.toml is the only schema source** (ADR-0013); anything not in it does not exist.
3. **Fail-closed mutation** (ADR-0009 direction): verifier-gated, evidence-bearing, audited.
4. **Advisory in responses, deterministic at the plugin boundary** — the server suggests
   (`suggested_next_tools`); only host-side hooks may deny, and only opt-in.
5. **Measure before defaulting** — every behavioral lever ships observable counters first.

## Decisions

| # | Decision | Serena evidence | CodeLens substrate |
|---|----------|-----------------|--------------------|
| D1 | Surface 3 LSP read tools: `find_declaration`, `find_implementations`, `get_diagnostics_for_symbol` | v1.3.0 added all three + `get_diagnostics_for_file`; completes the overview→find→refs→decl→impl→diag navigation loop | LSP client, union references, readiness states, `search_workspace_symbols`, `get_lsp_recipe`, `get_file_diagnostics` all exist |
| D2 | 3-way surface drift gate: dispatch ∪ tools.toml ∪ preset_tags must reconcile; resolve every ghost | Serena's `ToolRegistry._deleted_tools` is an explicit tombstone list — deleted means deleted | `audit_tool_surface_consistency` exists; extend + CI gate |
| D3 | Re-list a 4-tool symbolic edit core behind the mutation gate: `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `rename_symbol`. Delete the line-edit family from dispatch | Serena ships exactly this core (+ safe_delete) as primary path; marks line-edit tools (`delete_lines`/`replace_lines`/`insert_at_line`) Optional = effectively off | Handlers exist in `tools/mutation`; `verify_change_readiness` gate exists; `edit_transaction.rs` substrate exists |
| D4 | Plugin-side enforcement hook (opt-in): non-symbolic streak counter → soft `additionalContext` reminder; strict deny only via env opt-in | `hooks.py` PreToolUse counters (grep≥3 / read≥3 / mixed) → deny + reminder; auto-approve own tools; Bash command-string heuristics | SP-4 plugin exists (`.claude-plugin/`, hooks deliberately not bundled yet — this is the principled first hook) |
| D5 | Task→tool mapping table in plugin skills (not a system-prompt override) | `cc_system_prompt_override` mapping table + "Disallowed reasoning" list; claude-code context excludes host-duplicated tools | Plugin skills `analyze`/`code-review`/`onboard` exist; CLAUDE.md routing blocks are generated |
| D6 | Memory reference convention: `[[name]]` links, rename propagation, integrity audit | `mem:` convention + `rename_memory` propagation + `serena memories check` + `memory_maintenance` seed | memory tool family + `audit_memory_consistency` exist |
| D7 | Description lint: no tool-name cross-references in new descriptions; chaining stays in `suggested_next_tools` | v1.5.0 rewrote descriptions for tool-search friendliness | Measured 5/87 offenders today; cheap lint in `regen-tool-defs.py --check` |

## What we explicitly do NOT adopt

- **Thinking/meta tools** — Serena deleted all six of theirs (`think_about_*`, `summarize_changes`,
  `switch_modes`, `check_onboarding_performed`). Validates never shipping them.
- **Per-edit diagnostics embedding** — Serena built `EditingToolWithDiagnostics`, then disabled it
  (`ENABLE_DIAGNOSTICS=False`) because mid-task edits legitimately introduce transient diagnostics.
  CodeLens keeps diagnostics as a *chained* step (`suggested_next_tools` → `get_file_diagnostics`).
- **System-prompt override** — `cc_system_prompt_override` replaces the host's system prompt
  wholesale. Out of scope for an MCP server's contract; the mapping table goes in plugin skill
  instructions instead.
- **Blind-trust editing prompts** — Serena's "you never need to verify the results" raises
  throughput but removes the safety net. CodeLens keeps the verifier gate; the trust statement we
  *can* make is narrower: "on success, do not re-read the file; chain `get_file_diagnostics`."
- **LSP adapter breadth race** — 60 languages is Serena's moat; ours is bounded hybrid retrieval.
- **`name_path` hierarchical addressing** — `find_symbol` already canonicalized on `name` +
  `symbol_id` (`file#kind:name_path`); `symbol_id` covers Serena's `Foo/__init__` use case without
  re-introducing a deprecated parameter.

## D1 — LSP read tool surfacing (P1)

Three new `tools.toml` entries, dispatched to thin handlers over the existing LSP session manager:

- `find_declaration(name|symbol_id, file_path?)` → declaration site; SCIP fallback when index
  present; otherwise `fallback_hint` → `find_symbol`/`bm25_symbol_search`, `degraded_reason`.
- `find_implementations(name|symbol_id, file_path?)` → implementors of trait/interface/abstract;
  tree-sitter heuristic fallback (impl blocks / extends-implements clauses) clearly labeled
  `confidence: syntax_grade`.
- `get_diagnostics_for_symbol(name|symbol_id, file_path)` → filter of `get_file_diagnostics` to the
  symbol's range; pure composition, no new engine work.

Preset membership: `reviewer-graph` + `builder-minimal` + `planner-readonly` (read-only annotations
`ro_p`). Descriptions follow D7 (self-contained, `[CodeLens:Symbol]` prefix, no tool-name refs).

**AC-D1**: `tools/list` under reviewer-graph shows all three with schemas; LSP-absent project
returns structured fallback (not error); `regen-tool-defs.py --check` and `surface-manifest.py
--check` green; contract tests per tool (LSP present / absent / SCIP-only).

## D2 — Surface hygiene: 3-way drift gate (P1, smallest, do first)

Extend `audit_tool_surface_consistency` to diff three sets and fail CI on any unexplained delta:

1. dispatch table keys (all registration styles: match arms + `Arc::new` handler registrations)
2. `tools.toml` names
3. preset constants / `preset_tags` ∪ generated routing blocks

Measured today (2026-06-10): dispatch-only = 20 (mutation family ×16 + `analyze_change_request`,
`onboard_project`, `orchestrate_change`, `propagate_deletions`); preset-dead strings in
`PLANNER_READONLY_TOOLS`/`REVIEWER_GRAPH_TOOLS`/`BUILDER_MINIMAL_TOOLS`.

Resolution per ghost (the explicit tombstone-or-register rule, Serena `_deleted_tools` pattern):

- `analyze_change_request`, `onboard_project`, `orchestrate_change`: **register in tools.toml**
  (they are load-bearing in routing docs and CLAUDE.md bootstrap sequences) — or, if measurement
  says they duplicate `prepare_harness_session`/`explore_codebase`, tombstone + scrub routing docs.
  Decision input: `get_tool_metrics` 30-day counts (ADR-0010 telemetry diet).
- Mutation family: per D3 (4 re-listed, rest tombstoned).
- Tombstone list lives in code (like Serena's `_deleted_tools`) so re-introduction under the same
  name fails a test.

**AC-D2**: new audit section `surface_drift {dispatch_only, schema_only, preset_dead}` returns
empty on main; CI step fails on non-empty; tombstone regression test.

## D3 — Symbolic edit core re-listing (P2, gated on ADR-0009 phases)

Re-list exactly four tools in `tools.toml` with full schemas + output_schemas:

| Tool | Contract |
|------|----------|
| `replace_symbol_body` | whole-symbol replacement; tree-sitter range authority; refuses if symbol not uniquely resolved |
| `insert_before_symbol` / `insert_after_symbol` | anchored insertion (imports → before first symbol; new fn → after last); refuses on ambiguous anchors (Serena v1.2.0 learned this: restricted `insert_after_symbol` on assignments) |
| `rename_symbol` | existing verifier-gated path; `safe_rename_report` stays the preflight |

Not re-listed (tombstoned from dispatch): `insert_content`, `insert_at_line`, `replace`,
`replace_content`, `replace_lines`, `delete_lines`, `create_text_file`, `add_import` — the host's
native Edit/Write covers line/content edits better than an MCP round-trip, and Serena itself keeps
the line-edit family Optional-off. `propagate_deletions` folds into a future `safe_delete` check
(reference-checked, Serena `SafeDeleteSymbol` pattern) when ADR-0009 audit substrate lands.

Gate semantics unchanged and mandatory: `verify_change_readiness` (or `safe_rename_report` for
rename) within preflight TTL → mutation → `suggested_next_tools: [get_file_diagnostics]`. Response
carries `ApplyEvidence` per ADR-0009. **The gate is the differentiator vs Serena's blind trust —
keep it.**

Surface: `builder-minimal` only (mutation daemon / refactor-full alias). Read-only daemons never
list them.

**AC-D3**: 4 tools listed under builder-minimal with schemas; mutation without fresh preflight →
structured gate refusal; nextest mutation contract suite green; tombstoned names return
`MethodNotFound` + test asserting they are not in dispatch.

## D4 — Plugin enforcement hook (P2, Rust 0, plugin-side)

New `hooks/` in `.claude-plugin` bundle (first bundled hook — SP-4 deliberately shipped none; this
one is the principled exception because it is user-value, not repo-internal tooling):

- `PreToolUse` matcher on `Read|Grep|Glob|Bash` → `codelens-hook route-reminder` (small script, no
  server dependency):
  - parses Bash commands with the same heuristic families as Serena (`grep|rg|ag|ack` /
    `cat|head|tail|sed|less|bat`), counts only code-file targets;
  - session-scoped counter file under `~/.codelens/hook_data/<session_id>/`;
  - streak ≥3 grep-type or ≥3 read-type or ≥5 mixed → emit `additionalContext` reminder naming the
    symbolic alternative (`get_symbols_overview`, `find_symbol include_body=true`,
    `find_referencing_symbols`, `semantic_search`);
  - counters reset on any `mcp__codelens__*` call;
  - `CODELENS_HOOK_STRICT=1` upgrades reminder → `permissionDecision: deny` (Serena default; our
    opt-in).
- `SessionEnd` → cleanup of the session's counter dir.

Soft-default rationale: deny-by-default conflicts with "성공은 조용히" harness principle and with
hosts where native Read is legitimately better (non-code files, tiny repos). Ship soft, count
fires via the counter files, revisit default with data (principle 5).

**AC-D4**: `validate-plugin-manifest --check` green with hooks block; hook fires on a scripted
3-grep streak (fixture test via `serena-hooks`-style stdin JSON harness); strict mode denies;
non-code files never counted; counters reset on codelens tool use.

## D5 — Plugin skill mapping table (P2, docs-only)

Add to plugin skills (`analyze`, `code-review`, `onboard`) a shared instructions block:

- task→tool mapping table (structure: see file's structure → `get_symbols_overview`; read one
  symbol → `find_symbol include_body=true`; who calls X → `find_referencing_symbols`; NL question →
  `semantic_search`; pre-edit → `verify_change_readiness`; post-edit → `get_file_diagnostics`).
- a 3-item "disallowed reasoning" list (the Serena pattern, toned to advice): "the file is small",
  "I already know the path", "one Read is faster than three tool calls" — each answered with the
  measured counter-evidence (6.1× token efficiency, docs/benchmarks).
- explicit non-claims: when native Read/Grep is genuinely better (single known file <30 LOC,
  non-code files, regex content audits) — honesty keeps the instruction credible, mirroring the
  scenario matrix already in CLAUDE.md.

**AC-D5**: skills render the block; no contradiction with generated CLAUDE.md routing blocks
(`surface-manifest.py --check` green).

## D6 — Memory reference conventions (P3)

- `[[name]]` (repo-wide existing wiki-link habit) recognized in memory bodies.
- `rename_memory` rewrites `[[old]]` → `[[new]]` across memories (Serena propagation pattern);
  read-only memories excluded.
- `audit_memory_consistency` gains a `references` section: dangling `[[x]]`, orphan memories
  (no inbound links, no recent reads).
- Optional `memory_maintenance` seed on `onboard_project` (if D2 keeps it) describing conventions.

**AC-D6**: rename propagation test; audit reports dangling/orphans on fixture; `..` traversal
already guarded (verify test exists, add if not — Serena v1.2.0 security fix parity).

## D7 — Description lint (P1, trivial)

`regen-tool-defs.py --check` gains: description of tool X must not contain the name of tool Y
(word-boundary match) unless whitelisted (`get_current_config`↔`activate_project` pair is
borderline-legitimate; decide at implementation). Today's 5 offenders fixed in the same PR.

**AC-D7**: lint green on main; intentionally violating fixture fails.

## Phasing

| Phase | Items | Size | Risk |
|-------|-------|------|------|
| P1 | D2 (drift gate + ghost resolution), D7 (lint), D1 (3 read tools) | S+S+M | low — read-only, additive |
| P2 | D3 (edit core re-list), D4 (plugin hook), D5 (skill table) | M+M+S | medium — D3 needs ADR-0009 phase alignment; D4 is opt-in |
| P3 | D6 (memory conventions) | M | low |

P1 lands without touching mutation semantics — it is pure honesty + read-core completion and can
ship in one sprint. P2's D3 is the strategic bet ("symbolic-first rebirth") and should ride the
ADR-0009 substrate timeline rather than fork it.

## Open questions (for grill before implementation)

1. D2: register vs tombstone for `analyze_change_request`/`onboard_project`/`orchestrate_change` —
   needs `get_tool_metrics` 30-day data; if the daemons show zero calls (they are unlistable, so
   likely), the deciding input is whether the *routing docs* should keep teaching them.
2. D3: does `insert_after_symbol` need the Serena v1.2.0 assignment-anchor restriction in
   tree-sitter terms (anchor must be a top-level item, not a statement)?
3. D4: hook script language — POSIX sh (zero-dep) vs the existing Python script conventions
   (`scripts/*.py`); plugin portability favors sh, test harness favors Python.
4. D6: `[[name]]` vs `mem:name` syntax — repo memories already use `[[...]]`; staying native avoids
   a migration but loses backtick-safety in markdown renderers.
