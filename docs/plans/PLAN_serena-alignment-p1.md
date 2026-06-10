# Implementation Plan: Serena-Pattern Alignment P1 (read core + surface hygiene)

**Status**: ⏳ Pending approval-to-start
**Started**: —
**Last Updated**: 2026-06-10
**Estimated Completion**: ~10–13h across 4 phases (1–2 sessions)

---

**⚠️ CRITICAL INSTRUCTIONS**: After completing each phase:

1. ✅ Check off completed task checkboxes
2. 🧪 Run all quality gate validation commands
3. ⚠️ Verify ALL quality gate items pass
4. 📅 Update "Last Updated" date above
5. 📝 Document learnings in Notes section
6. ➡️ Only then proceed to next phase

⛔ **DO NOT skip quality gates or proceed with failing checks**

---

## 📋 Overview

### Feature Description

Implements **P1** of `docs/superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md`
(decisions **D2 + D7 + D1**): make the tool surface honest (resolve 20 dispatch-only ghost tools,
gate 3-way drift in CI), keep descriptions tool-search-clean, and complete the symbolic **read**
navigation loop with `find_declaration` / `find_implementations` / `get_diagnostics_for_symbol`.

Out of scope (P2/P3): mutation core re-listing (D3), plugin enforcement hook (D4), skill mapping
table (D5), memory reference conventions (D6).

### Success Criteria

- [ ] 3-way drift (dispatch ∪ tools.toml ∪ presets) = ∅ on main, enforced by CI; pending-D3
      allowlist is the only sanctioned exception and is visible in the report
- [ ] 13 line-edit/refactor ghosts tombstoned (call → structured guidance, not silent absence);
      `analyze_change_request` / `onboard_project` / `orchestrate_change` listed with real schemas
- [ ] `find_declaration` / `find_implementations` / `get_diagnostics_for_symbol` appear in
      `tools/list` (planner-readonly / reviewer-graph / builder-minimal) with LSP-absent fallback
- [ ] All repo gates green: fmt, clippy ×2 axes, nextest (http,semantic + no-default), regen
      `--check`, surface-manifest `--check`, script contract tests
- [ ] Live daemon redeployed; dogfood probe of the 3 new tools recorded in Notes

### User Impact

Agents can trust `tools/list` (no doc-taught phantom tools), CI blocks future surface drift
structurally, and the symbolic read loop (overview → find → refs → declaration → implementations →
diagnostics) closes the navigation gap vs Serena v1.5.3 without giving up CodeLens's gates.

---

## 🏗️ Architecture Decisions

