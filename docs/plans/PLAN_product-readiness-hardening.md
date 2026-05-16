# Product Readiness Hardening Roadmap

**Status:** Active  
**Created:** 2026-05-16  
**Last Updated:** 2026-05-16  
**Owner:** maintainers  
**Scope:** public docs, release smoke coverage, tool-surface discipline, and control-plane simplification

**CRITICAL INSTRUCTIONS:** After completing each phase:

1. Check off completed task checkboxes.
2. Run the phase quality gate commands.
3. Verify all quality gate items pass.
4. Update "Last Updated".
5. Record any new evidence in Notes.
6. Only then proceed to the next phase.

Do not mark a product-readiness item complete from code shape alone. Each item needs either a passing command, a generated artifact check, or an explicit documented non-goal.

## Baseline

The 2026-05-16 audit classified the repository as **conditionally ready**:

- Core Rust gates pass locally: `cargo fmt --check`, `git diff --check`, `cargo check`, package tests, HTTP-feature tests, `cargo clippy -- -W clippy::all`, surface manifest check, and current-docs tool-surface check.
- The product is useful as a harness-native MCP code-intelligence substrate.
- General release readiness is limited by stale public docs, stale daemon/index warnings, release-install smoke gaps, and control-plane size.

## Phase 1: Public Docs And Surface Truthing

**Goal:** Current user-facing docs should advertise only supported public workflow names and should point readers to one product-readiness roadmap.

**Tasks**

- [x] Create this roadmap with small, independently verifiable phases.
- [x] Update README examples and performance table away from removed public impact-tool names.
- [x] Replace the stale architecture tool-count block with the generated manifest summary.
- [x] Update current benchmark snapshots to use the current public workflow name.
- [x] Update the Serena comparison source line so it no longer claims an obsolete tool count.
- [x] Extend current-doc checks to block removed public tool names in current docs.

**Quality Gate**

- [x] `python3 scripts/test/test-current-docs-tool-surface.py`
- [x] `python3 scripts/surface-manifest.py --check`
- [x] `cargo fmt --check`

**Rollback:** Revert this documentation-only patch and the test-script extension.

## Phase 2: Release And Install Smoke Matrix

**Goal:** Prove that the paths users actually install and launch behave like the developer workspace.

**Tasks**

- [x] Add a lightweight smoke script for the default local binary shape: `--version`, `--print-surface-manifest`, and one read-only one-shot tool call.
- [ ] Add HTTP-feature smoke: build with `--features http`, start on a random local port, call `tools/list`, then terminate cleanly.
- [ ] Add semantic-feature smoke that fails closed when the model directory is absent and succeeds when a staged model payload is present.
- [ ] Add release-archive smoke for one local dry-run archive before tag release.
- [ ] Document which smoke checks are required for release candidates vs local development.

**Quality Gate**

- [ ] `cargo check`
- [ ] `cargo test -p codelens-mcp --features http`
- [x] `scripts/smoke-release-install.sh` passes on the current debug binary
- [ ] release smoke script passes on a clean checkout

**Rollback:** Remove the smoke script and release-verification doc additions; no runtime code changes should be mixed into this phase.

## Phase 3: Tool Surface Diet And Deprecated Profile Exit

**Goal:** Reduce low-value control-plane surface without guessing.

**Tasks**

- [ ] Implement or finish the ADR-0010 underutilized-tool report against `get_tool_metrics`.
- [ ] Emit a machine-readable list of deprecated profiles and their canonical replacements.
- [ ] Add a v2.0 removal checklist for `evaluator-compact`, `refactor-full`, `ci-audit`, and `workflow-first`.
- [ ] Decide which legacy dispatch-only mutation entries remain compatibility shims and which are removed.
- [ ] Keep new features off deprecated profiles unless they are inherited through the canonical core trio.

**Quality Gate**

- [ ] `cargo test -p codelens-mcp telemetry`
- [ ] `cargo test -p codelens-mcp tool_defs::presets`
- [ ] `python3 scripts/surface-manifest.py --check`

**Rollback:** Revert profile/deprecation metadata changes independently from telemetry reporting changes.

## Phase 4: Large Module Decomposition

**Goal:** Lower review risk in the largest control-plane files without adding new layers.

**Tasks**

- [ ] Split `tools/symbols/handlers.rs` by behavior: argument parsing, ranking/merge, response shaping, follow-up suggestions.
- [ ] Split `tools/report_jobs.rs` by job family while keeping one common progress/handle helper.
- [ ] Move `tools/session/metrics_config/capabilities.rs` static capability rendering into smaller files or generated data.
- [ ] Keep public schemas stable while splitting `tool_defs/output_schemas.rs`.
- [ ] Add module-level tests before each split so behavior can be checked before moving code.

**Quality Gate**

- [ ] `cargo test -p codelens-mcp tools::symbols`
- [ ] `cargo test -p codelens-mcp tests::workflow::analysis_jobs`
- [ ] `cargo test -p codelens-mcp tests::protocol_tools_list`
- [ ] `cargo clippy -- -W clippy::all`

**Rollback:** Each file split must land as an isolated patch with no behavior change; revert one split at a time.

## Phase 5: External Proof Matrix

**Goal:** Replace broad product claims with reproducible evidence across mixed-language repositories.

**Tasks**

- [ ] Refresh the SCIP index and daemon binary before each dogfood audit run.
- [ ] Add mixed-language call-graph accuracy fixtures beyond the current package tests.
- [ ] Add semantic-refactor correctness fixtures for rename, declaration/implementation, move, inline, and change-signature where supported.
- [ ] Compare against Serena, Sourcegraph/Cody, Aider, OpenHands, and Copilot only on capabilities with cited official docs or reproducible local fixtures.
- [ ] Publish pass/fail tables rather than prose-only superiority claims.

**Quality Gate**

- [ ] external fixture matrix passes or records explicit failures
- [ ] product comparison docs link to official sources and local evidence
- [ ] release notes list any remaining non-goals

**Rollback:** Keep benchmark fixtures additive; failed experiments should be documented and disabled rather than deleted silently.

## Notes

- Do not introduce a new architecture layer to solve documentation drift. The immediate fix is stricter generated-surface checks and fewer hand-maintained public tables.
- Keep the product position narrow: CodeLens is a bounded MCP code-intelligence and mutation-governance substrate, not a full coding agent runtime.
