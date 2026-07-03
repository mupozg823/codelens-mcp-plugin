# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Readiness-aware LSP confidence calibration (P1.1b)** — `find_referencing_symbols` LSP results (both the warm default path and explicit `use_lsp=true`) now consume the P1.1a quiescence signal: a server that *itself* reports indexing-in-progress (`experimental/serverStatus` `quiescent: false`) degrades to confidence 0.7 with `degraded_reason: lsp_server_indexing_in_progress` (mirroring the SCIP-stale pattern); verified-quiescent answers keep 0.95 with an earned `lsp_precise_warm_quiescent`/`lsp_precise_quiescent` basis; servers that emit no readiness signal (e.g. pyright) keep the legacy label — no evidence either way, blanket distrust would permanently punish servers without the extension. The warm-path `routing_note` now reports `server_quiescent` (true/false/null). Closes the "warm ≠ quiescent 0.95" finding from the 5-lens architecture eval.

- **LSP client protocol parity, increment A (P1.1a)** — the response read loop now *answers* server→client requests instead of silently discarding them (the pre-existing behavior violated the LSP contract and could stall servers blocked on a reply): `workspace/configuration` → one `null` per item ("use defaults", deterministic), `client/(un)registerCapability` / `window/workDoneProgress/create` → acknowledged, `workspace/applyEdit` → **refused** (`applied: false` — read sessions never accept server-initiated edits; mutations flow through the verifier-gated transaction), unknown methods → spec-correct `MethodNotFound` (-32601). The client also advertises `experimental.serverStatusNotification` and harvests rust-analyzer's `experimental/serverStatus` quiescence signal into a per-session readiness state exposed via `LspSessionPool::warm_session_quiescence` (warm ≠ quiescent — groundwork for readiness-aware confidence calibration). Verified against a live rust-analyzer (`quiescent=true` harvested in ~2s; ignored integration test included).

- **Lean response contract for token-expensive models (Fable)** — a quality-neutral, scaffold-only envelope thrift, grounded in Anthropic tool-response guidance (keep responses lean; Claude Code counts/injects the **text** channel and warns at 10K tokens). Activation: per-call `_lean: true` (an explicit `_lean: false` overrides the env — per-call escape hatch), or daemon-wide `CODELENS_RESPONSE_CONTRACT=lean`. Drops from the text envelope: `suggestion_reasons` (prose duplication of `suggested_next_tools`), `token_estimate`/`elapsed_ms` (volatile telemetry, cache-hostile), `routing_hint` when `sync`, constant `schema_version`, under-budget `budget_hint` (kept when >75% budget / doom loop / missing preflight), and the `index_freshness` object **only in the `fresh` bucket** (degraded buckets stay attached — answer-affecting signal). Never touches `data`, `suggested_next_tools`/`_calls`, `error`, `recovery_hint`, or `structuredContent` (spec-required with `outputSchema`). Deliberately decoupled from the legacy `_compact` data pruning (adversarial review finding). Measured: 17–18% smaller text channel on `find_symbol` + body, symbol data byte-identical.

### Fixed

- **`index_freshness` staleness signal was inert (unit mismatch)** — `files.indexed_at` is written in epoch **milliseconds** but the hint compared it against `now.as_secs()`, so the age always clamped to 0 and every response reported `"fresh"` / `refresh_recommended: false`; `recent`/`possibly_stale`/`stale` could never fire. Timestamps are now normalised (ms→s, legacy second-scale values pass through). Side effect: the previously-dormant stale-index contract activates — on a >1h-old index, `refresh_symbol_index` is prepended to `suggested_next_tools` as documented.

## [1.13.33] - 2026-07-03

### Security