| Decision | Rationale | Trade-offs |
|----------|-----------|------------|
| Drift detection lives in `regen-tool-defs.py` (script codegen layer), runtime audit consumes a **generated** `DISPATCHED_TOOLS` const | Script already parses preset consts (`extract_preset_const`, `validate_preset_tags` orphan warnings); match-arm dispatch is not runtime-enumerable — codegen makes one source of truth | Dispatch edits require regen (already true for tools.toml; same muscle) |
| Register the 3 workflow ghosts instead of tombstoning | CLAUDE.md/AGENTS.md routing blocks + overlay compiler actively teach them; preset constants already include them; scrubbing docs has larger blast radius | Keeps 3 possibly-low-traffic tools; ADR-0010 telemetry can retire them later with data |
| Tombstone 13, allowlist 4 (`replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `rename_symbol`) as `pending-D3` | Line/content edits are covered by host-native Edit (Serena keeps its own line-edit family Optional-off); the 4 are the D3 re-list candidates — deleting them now would churn P2 | Allowlist = temporary sanctioned drift; must be revisited by D3 decision |
| Tombstoned call returns structured guidance (`MethodNotFound` + replacement hint), names kept in a `TOMBSTONED_TOOLS` const with re-introduction test | Serena `ToolRegistry._deleted_tools` pattern; direct `tools/call` users get a migration path | Slightly larger dispatch error path |
| `get_diagnostics_for_symbol` = composition over `get_file_diagnostics` + symbol range from index | Zero new engine surface; pure MCP-layer filter | Range accuracy bounded by tree-sitter symbol spans |
| `find_declaration`/`find_implementations` reuse engine `session_requests.rs` op mapping (`textDocument/declaration` / `textDocument/implementation` already mapped) | Engine support exists; this is surfacing work, not capability work | LSP-absent projects get fallback hints, not answers (labeled `degraded_reason`) |

---

## 📦 Dependencies

### Required Before Starting

- [ ] Worktree on a fresh branch from `main` (post `79791c9b` docs commit)
- [ ] Live daemons healthy (`bash scripts/daemon-stale-check.sh` — expect in-sync or STALE, not DIVERGENT)
- [ ] Issue context: [#346](https://github.com/mupozg823/codelens-mcp-plugin/issues/346) (this plan resolves it), #200 (preset codegen — adjacent, do not entangle), #287 (LSP edit backends — P2 territory)

### External Dependencies

None added. Python 3 stdlib only for script work; no new crates.

---

## 🧪 Test Strategy

**TDD Principle**: Write tests FIRST, then implement to make them pass.

This repo has no coverage-% tooling; coverage targets are expressed as **scenario checklists** per
phase (repo convention: contract tests + nextest). Test types used:

| Test Type | Vehicle | Purpose |
|-----------|---------|---------|
| Script contract tests | `scripts/test/test-*.py` (auto-collected by the "script contract tests" glob) | drift parser, lint, enforce-mode semantics |
| Rust unit/contract tests | `crates/codelens-mcp` `#[cfg(test)]` + `integration_tests/` | dispatch tombstones, admin audit section, new tool handlers |
| Protocol/integration tests | `integration_tests/protocol_tools_list.rs` 등 | tools/list membership per surface |
| Live dogfood probe | daemon redeploy + manual MCP calls | end-to-end evidence (recorded in Notes) |

Naming follows existing patterns (`test_<behavior>` Rust, `test_*` pytest-style functions in script tests).

---

## 🚀 Implementation Phases

### Phase 1: Script gate — 3-way drift report + description lint (warn-mode)

**Goal**: `regen-tool-defs.py --check` reports (warn-only) every dispatch/schema/preset
inconsistency and description cross-references; the 5 existing cross-ref descriptions are cleaned.
**Estimated Time**: ~2h
**Status**: ⏳ Pending

#### Tasks

**🔴 RED: Write Failing Tests First**

- [ ] **Test 1.1**: `scripts/test/test-regen-tool-defs-drift.py` — drift parser contract
  - Fixture Rust snippets containing both registration styles (`"name" =>` match arms and
    `.register("name", std::sync::Arc::new(handler))`); expect `parse_dispatch_names()` to find both
  - `three_way_report()` over fixture sets classifies `dispatch_only` / `schema_only` / `preset_dead`
  - Expected: FAIL (functions don't exist yet)
- [ ] **Test 1.2**: description cross-ref lint contract
  - Fixture tool list where one description names another tool → lint flags it; allowlisted pair
    passes; self-reference does not flag
  - Expected: FAIL

**🟢 GREEN: Implement to Make Tests Pass**

- [ ] **Task 1.3**: `parse_dispatch_names()` + `three_way_report()` in `scripts/regen-tool-defs.py`
  - Parse `crates/codelens-mcp/src/tools/mod.rs` + `crates/codelens-mcp/src/dispatch/table.rs`
    (both styles); regression-pin the parsed count against the live tree so style drift breaks loudly
  - Wire into `--check` as **stderr warnings, exit 0** (enforcement comes in Phase 2)
- [ ] **Task 1.4**: `lint_description_crossrefs()` + clean the 5 current offenders
  - Decide allowlist at implementation (candidate: `activate_project`↔`get_current_config`)
  - Edit `tools.toml` descriptions (`get_current_config`, `search_workspace_symbols`,
    `activate_project`, `find_redundant_definitions`, `index_embeddings`) → run regen `--write`,
    commit regenerated `build_generated.rs` verbatim

**🔵 REFACTOR**

- [ ] **Task 1.5**: dedup parser helpers with existing `extract_preset_const`; docstrings; keep
      `validate_preset_tags` orphan warning path delegating to the new report (single formatter)

#### Quality Gate ✋

- [ ] RED ran first and failed; GREEN made them pass; REFACTOR kept them green
- [ ] Script contract tests pass (incl. pre-existing `test-validate-plugin-manifest.py`)
- [ ] `python3 scripts/regen-tool-defs.py --check` exit 0, warnings list exactly the known 20 ghosts + preset-dead strings
- [ ] `cargo fmt --all -- --check` · `cargo nextest run --workspace --features http,semantic` (descriptions are metadata-only; full matrix deferred to Phase 2 gate)
- [ ] `python3 scripts/surface-manifest.py --check` green

**Validation Commands**:

```bash
python3 scripts/test/test-regen-tool-defs-drift.py
python3 scripts/regen-tool-defs.py --check        # exit 0 + drift warnings on stderr
python3 scripts/surface-manifest.py --check
cargo fmt --all -- --check && cargo nextest run -p codelens-mcp --features http,semantic
```

---

### Phase 2: Ghost resolution + gate promotion (hard-fail)

**Goal**: tools.toml ∪ dispatch ∪ presets reconcile; 3 workflow tools listed; 13 tools tombstoned;
drift check enforced in CI.
**Estimated Time**: ~3–4h
**Status**: ⏳ Pending

#### Tasks

**🔴 RED: Write Failing Tests First**

- [ ] **Test 2.1**: `integration_tests/protocol_tools_list.rs` — planner-readonly surface contains
      `onboard_project`, `analyze_change_request`, `orchestrate_change` with input schemas
      (currently absent → FAIL)
- [ ] **Test 2.2**: tombstone dispatch test — calling `insert_at_line` (representative of the 13)
      returns structured `MethodNotFound` guidance naming the replacement path (currently
      dispatches → FAIL); `TOMBSTONED_TOOLS` re-introduction test: every tombstoned name must NOT
      appear in dispatch or tools.toml
- [ ] **Test 2.3**: script enforce-mode contract — with `--enforce-drift`, unexplained drift exits 1;
      `pending-D3` allowlist (`replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`,
      `rename_symbol` dispatch-only) passes but is printed

**🟢 GREEN: Implement to Make Tests Pass**

- [ ] **Task 2.4**: author `tools.toml` entries for the 3 workflow tools — derive input schemas from
      handler arg extraction in their `tools/*.rs` bodies (not from docs); add `output_schema`
      entries; `preset_tags` matching today's preset constants (planner-readonly +/- reviewer-graph)
- [ ] **Task 2.5**: tombstone the 13 (`insert_content`, `insert_at_line`, `replace`,
      `replace_content`, `replace_lines`, `delete_lines`, `create_text_file`, `add_import`,
      `propagate_deletions`, `refactor_extract_function`, `refactor_inline_function`,
      `refactor_move_to_file`, `refactor_change_signature`): remove dispatch arms, add
      `TOMBSTONED_TOOLS: &[(&str, &str)]` (name → guidance) consulted by dispatch error path
- [ ] **Task 2.6**: presets cleanup — remove the 6-entry deprecated block from
      `BUILDER_MINIMAL_TOOLS` (the 4 allowlisted stay dispatch-only, NOT preset members until D3);
      remove dead strings; run regen `--write`; run `surface-manifest.py --write` (routing blocks
      will change — review diff deliberately)
- [ ] **Task 2.7**: CI — add `--enforce-drift` to the existing tool-defs drift step in
      `.github/workflows/ci.yml`

**🔵 REFACTOR**

- [ ] **Task 2.8**: dispatch error-path dedup (tombstone guidance vs generic MethodNotFound);
      comment the allowlist with D3 spec pointer

#### Quality Gate ✋

- [ ] RED→GREEN→REFACTOR evidenced in commit order
- [ ] Full repo matrix: `cargo fmt --all -- --check` · `cargo clippy --workspace -- -D warnings` ·
      `cargo clippy --workspace --no-default-features -- -D warnings` ·
      `cargo nextest run --workspace --features http,semantic` ·
      `cargo nextest run --workspace --no-default-features` · `cargo test --doc --workspace`
- [ ] `python3 scripts/regen-tool-defs.py --check --enforce-drift` exit 0 (allowlist printed)
- [ ] `python3 scripts/surface-manifest.py --check` green AFTER regeneration commit
- [ ] `python3 benchmarks/lint-datasets.py --project .` green (dataset rows may reference moved symbols)
- [ ] Manual: `tools/list` via local stdio run shows the 3 workflow tools; `tools/call insert_at_line` returns guidance

**Manual Test Checklist**:

- [ ] planner-readonly `tools/list` includes 3 workflow tools, excludes all 13 tombstoned
- [ ] CLAUDE.md/AGENTS.md regenerated routing blocks reviewed — bootstrap chains now reference only listable tools
- [ ] Direct `tools/call rename_symbol` still dispatches (allowlist behavior unchanged pending D3)

---

### Phase 3: Runtime audit extension — `surface_drift` section

**Goal**: `audit_tool_surface_consistency` reports the same 3-way truth at runtime from the
generated const; tombstone regression locked.
**Estimated Time**: ~2–3h
**Status**: ⏳ Pending

#### Tasks

**🔴 RED: Write Failing Tests First**

- [ ] **Test 3.1**: admin contract test — response contains
      `surface_drift: {dispatch_only: [...pending-D3 only], schema_only: [], preset_dead: []}`
      (section doesn't exist yet → FAIL)
- [ ] **Test 3.2**: tombstoned name passed to audit's "explain tool" path (or dispatch) yields the
      guidance string, not a panic/empty

**🟢 GREEN: Implement to Make Tests Pass**

- [ ] **Task 3.3**: emit `DISPATCHED_TOOLS` const from `regen-tool-defs.py` into
      `tool_defs/generated/build_generated.rs` (or sibling generated file) — script is the single
      parser; runtime never re-parses source
- [ ] **Task 3.4**: extend `audit_tool_surface_consistency` (`tools/admin/mod.rs:130`) to compute
      the 3-way diff from `DISPATCHED_TOOLS` × tool registry × preset membership; include
      `pending_d3_allowlist` and `tombstoned_count` fields; update its `output_schema` if present

**🔵 REFACTOR**

- [ ] **Task 3.5**: share set-diff helper with any existing audit internals; keep response bounded
      (lists capped + `_omitted_count` per repo truncation convention)

#### Quality Gate ✋

- [ ] TDD evidence; admin contract tests green
- [ ] `cargo nextest run -p codelens-mcp --features http,semantic` + no-default axis green
- [ ] regen `--check --enforce-drift` + surface-manifest `--check` green (generated const is fresh)
- [ ] Manual: run audit via stdio; `surface_drift` empty except allowlist

---

### Phase 4: D1 — surface 3 LSP read tools

**Goal**: `find_declaration`, `find_implementations`, `get_diagnostics_for_symbol` listed and
working with graceful degradation; daemons redeployed; dogfood evidence recorded.
**Estimated Time**: ~3–4h
**Status**: ⏳ Pending

#### Tasks

**🔴 RED: Write Failing Tests First**

- [ ] **Test 4.1**: contract tests ×3 tools — mock-LSP path (reuse the mock LSP harness from the
      P0-2 union work): declaration/implementations return locations with `confidence:
      semantic_grade`; expected FAIL (handlers absent)
- [ ] **Test 4.2**: LSP-absent path — response carries `degraded_reason` +
      `fallback_hint` (→ `find_symbol` / `bm25_symbol_search`), exit success not error
- [ ] **Test 4.3**: `get_diagnostics_for_symbol` filters `get_file_diagnostics` to the symbol's
      span (fixture file with 2 symbols, diagnostics in both → only target symbol's returned)
- [ ] **Test 4.4**: `protocol_tools_list` membership — all three visible under planner-readonly /
      reviewer-graph / builder-minimal

**🟢 GREEN: Implement to Make Tests Pass**

- [ ] **Task 4.5**: handlers in `tools/lsp/` (`navigation.rs` new or extend `symbols.rs`):
      `find_declaration` / `find_implementations` via engine `session_requests.rs` op names
      (`declaration`, `implementation`); resolve target position from index (`find_symbol`
      resolution path), convert locations with existing helpers
- [ ] **Task 4.6**: `get_diagnostics_for_symbol` in `tools/lsp/diagnostics.rs` — symbol span from
      symbols index, filter diagnostics by range overlap
- [ ] **Task 4.7**: `tools.toml` entries (D7-clean descriptions, `[CodeLens:Symbol]` prefix,
      `preset_tags` ×3 surfaces, `annotations: ro_p`, `output_schema`s) + regen `--write`;
      `suggestions.rs` chains: `find_symbol` → `find_declaration`/`find_implementations`;
      post-tools include `get_file_diagnostics`
- [ ] **Task 4.8**: redeploy daemons (`bash scripts/redeploy-daemons.sh --build --probe`) and run
      live dogfood: `find_implementations` on a repo trait (e.g. a tool handler trait),
      `find_declaration` on an engine symbol, `get_diagnostics_for_symbol` on a file with a known
      warning — record outputs in Notes

**🔵 REFACTOR**

- [ ] **Task 4.9**: extract shared "resolve symbol → LSP position" helper if duplicated across the
      two navigation handlers; keep helpers file-private (repo seam rules)

#### Quality Gate ✋

- [ ] TDD evidence; all new contract tests green
- [ ] Full matrix (same command set as Phase 2 gate) green
- [ ] regen `--check --enforce-drift` · surface-manifest `--check` · lint-datasets green
- [ ] Daemon redeploy verified: 7838/7839 LISTEN + `tools/list` probe includes the 3 tools +
      `daemon-stale-check.sh` in-sync
- [ ] Dogfood evidence recorded in Notes (commands + outputs)

---

## ⚠️ Risk Assessment

| Risk | Probability | Impact | Mitigation Strategy |
|------|-------------|--------|---------------------|
| Dispatch parser misses a registration style → false "schema_only" (the 6 semantic Arc-registered tools were a near-miss in analysis) | Med | Med | Parse both styles; pin parsed-count regression test; Phase 3 codegen makes runtime consume the same parse |
| Hand-authored schemas for the 3 workflow tools drift from handler behavior | Med | Med | Derive schemas from handler arg-extraction code; schema pre-validation contract test calls each with valid+invalid args |
| `surface-manifest.py --write` ripples into CLAUDE.md/AGENTS.md/README routing blocks unexpectedly | Med | Low | Dedicated commit for regenerated docs; manual diff review task (2.6); `--check` gates after |
| Tombstoning breaks an external direct `tools/call` user | Low | Med | They were never listable (no schema); guidance error names replacement; CHANGELOG entry |
| Mock-LSP tests flaky in CI (process spawn) | Med | Med | Reuse existing mock-LSP harness patterns (already CI-stable since P0-2); LSP-absent paths are pure-Rust deterministic |
| Phase 2 touches presets while #200 (preset codegen) is open → merge conflict with that work | Low | Low | Keep edits inside existing constants; do not start #200's codegen here; note in #200 after landing |

---

## 🔄 Rollback Strategy

Each phase = one (or two: code + regenerated docs) commit on the feature branch; rollback is
`git revert` of that phase's commits. Specifics:

- **Phase 1 fails**: revert script + description commits; regen `--write` restores
  `build_generated.rs`; no runtime behavior changed.
- **Phase 2 fails**: revert tools.toml/presets/dispatch commit + regenerated-docs commit; re-run
  `regen-tool-defs.py --write` and `surface-manifest.py --write` to confirm clean round-trip; CI
  enforce flag reverts with the ci.yml hunk.
- **Phase 3 fails**: revert; generated const is additive — reverting regenerates without it.
- **Phase 4 fails**: revert handler/tools.toml commits; **daemon**: redeploy previous binary
  (`.codelens/bin/codelens-mcp-http.bak-pre-*` retained by redeploy script) or rebuild from
  reverted HEAD; verify with `daemon-stale-check.sh`.

---

## 📊 Progress Tracking

### Completion Status

- **Phase 1 (script gate, warn)**: ⏳ 0%
- **Phase 2 (ghost resolution + enforce)**: ⏳ 0%
- **Phase 3 (runtime audit)**: ⏳ 0%
- **Phase 4 (3 LSP read tools)**: ⏳ 0%

**Overall Progress**: 0%

### Time Tracking

| Phase | Estimated | Actual | Variance |
|-------|-----------|--------|----------|
| Phase 1 | 2h | — | — |
| Phase 2 | 3–4h | — | — |
| Phase 3 | 2–3h | — | — |
| Phase 4 | 3–4h | — | — |
| **Total** | 10–13h | — | — |

---

## 📝 Notes & Learnings

### Implementation Notes

- (append during execution)

### Blockers Encountered

- (append during execution)

### Improvements for Future Plans

- (append during execution)

---

## 📚 References

- Spec: `docs/superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md` (D1/D2/D7)
- Comparison evidence: `docs/serena-comparison.md` (2026-06-10), `docs/comparison.md` Note 2
- Issues: [#346](https://github.com/mupozg823/codelens-mcp-plugin/issues/346) (resolved by Phase 2/3),
  #200 (adjacent preset codegen — keep separate), #287 (P2 LSP edit backends)
- ADRs: ADR-0010 (telemetry diet pipeline), ADR-0013 (tools.toml codegen)
- Serena patterns: `ToolRegistry._deleted_tools` (tombstone), v1.5.0 description rewrite (D7)

---

## ✅ Final Checklist

**Before marking plan as COMPLETE**:

- [ ] All 4 phases completed with quality gates passed
- [ ] `audit_tool_surface_consistency` reports zero unexplained drift on main
- [ ] CI enforce-drift step green on the PR
- [ ] Daemons redeployed and in-sync; dogfood evidence in Notes
- [ ] CHANGELOG entry (tombstoned tools + new tools)
- [ ] #346 closed with link to landing commits; #200/#287 cross-noted
- [ ] Global memory `codelens-mcp-plugin.md` updated (surface counts change: 87 → 93 listed)

---

**Plan Status**: ⏳ Awaiting start
**Next Action**: branch from main → Phase 1 RED (Test 1.1)
**Blocked By**: None