- **RBAC fails closed on malformed principals in mutation-capable runtimes (#355)** — a present-but-broken `principals.toml` used to fall back to the permissive default (every principal → `Refactor`), silently disarming RBAC exactly when it was misconfigured. Discovery errors now resolve to `strict_default` (every principal → `ReadOnly`, all code-mutation tools denied, logged at `error`) in any mutation-capable mode — `MutationEnabled` **and** `Standard`, the stdio/unspecified `--daemon-mode` default (adversarial review caught the Standard gap). Read-only daemons keep the legacy permissive fallback; a *missing* file still selects the env default in every mode, so installs that never deployed RBAC are unaffected. The env-strict "no principals.toml" warning is suppressed on the fail-closed path (it was factually wrong there).

- **Memory mutation now requires the `Refactor` tier (#355)** — the `write_memory`/`delete_memory`/`rename_memory` ReadOnly carve-out is removed: agent long-term memory is a trust surface whose poisoning is comparable to a source mutation, so the role gate treats it like every other content mutation (consistent with `archive_memory`/`restore_memory`, which already required `Refactor`).

### Added

- **Diagnostic-delta verification on semantic edits (#357)** — `semantic_edit` captures pre/post diagnostics across **all** edited files (including move destinations, capped by `MAX_DIAGNOSTIC_CAPTURE_FILES`; the adversarial review caught the single-file version reporting destination breakage as clean), scopes `introduced_diagnostics` to newly-introduced entries only, distinguishes `clean / introduced / preexisting / not_captured / unavailable` so an empty list is never mistaken for a clean verify, and states `edit_applied` explicitly so agents stop misreading post-edit diagnostics as a failed edit.

- **Working-set anchor ranking (#357)** — `get_ranked_context` accepts an optional `anchor_files` array (active/recently-edited working set) that feeds the existing user_context/recency RRF lane, giving Aider-style task-relative ranking without new PageRank machinery.

- **Warm-LSP precision stage on the default `find_referencing_symbols` path (#357)** — between SCIP and the tree-sitter fallback, the default path now routes through an already-warm LSP session (non-spawning probe, asserted by test — no cold start is ever incurred by routing); cold sessions fall back with a hint toward `use_lsp=true`. Closes the known Python import/type-annotation reference gap whenever pyright is warm. The `routing_note` reports honestly when a rare probe-to-call race forces a respawn.

- **Query-adaptive RRF channel weights, experiment-gated (#357)** — `CODELENS_RRF_ADAPTIVE=1` shifts fusion channel weights by query shape (identifier→sparse, NL→dense) reusing the existing intent classifier. Default **off**: measured on the canonical harness (n=96), hybrid MRR moved 0.6031 → 0.6057 (Δ+0.0026, below the +0.01 adoption threshold); flag-off invariance is pinned by test.

- **Evaluation: F-β(0.5) retrieval metrics + per-tool token profiler (#357)** — `benchmarks/retrieval-fbeta.py` adds SWE-grep-style precision-weighted F-β(0.5) at file and line granularity over the existing labeled datasets; `benchmarks/tool-token-profile.py` probes a live daemon and compares actual response tokens against the manifest's `estimated_tokens` (first run flagged `get_symbols_overview` at 2.23× and `bm25_symbol_search` at 2.77× over estimate).

### Changed

- **`orchestrate_change` is described as what it is (#355)** — an advisory dry-run orchestration plan: session-scoped in-memory approval, enforced opt-in only when a mutation call passes `orchestration_run_id`. Tool description and `docs/multi-agent-integration.md` now separate shipped behavior from the ADR-0014 durable-state direction. pending-D3 is recorded as **decided (2026-07-03): stays dispatch-only** — identifiers and drift-gate vocabulary unchanged. Memory tools expose their real `scope` parameter (`project`/`global`; `read` also `auto`, `list` also `both`) in tools.toml, and the memory-scopes resource note matches the runtime tiers.

### Documented

- **Landscape deep-research report (#357)** — `docs/research/landscape-deep-research-2026-07-03.md`: 8-axis adversarially-verified survey (Serena, LSP bridges, SCIP ecosystem, semantic-indexing MCPs, Aider repo-map, editor indexing, benchmark methodology, mutation-safety patterns) with 12 repo-grounded improvement candidates and an "already implemented — do not re-propose" corrections list. Also fixed the stale CLAUDE.md claim that the daemon "does not auto-watch" (the `FileWatcher` is default-on).

### Fixed

- **NFC follow-ups: query lanes, call graph, and codex workspace binding (#353)** — completes the grill-me triage of the #349/#351 residuals. (1) *Query lanes*: `analyze_retrieval_query` (feeds BM25F sparse + ranked-context fusion) and `search_symbols_fuzzy` normalize NFD-pasted Hangul queries to NFC, matching the NFC corpus — the NFC end-to-end invariant now holds on the query side of every lane. (2) *Call graph*: `insert_calls` and `get_callers_cached` / `get_callees_cached` join the same 3-boundary NFC contract as the symbols table. (3) *Codex binding*: `codelens-mcp attach codex` stamps `http_headers = { "x-codelens-project" = … }` into the `config.toml` template — officially supported by codex for streamable-HTTP MCP servers — so codex sessions bind at initialize and survive the 30-min eviction exactly like JSON-template hosts (#347/#351). Deliberately deferred, recorded on #349: NFD filename path emission (freshness/watcher path-key risk) and pre-existing NFD index rows (refresh re-normalizes).

- **NFC-canonical symbol names — Hangul NFD identifiers no longer silently miss (#349)** — identifiers written in decomposed jamo (NFD, e.g. pasted from macOS filenames) were stored byte-exact, so the NFC query an agent types returned 0 hits with no signal. Symbol names now normalize to NFC at the extraction choke point (`parse_symbols`), at the DB write boundary (`insert_symbols`), and on the query side (`find_symbols_by_name` / `find_symbols_with_path`) — NFC and NFD queries both resolve, and the index/overview/BM25F corpus all carry one canonical form. Signatures and bodies stay byte-faithful to the source. Pre-existing index rows keep their old form until the next `refresh_symbol_index`. Found by the 2026-06-10 Korean-code dogfood probe; the same probe verified parsing, body reads, references, fuzzy, and ko→ko semantic search already handle Hangul correctly.

- **Session evict/resurrect no longer drops the explicit workspace binding (#351)** — the 30-min idle sweep plus lenient resurrection re-seeded sessions with default metadata, so a header-bound session could silently flip to the daemon's global scope mid-conversation (observed live: `index_embeddings` ran against the wrong repo). Three layers: `SessionSeed` now carries `x-codelens-project` so resurrection re-binds explicitly; the binding is re-captured from the header on **every** request (not just initialize) — header-attached hosts are now immune to eviction entirely and can switch workspaces mid-session; and `session_project_binding_explicit` treats a store miss as not-explicit, so an evicted-mid-request session surfaces the `project_binding` hint instead of suppressing it.

- **`bm25_symbol_search` joins the planner-readonly / builder-minimal / reviewer-graph surfaces (#350)** — fallback hints reachable from those surfaces (find_symbol miss hint, the D1 LSP read trio's `fallback_hint`) referenced the tool, but following the hint returned `not available in active surface` — the recovery chain dead-ended. A new preset regression test pins the contract: every surface exposing a hint emitter must expose the hint targets. (The originally-reported "sparse lane needs index_embeddings" premise was a misdiagnosis — the 0-result probes were #351's lost binding reading the wrong project; correction recorded on the issue.)

### Added

- **Session project binding — shared daemons stop defaulting to the wrong workspace (#347)** — four-layer fix for the recurring "calls from another project hit the CodeLens dev folder and the new project never gets indexed" failure. (1) *Initialize-time capture*: `initialize` now binds the session to `params.project` or the `x-codelens-project` HTTP header; the previous behaviour silently seeded every session with the daemon's default project. Binding provenance is tracked (`project_path_explicit`), and switching projects auto-builds the symbol index on first use plus starts the file watcher (existing `ensure_session_project` → `build_project_runtime_context` path — it only ever needed a correct binding to fire). (2) *Serena-style onboarding instructions*: the `initialize` response now leads with "FIRST CALL of every session: `prepare_harness_session` with `project=<absolute workspace root>`" so agents that ignore headers still self-serve the binding. (3) *Attach stamping*: `codelens-mcp attach <host>` JSON templates (`.mcp.json`, `.cursor/mcp.json`, …) now embed `headers.x-codelens-project=<workspace root>`, making the binding zero-touch for every session the host opens; `doctor`/`status` compare against the stamped canonical form. (4) *Unbound-session hint*: any tool response on a session that never declared its workspace carries a `project_binding` object (`bound:false`, active project, recovery hint) instead of silently answering from the wrong index.

- **D1 LSP read trio: `find_declaration` / `find_implementations` / `get_diagnostics_for_symbol` (#346 Phase 4)** — Serena-parity read navigation surfaced on planner-readonly / builder-minimal / reviewer-graph. The navigation pair resolves the position from the symbol index by name (no manual line/column needed) and rides the existing `resolve_symbol_target` engine path; `get_diagnostics_for_symbol` filters the `get_file_diagnostics` pipeline (SCIP→LSP, classification/suppression intact) to one symbol's span (definition line through body end). All three honor the read-surface degradation contract: when no LSP server is reachable they return a successful empty result with `degraded_reason` + `fallback_hint` (`find_symbol`, `bm25_symbol_search`) instead of erroring. Live dogfood on the redeployed daemons: `find_implementations(PreciseBackend)` → `scip_backend/navigation.rs:16`, warm `find_declaration(make_symbol_id)` → `symbols/types.rs:198`, `get_diagnostics_for_symbol(resolve_symbol_position)` → span 14..24, 0 diagnostics on a clean file.

### Changed

- **Ghost-tool resolution (#346 Phases 1–3)** — the 20 dispatch-only ghosts split: 8 line-edit tools (`create_text_file`, `delete_lines`, `insert_at_line`, `replace_lines`, `replace_content`, `insert_content`, `replace`, `add_import`) are tombstoned with replacement guidance pointing at host-native Edit/Write; 9 symbolic-edit/refactor tools stay dispatch-only on a pending-D3 allowlist; `onboard_project` / `analyze_change_request` / `orchestrate_change` are promoted to first-class tools.toml entries and now actually list (they had been silently filtered by the v1.13.27 deprecation list). `regen-tool-defs.py --enforce-drift` gates CI on unexplained dispatch/schema/preset drift and tombstone re-introduction; `audit_tool_surface_consistency` reports the same vocabulary at runtime via a new `surface_drift` section plus a `tombstone_reintroduced` violation bucket.

### Documented

- **Self-auditability cycle dogfood — first real-call audit of the resurrected detectors (2026-05-21)** — after daemon redeploy of `b07d577` activated the new schemas, calling each detector against this repo surfaced concrete findings instead of empty payloads. Notes recorded so the next operator has the audit-trail without re-running the daemon:
  - `find_redundant_definitions` returned **7 wrapper candidates**: `build_host_adapters → build_host_adapters_for_project(None)` (surface_manifest/host_adapters.rs:10), `chunk_from_row → chunk_from_row_at(0)` (vec_store.rs:146), `duplicate_pair → duplicate_pair_with_symbols("b")` (workflows.rs:553), and 4 more. Cleanup candidacy depends on whether each wrapper is an intended convenience alias or accidental drift — not auto-fixed in this commit; reserved for a focused cleanup sprint that can examine call-sites.
  - `find_misplaced_code` returned **10 outliers**, dominated by known-pattern false positives (top three: `orphan_handlers` module declaration in main.rs:18, `call_graph` in engine/lib.rs:31, `status_label` in codelens-tui/app.rs:81). The semantic outlier signal is real but role-aware weighting (entry-point file boost) is the documented improvement axis — see `dogfood/results_2026_05_18` Serena memory G5.
  - `find_code_duplicates` returned **10 near-duplicate pairs**, dominated by `.github/workflows/*.yml` metadata fields (name/permissions/pull_requests blocks at similarity 0.94+). Confirms the 2026-05-18 finding G6: threshold 0.85 is too generous for YAML metadata; filetype-aware threshold or directory-exclude is the documented improvement axis.
  - `audit_memory_consistency` returned `total=9, stable_skipped=4, stale=0` — confirms the `<!-- audit-skip: stable -->` marker (0159daf6) correctly excluded the 4 historical entries in `.codelens/memories/` (2 ADRs + 1 benchmark snapshot + 1 v1.9.47-v1.9.49 security audit).
  - `find_over_visible_apis` returned **1 violation** → fixed in this commit batch (00a6289a). This is the first defect surfaced by the Self-auditability cycle that triggered an actual patch in-session, closing the loop end-to-end.

### Fixed

- **`refresh_symbol_index` removed from `MINIMAL_TOOLS` preset (dogfood patch on b07d5773 surface)** — daemon redeploy of the b07d5773 binary surfaced a real `find_over_visible_apis` violation: `refresh_symbol_index` has `annotations = "mut_w"` (`audit_category="mutation"`, `approval_required=true`) but was listed in `MINIMAL_TOOLS`, which promises read-only safety. The Minimal preset is the surface a planner uses when it should not be able to trigger mutations, so the leak meant a `Minimal`-bound caller could initiate an index rebuild without surfacing the approval-required intent. Patch drops `refresh_symbol_index` from both `presets.rs::MINIMAL_TOOLS` (line 167) and the matching `preset_tags = ["minimal", ...]` entry in `tools.toml`. The tool remains in `BUILDER_MINIMAL_TOOLS` and the broader `Full` preset where mutation surface is expected. Re-running `find_over_visible_apis` against the patched binary now reports `violation_count = 0`. This is the first real defect uncovered by the Self-auditability detector family — proof the cycle is doing the job it was built for.

### Added

- **Resurrect 3 sibling schemas: `find_similar_code` / `find_code_duplicates` / `classify_symbol` (completes ghost-entry recovery)** — same pattern as the c7843092 `find_misplaced_code` restoration: handlers have lived in `dispatch/table.rs:379-456` since v1.13.6 (feature-gated on `semantic`), Sprint B-3 (6726e663) only dropped the tools.toml schemas. Restoration is schema-only — no engine wiring, no `cfg` changes. Input schemas follow the handler signatures: `find_similar_code` requires `file_path` + `symbol_name` (anchor target) with optional `max_results` + `min_similarity`; `find_code_duplicates` takes optional `threshold` + `max_pairs`; `classify_symbol` requires `file_path` + `symbol_name` + `categories[]`. All three use the admin-only surfacing (`preset_tags=[]`, `annotations="ro_a"`) established in Sprint 2. 1 new regression guard (`resurrected_semantic_detectors_all_registered_in_toml`, semantic-feature-gated) pins all four siblings (find_misplaced_code + find_similar_code + find_code_duplicates + classify_symbol) against future Sprint B-style trims via `audit_tool_surface_consistency`. Closes the ghost-entry recovery — `find_orphan_handlers` remains semantically subsumed by `audit_tool_surface_consistency.missing_in_toml`.

- **Resurrect `find_misplaced_code` schema (corrects 99f42451 ghost-entry policy)** — agent-loop dogfood of 99f42451 revealed the policy commit was inaccurate: `find_misplaced_code_handler` (and its 3 siblings) have lived in `crates/codelens-mcp/src/dispatch/table.rs:459-475` since v1.13.6, feature-gated on `semantic`. The Sprint B-3 cleanup (6726e663) dropped only the tools.toml schema; the dispatch handler was preserved the whole time. So restoration is **schema-only** — no engine wiring, no `cfg` plumbing, no new wrapper code needed. The tools.toml entry uses the same admin-only surfacing (`preset_tags=[]`, `annotations="ro_a"`) as the four sibling detectors resurrected in Sprint 2. 1 new regression test (`find_misplaced_code_registered_in_toml`, semantic-feature-gated) pins the schema against future Sprint B-style cleanups by asserting the audit no longer flags it as `missing_in_toml`. The remaining three (`find_similar_code`, `find_code_duplicates`, `classify_symbol`) follow the identical pattern — handlers live, schemas missing — and can be restored in a follow-up commit. Closes the `<!-- audit-skip: stable -->` self-dogfood cycle that started in 0159daf6.

### Documented

- **Ghost-entry policy: `find_misplaced_code` / `find_similar_code` / `find_code_duplicates` / `classify_symbol` stay dropped until a PRD lands** — these four tools were removed from `tools.toml` in Sprint B-3 (6726e663) as schema-only ghosts (visible in `tools/list` but `404 Unknown tool` on `tools/call`). Engine implementations for three of them remain in `crates/codelens-engine/src/embedding/engine_impl.rs::{find_similar_code, classify_symbol, find_misplaced_code}` (find_code_duplicates was never implemented at the engine level). Restoring the MCP surface requires more than glue: (a) a spec defining the use case for each (DRY auditing vs outlier detection vs ad-hoc classification), (b) feature-gating behind `cfg(feature = "semantic")` since all three depend on the embedding engine, (c) a regression test that pins false-positive behaviour against the patterns the original `audit_tool_surface_consistency` flagged. Until a PRD covers those three, the cleanest stance is the current one — `audit_tool_surface_consistency` no longer reports them as drift, and `find_misplaced_code` / `find_redundant_definitions` / `find_phantom_modules` / `find_over_visible_apis` already cover the cleanup auditing that B-3 was bundled with. Logged here so the next observer of the engine-impl ↔ MCP-surface gap can confirm the absence is intentional, not stale.

### Fixed

- **`get_analysis_section`: any-scope fallback for chained handle resolution (#G8 root cause)** — the handler resolved the project scope via `state.project_scope_for_arguments(arguments)` which falls through to `current_project_scope()` when the caller doesn't restate `path`. If `review_architecture` (or sibling report tools) had been called with an explicit `path` argument, the artifact was stored under that path's scope; the chained `get_analysis_section` then computed a different scope from the active project and the strict `matches_scope` filter returned `None` — surfacing `"Not found: unknown analysis_id"` even milliseconds after the handle was returned (observed in 2026-05-21 self-dogfood, ~256 ms after the cached `reused: true` response). Patch adds `AppState::get_analysis_any_scope` (artifact_store.get with `None` scope) and uses it as a fallback when the strict lookup misses. Response gains `scope_widened: true` + `stored_scope` + `requested_scope` fields when the fallback path is taken, so callers can spot the mismatch without losing the section content. `analysis_id` is monotonic per daemon process (`analysis-{ms}-{seq}`), so cross-scope id collisions are vanishingly rare; scope-isolated lookups remain available through the strict path for callers that need them. 1 new regression test (`any_scope_lookup_returns_artifact_across_scopes`) pins the contract at the artifact_store layer. The earlier Sprint 1 env-var escape hatch (`CODELENS_MAX_ANALYSIS_ARTIFACTS`/`CODELENS_ANALYSIS_TTL_HOURS`) is unchanged — that addresses cap exhaustion, this addresses lookup geometry.

### Added

- **`audit_memory_consistency`: opt-out marker `<!-- audit-skip: stable -->` for point-in-time memories** — self-dogfooding the eec5e032 detector against `.codelens/memories/` surfaced four entries that the staleness check correctly flagged but that are inherently frozen-in-time (2 ADRs, a 2026-04-19 benchmark snapshot, a v1.9.47-v1.9.49 security audit). These are pinned-to-a-moment artifacts, not drift-prone working notes. Added a stable-skip marker the audit recognises in the first 4 lines of any memory file: `<!-- audit-skip: stable -->`. The scan window is intentionally tiny so a typo deeper in the file can't accidentally suppress the audit. Response gains `stable_skipped` counter so dashboards distinguish "opt-out coverage" from "still-fresh entries". The four historical files in `.codelens/memories/` are marked. 2 new regression tests pin (a) the field's always-present presence and (b) the bounded scan window (line 5+ marker must not match).

- **`audit_memory_consistency`: surface stale project memory files (Sprint 2 step 3 — self-auditability cycle extension)** — `.codelens/memories/*.md` files are frozen-in-time observations that silently drift from the codebase they describe (cited paths get renamed, cited symbols disappear, cited architectural claims stop matching). This is the file-system complement to the four tool-surface detectors completed in Sprint 2: same admin-only surfacing (`preset_tags=[]`, `annotations="ro_a"`), same runtime-query pattern (no engine impl needed). Threshold is configurable via `threshold_days` argument (default 30, clamped 1..=3650). Each stale entry reports `{file, age_days, mtime_epoch_secs}` so callers can fold output into a freshness ratchet. Entries are sorted oldest-first. Resolves the "메모리 stale 클래스 자동 검출" follow-up from the 2026-05-21 session memo. 3 new regression tests pin (a) dispatch+toml co-registration via the audit, (b) the response envelope shape, and (c) threshold clamping (0 → 1, 10000 → 3650).

- **`semantic_edit_backend = "auto"`: LSP-first routing for refactor tools (Sprint 3 step 1 — Serena edit-substrate gap)** — `refactor_{extract,inline,move_to_file,change_signature}` are dual-backend (LSP `textDocument/codeAction` vs tree-sitter syntactic fallback). Before this change the default was `"tree-sitter"`, requiring callers to opt into LSP per call. New `"auto"` value picks LSP when `default_lsp_command_for_path(file_path)` returns Some (rust/python/ts/js/go/java/kotlin, etc.), else falls back to the syntactic path. Activate per call with `semantic_edit_backend="auto"` or session-wide with `CODELENS_SEMANTIC_EDIT_BACKEND=auto`. Closest CodeLens equivalent of Serena's always-on LSP routing; the default `"tree-sitter"` remains unchanged to preserve backward compatibility for existing callers. Capability detection never errors — if `file_path` is missing, falls back silently to tree-sitter. 7 new unit tests pin the resolution table (default/aliases/lsp/auto+capable/auto+uncapable/auto+nofile/unsupported-error-message). Documented in CLAUDE.md "Semantic Edit Backend" section. The remaining `conditional_authoritative_apply` gate (fixture-green inspectable `WorkspaceEdit` coverage per language) is unchanged — that's a separate sprint that promotes the 4 tools to `authoritative_apply`.

- **`scripts/cleanup-stale-backups.sh`: rotate three orphaned backup classes** — codifies the friction discovered during the 2026-05-21 self-dogfood: ~2.4 GB of `.bak-*` files had accumulated across two locations without any retention policy, because every backup is created at a discrete decision point (daemon upgrade, db schema migration, readonly conversion) but never retired. Script keeps the N most recent backups per pattern (default `--keep 2`, configurable) and deletes the rest. Three patterns managed: `${REPO}/.codelens/bin/codelens-mcp-http.bak-pre-*` (daemon redeploy), `~/.codelens/index/{symbols,embeddings}.db.bak-*-migration` (in-place schema migration), and `~/.codelens/index/{symbols,embeddings}.db.bak-readonly-old` (rw→ro conversion). `--dry-run` previews without deleting; `--repo-root PATH` retargets when invoked outside the workspace. Manual invocation only — no daemon code touches the on-disk rotation, so a future operator can audit deletions without runtime side-effects.

- **`artifact_store`: runtime override for cache caps via env vars** — `CODELENS_MAX_ANALYSIS_ARTIFACTS` (non-zero usize, default 50) and `CODELENS_ANALYSIS_TTL_HOURS` (non-zero u64, default 6) now adjust the analysis artifact LRU count cap and TTL without rebuilding. Discovered via self-dogfood (2026-05-21): `get_analysis_section` follow-ups can hit `Not found: unknown analysis_id` when long-running planner sessions exceed 50 chained analyses, and there was no operator escape hatch short of patching the const. Invalid or zero values fall back to the compiled defaults. Documented in CLAUDE.md "Analysis Artifact Cache" section. Underlying `reused: true` vs stored-id consistency remains a separate sprint (#G8 root cause).

- **Resurrect `find_over_visible_apis` (Sprint 2 Step 2 — self-auditability)** — completes the v1.13.27 detector trim recovery. Surfaces tools whose `ToolAnnotations` contradict the readonly-intent of the preset/profile they're listed in: a `destructive_hint=true` or `approval_required=true` tool exposed on `preset:Minimal`, `profile:PlannerReadonly`, or `profile:ReviewerGraph` is leakage — the surface promises read-only safety, but the tool reserves write/approval semantics. The 2026-05-18 dogfood memo referenced "495 over-visible cleanup" as the unfinished tail of this audit. Runtime query only: walks `visible_tools(ToolSurface::*)` for the three readonly-intent surfaces, inspects `Tool.annotations.destructive_hint` and `approval_required`, and emits `{tool, surface, reasons[]}` triples. No engine impl needed — data lives entirely in the Tool registry compiled from tools.toml. Response includes `policy` keys documenting the rule and `readonly_surfaces_checked` for the audit envelope. Same admin-only surfacing as the sibling detectors (`preset_tags=[]`, `annotations="ro_a"`). 2 new regression tests pin (a) dispatch+toml co-registration via the existing audit and (b) the response shape contract. Resolves the Serena vs CodeLens v2 "Self-auditability" gap for the last remaining detector in the original five; `find_orphan_handlers` is semantically subsumed by `audit_tool_surface_consistency.missing_in_toml`.

- **Resurrect `find_phantom_modules` + `find_redundant_definitions` (Sprint 2 Step 1 — self-auditability)** — both detectors were dropped from the dispatch table during the v1.13.27 surface trim alongside `audit_tool_surface_consistency`, leaving their `codelens_engine::{phantom_modules,redundant_definitions}` impls orphaned (callable only through internal Rust API, invisible to MCP). This patch restores the missing MCP wrappers in `tools/admin.rs`: each takes `max_results` (1..500, default 50), calls the engine fn against the active project, and returns `{count, max_results, truncated, next_actions, ...entries}`. Both `tools.toml` entries use `preset_tags = []` + `annotations = "ro_a"` to match the admin-only surfacing of `audit_tool_surface_consistency` — invocable via `tools/call` but not listed in any preset. Regression guard test calls the audit and asserts neither tool appears in `missing_in_dispatch` or `missing_in_toml`, locking the registration to dispatch+toml co-evolution. Resolves the Serena vs CodeLens v2 "Self-auditability" gap for these two of the original five detectors; `find_orphan_handlers` and `find_over_visible_apis` (no engine impl yet) and the v1.13.32 ghost-entries `find_misplaced_code`/`find_similar_code`/`find_code_duplicates` (engine impls exist, MCP wrappers dropped in Sprint B-3) remain follow-ups.

## [1.13.32] - 2026-05-19

### Fixed

- **`tools.toml`: drop 4 schema-only ghost entries (Sprint B-3)** — `find_similar_code`, `find_code_duplicates`, `classify_symbol`, `find_misplaced_code` had schemas in `tools.toml` but no dispatch handlers; calling them returned `Unknown tool` despite being visible in `tools/list`. All four had `preset_tags = []` confirming they weren't surfaced anywhere, so removing the schemas closes a tools/list contract violation without changing any callable surface. Engine implementations remain in `crates/codelens-engine/src/embedding/engine_impl.rs` for future wrapper restoration. 2 active ghosts kept (`semantic_search` + `index_embeddings`) — they have `preset_tags` set and 9 references across `suggestions.rs` / `principals.rs`; handler revival is a follow-up sprint.
- **`audit_tool_surface_consistency`: split intentional deprecations out of violation buckets (Sprint B-2)** — the audit was reporting 27 false positives from the `tool_deprecation()` allowlist (v1.13.27 deprecation cycle). Now partitions `missing_in_toml` and `orphan_in_preset` through the deprecation allowlist: violations bucket contains only real issues, `intentional_deprecation` bucket surfaces the 27 grandfathered tools separately for visibility. `missing_in_dispatch` is intentionally NOT filtered — schema visibility implies callable, deprecation is irrelevant for that direction.
- **`cliff.toml`: empty header to stop CHANGELOG re-injection (#339)** — release-plz PR #339 (chore: release v1.13.30) shipped a broken CHANGELOG diff: the markdown title block was injected under `[Unreleased]` on every release PR because git-cliff emitted the `[changelog] header = "..."` template each invocation, and release-plz unconditionally prepended the output. Set `header = ""`; the title + blurb live permanently at the top of CHANGELOG.md. Verified with git-cliff 2.13.1 dry-run.

### Refactored

- **`scip_backend.rs` → 5-file directory module (P2-2)** — 974-line monolith split into `mod.rs` (96 LOC: struct + load/detect/counts), `parse.rs` (78 LOC: 5 stateless helpers), `call_graph.rs` (210 LOC: find_callees/find_callers), `navigation.rs` (259 LOC: PreciseBackend impl + resolve_scip_symbols), `tests.rs` (377 LOC: 12 unit tests). Public API unchanged — sub-modules use multi-impl-block pattern so `backend.find_callees(..)` etc. callsites are byte-identical. `pub(super)` on `documents` / `symbol_info` fields scopes field access to the module tree. Verified: engine 412 / 412 + mcp 599 / 599 pass, clippy / fmt clean.

### Chore

- **rustfmt normalization across recent commits** — 6 files where local rustfmt collapsed multi-line `let` / `use` blocks to single lines that CI's `cargo fmt --all -- --check` would otherwise reject. Pure whitespace, no semantic change.

### Added

- **scripts/redeploy-daemons.sh**: post-build daemon redeploy automation. Encodes the friction discovered during the 2026-05-18 self-dogfood session: every `cargo build` → `cp` → `launchctl kickstart` cycle was hitting `OS_REASON_CODESIGNING SIGKILL` because cargo-produced binaries carry a `com.apple.provenance` xattr that macOS gatekeeper rejects on launchd-spawned processes. The script handles `cp + xattr strip + ad-hoc codesign + kickstart + LISTEN wait + (optional) tools/list health probe`. Has `--build` (run cargo build first), `--skip-{readonly,mutation}`, `--probe`, and `--wait-secs` flags. CLAUDE.md HTTP Daemon Operations section now points at this script as the preferred restart path; the manual sequence remains for fallback. Closes P0-3 of the v2 improvement roadmap.

### Fixed

- **artifact_store: cross-tool cache isolation (#G2)**: `find_reusable_tiered`'s L3 cold-tier matched on scope + generic `cache_key` alone, allowing a stored `dead_code_report` artifact to be returned verbatim for an unrelated `module_boundary_report` call when the latter missed L1/L2. The two tools produce structurally different payloads (summary, findings, section layout), so the fallback was payload-poisoning. All three tiers now require `tool_name` to match; L3 still relaxes the surface constraint (planner-readonly can reuse a refactor-full artifact from the same scope). Replaces the cross-tool hit test with an isolation regression test and adds an L3 same-tool different-surface hit test. Discovered via self-dogfood (2026-05-18).
- **dead_code_report: accept `path` as soft alias of `scope` (#G1)**: `dead_code_report` was the only composite report tool that argued over `scope`; the rest of the family (`impact_report`, `module_boundary_report`, `refactor_safety_report`, `diff_aware_references`, …) takes `path`. A caller copy-pasting the sibling-tool convention sent `{"path":"crates/..."}` and the handler silently fell back to `scope = "."`, scanning the project root and surfacing false positives like `.cargo/audit.toml`. Accept `path` as a soft alias in both the sync and async-job handlers, include `path` in the cache key, and add `path` to the tools.toml input schema with a description marking it as the alias. Bonus: narrower scope drops the `.cargo/audit.toml` false positives from `top_findings` naturally.

### Refactored

- **build_info: isolate drift evidence from payload shaping**: `daemon_binary_drift_payload` no longer mixes evidence-gathering with JSON shaping. The pure decision now lives in `build_drift_payload(&DriftEvidence, &str) -> Value`; `DriftEvidence` carries the four fields the classifier needs (`mtime_stale`, `executable_path`, `modified_seconds`, `head_git_sha`). The entry function still owns I/O (env var + `fs::metadata` + `current_head_git_sha`) and its four `status: "unknown"` early returns, so the public contract is byte-identical. Adds 3 pure unit tests next to the existing `classify_drift` suite — the staleness response envelope no longer depends on fixture-level env/fs/filetime hacks. Follow-up to #335 (`build_info::current_executable_path()` env+fs isolation note).

### Documentation

- **docs/comparison.md: mark detector-family tools as removed from MCP surface**: the five tools that the comparison table cited as CodeLens-exclusive — `find_over_visible_apis`, `find_phantom_modules`, `find_orphan_handlers`, `find_redundant_definitions`, `audit_tool_surface_consistency` — were silently dropped from the dispatch table during the v1.13.27 surface trim. `audit_tool_surface_consistency` still answers via the daemon path with `_meta.codelens/deprecatedSince=1.13.27` but the CLI oneshot path returns `Unknown tool`, and the other four are unreachable from either path. Library modules (`phantom_modules.rs`, `redundant_definitions.rs`) remain in `crates/codelens-engine/src/` but are not surfaced. Strike-throughs added to the comparison matrix with a footnote pointing at the self-auditability roadmap item.

## [1.13.29] - 2026-05-18

### Fixed

- **macOS test flake root cause (#332)**: 6 PRs (PR-F..PR-J) had been failing the same macOS-only check (`codelens-engine search::tests::semantic_low_scores_filtered_out`) since 2026-04. systematic-debugging tracked it to a path-collision bug in the shared search-test fixture: `subsec_nanos()` quantises to ~1 µs on macOS vs 1 ns on Ubuntu, so the 9 tests sharing `make_project_with_symbols` regularly hit the same temp-dir path. Two tests racing into the same SQLite file then hit `journal_mode = WAL`'s schema-level lock before the batch's own `busy_timeout = 5000` PRAGMA was applied (it was the 6th PRAGMA). Fixed by switching the fixture to `tempfile::TempDir`.
- **db (defense in depth) (#333)**: reorder `IndexDb::open` and `SqliteVecStore::new` PRAGMA batches so `busy_timeout = 5000` precedes `journal_mode = WAL`. Any production caller that ever opens the same DB from two threads now waits up to 5 s instead of erroring out instantly.

### Added

- **transparency: index_freshness signal (PR-J, #329)**: `find_referencing_symbols`, `find_symbol`, `get_ranked_context`, `get_symbols_overview` (and `onboard_project` via PR-L) now attach an `index_freshness` object with `newest_indexed_at_epoch_secs`, `newest_indexed_age_secs`, a 4-bucket `staleness_hint` (`fresh`/`recent`/`possibly_stale`/`stale`), and `refresh_recommended`. Backed by new `IndexDb::max_files_indexed_at()` / `SymbolIndex::max_indexed_at()` queries.
- **auto-suggest refresh on stale index (PR-L, #331)**: when `refresh_recommended: true`, `refresh_symbol_index` is prepended to `suggested_next_tools` so an agent can recover without knowing the call name. Idempotent. Sits after doom-loop override (heavy retry guidance still wins) but before the calls-builder.
- **onboard_project carries `index_freshness` (PR-L, #331)**: the first call into a new MCP session reports its own index staleness up front.

### Changed

- **deps: workspace pin for `codelens-engine` (PR-K, #328)**: moved the `version = "=…"` pin into `[workspace.dependencies]`. Future releases bump a single version string at the workspace root; member crates use `codelens-engine = { workspace = true }`. release-plz can now propagate version bumps without a member-crate hand-edit.
- **docs: Index Freshness Signal section in CLAUDE.md (#334)**: documents the four-bucket staleness contract and the human-driven `refresh_symbol_index` workflow for large multi-file renames.

### Infrastructure

- **release-plz workflow permission**: GitHub Actions can now create release PRs (`default_workflow_permissions=write` + `can_approve_pull_request_reviews=true`). Closes the recurring 403 in the release-plz workflow that had been blocking automation since the PR-F..PR-I sequence.

## [1.13.28] - 2026-05-18

### Refactored

- **arch (PR-A through PR-H)**: deep-module deepening of the symbol-query path.
  - PR-A/B/C/D/E (#317, #319, #320, #321, #322 → bulk #323): `SymbolQueryPipeline` introduced at `crates/codelens-mcp/src/tools/symbol_query/`. The three symbol-shape tools (`get_ranked_context`, `find_symbol`, `get_symbols_overview`) now dispatch through a single seam; each `pub fn` in `tools/symbols/handlers.rs` is a 3-line `SymbolQueryRequest::* → SymbolQueryPipeline::run` stub.
  - **PR-F (#324)**: cycle `mod.rs → ranked_context.rs → handlers.rs` removed by extracting `sparse_retriever.rs`. Dependency flow is now one-way: `symbols::*` → `symbol_query::*`. `review_architecture` live verify: `cycle hits = 0`.
  - **PR-G (#325)**: `tools/symbols/handlers.rs` split by responsibility into `bm25_search.rs` + `fuzzy_search.rs` + `inventory.rs`. `handlers.rs` collapsed from 448 → **31 LOC** (-93%).
  - **PR-H (#326)**: stage-4 rank-fusion extracted to `symbol_query/rank_fusion.rs` — 5 helpers + `RankFusionPolicy` are `pub(super)`, `ranked_context.rs` is the only legitimate caller. `ranked_context.rs` 958 → 665 LOC (-31%).

### Fixed

- **transport (#318 → bulk #323)**: `unknown_session` envelope unified across POST and GET-SSE paths via the new `unknown_session_response()` funnel. Both transports now return JSON + `x-codelens-session-rotate: 1` header.
- **#179 (#314)**: SCIP `end_line` propagation for precise body slicing in `find_symbol`.
- **#268 (#315)**: tree-sitter structural orphan files downgraded from `unused` to `low_confidence` in the dead-code report.
- **#299 (#316)**: `cleanup_duplicate_logic` guards signature-only false positives with body-token Jaccard distance.

### Docs

- **PR-I (#327)**: refresh `CLAUDE.md` "Symbol-query path lives behind one seam" with the post-deepening module tree (11 files across `tools/symbol_query/` + `tools/symbols/`). Update `docs/architecture.md` tool-handler diagram to reflect the deep pipeline + per-tool split + `semantic_retriever` cross-cutting seam.

## [1.9.59] - 2026-04-30

### Added
- **Benchmarks**: `search_paths` benchmark for exact/FTS5/fuzzy/no-match search paths.
- **Benchmarks**: Cache hit/miss benchmark for `ranked_context_cached` (75µs hit vs 413µs miss).
- **Benchmarks**: Large-project indexing benchmark (100 modules, 500+ symbols, ~7.3ms).
- **Tests**: 27 unit tests across `eval_reports`, `report_jobs`, and `semantic_edit_args`.
- **CI**: `semantic-off` build verification (`--no-default-features --features audit`) on every PR.
- **CI**: Slim binary artifact upload (`codelens-mcp-slim`, 58MB) alongside default build.

### Changed
- **Binary Size**: `semantic` feature-gate reduces binary size by 22.7% (75MB → 58MB) when disabled.
- **Architecture**: `SemanticMatch` and related data types moved to unconditional `embedding_types.rs` for graceful degradation.
- **Coverage**: `workflows.rs` line coverage improved from 34% to 65.68% via integration tests.
- **Coverage**: Overall line coverage improved from 82.31% to 82.90%.

### Fixed
- `semantic-off` builds now compile and pass 494 tests without feature-gate regressions.
- `cargo clippy --workspace -D warnings` remains at zero warnings.

## [1.9.58] - 2026-04-28

### Added
- **SCIP Backend**: Initial SCIP index integration with `get_callers` and `get_callees` support.
- **SCIP**: Startup probe with `scip_status` and setup hint surfacing.
- **SCIP**: Stale index detection against `Cargo.lock`/`Cargo.toml` mtime.
- **Call Graph**: Rust macro invocation edges (C-1).
- **Call Graph**: Java constructors and method references (C-2 + C-3).
- **Call Graph**: Python decorators and JSX/TSX component edges.
- **Dispatch**: `limit`/`top_k` argument aliases with unknown-arg surfacing.
- **Dispatch**: Grep-fallback recovery hint when call graph is unresolved.
- **Dispatch**: Compression truncation surfaced at top level.

### Changed
- **Refactor**: Tree-sitter heuristic honesty pass on 4 refactor tools.
- **Tools**: Deprecated v2.0 aliases removed from 5 tools.
- **Audit**: Single audit sink with retention sweep and per-project principals cache.
- **Capabilities**: `model_status` and honest model-sidecar messaging.

### Fixed
- Refactor handlers retain tree-sitter honesty surfaces (CI lint gate).
- `file_path` ↔ `path` bidirectional alias support.

## [1.9.57] - 2026-04-25

### Added
- **Mutation Primitives**: Atomic 2-file substrate for `move_symbol`.
- **Audit**: `audit_log_query` tool and lifecycle state machine.
- **Cache**: Cache invalidation contract with `evidence_hash`.
- **Coordination**: Agent work registration and file claim/release tracking.

### Changed
- Phase 2 close: ADR-0009 self-consistency across M1/M4/M6/M2/L3/L1.
- `get_capabilities` `detail=compact` opt-in.

### Fixed
- Test race conditions and Clippy warnings.
