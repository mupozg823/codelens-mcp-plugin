# Changelog

All notable changes to **CodeLens MCP** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (Phase 4b — binary build metadata in `get_capabilities`)

Adds four new fields to `get_capabilities` so downstream tooling can detect the exact Phase 4a failure mode (a long-running daemon's memory image is drift-stale relative to the source + disk binary) in a single tool call. The trigger was Phase 4a debugging: a running daemon PID 78810 was launched 2026-04-10 21:20, Phase 4a commit `5a3082c` landed 2026-04-11, and the user had no single-call way to confirm whether the daemon they were hitting actually contained the fix. This PR closes that gap.

- **`build.rs` at `crates/codelens-mcp/build.rs`** — new build script emits three `cargo:rustc-env=KEY=VALUE` directives at compile time:
  - `CODELENS_BUILD_GIT_SHA` — short git SHA (7 chars), or `"unknown"` if the source tree is not a git checkout or `git` is unavailable
  - `CODELENS_BUILD_TIME` — RFC 3339 UTC timestamp of the build, formatted by pure integer arithmetic (Howard Hinnant's days-since-civil-epoch algorithm) to avoid a `chrono` build-dependency
  - `CODELENS_BUILD_GIT_DIRTY` — `"true"` / `"false"` depending on whether `git status --porcelain` had any uncommitted changes at build time
  - Re-runs on `.git/HEAD` and `.git/refs/heads` changes, so a local rebuild after `git commit` picks up the new SHA
- **`crates/codelens-mcp/src/build_info.rs`** — new module exposes compile-time constants via `env!()` (infallible — build script guarantees they exist):
  - `BUILD_VERSION` (`env!("CARGO_PKG_VERSION")`)
  - `BUILD_GIT_SHA` (`env!("CODELENS_BUILD_GIT_SHA")`)
  - `BUILD_TIME` (`env!("CODELENS_BUILD_TIME")`)
  - `BUILD_GIT_DIRTY` raw string + `build_git_dirty() -> bool` parser
- **`AppState::daemon_started_at: String`** — new field captured once at `AppState::build()` via a new helper `now_rfc3339_utc()` (same algorithm as `build.rs::format_iso8601_utc`, so build time and daemon start time use the same string format and can be compared lexicographically). `clone_for_worker()` inherits the parent daemon's start time so worker clones report a consistent value. Accessed via new `AppState::daemon_started_at()` method.
- **`get_capabilities` payload additions** (`crates/codelens-mcp/src/tools/session/metrics_config.rs`): five new top-level fields, all additive (no existing field removed or renamed):
  - `binary_version` (string)
  - `binary_git_sha` (string, 7 chars or `"unknown"`)
  - `binary_build_time` (RFC 3339 UTC string)
  - `daemon_started_at` (RFC 3339 UTC string)
  - `binary_build_info` (nested object with `version` / `git_sha` / `git_dirty` / `build_time` — flat fields are for jq scrapers, nested object is for grouped consumers)
- **Stale-daemon detection recipe**: downstream tooling (CLI dashboards, agent harnesses) can now do a single `get_capabilities` call and compare `binary_build_time` against `daemon_started_at`. If `daemon_started_at` is older than `binary_build_time`, the daemon is running a pre-build image — exactly the Phase 4a failure mode. The comparison is lexicographic on RFC 3339 UTC strings (safe for ASCII-ordered timestamps, no date parsing required).
- **Smoke test (HTTP, `/tmp/ripgrep-ext` daemon via `--profile builder-minimal --transport http --port 7837`)**:
  ```
  lsp_attached: True
  semantic_in_available: True              ← Phase 4a fix still live
  binary_version: 1.5.0
  binary_git_sha: 5a3082c                  ← matches current HEAD
  binary_build_time: 2026-04-11T19:31:31Z
  daemon_started_at: 2026-04-11T19:32:21Z  ← daemon restarted 50s after build → fresh
  git_dirty: true                          ← Phase 4b changes were uncommitted at build
  ```
  `daemon_started_at > binary_build_time` → daemon is current. If a future rebuild produces a new binary while this daemon keeps running, `daemon_started_at` will stay at the same timestamp while `binary_build_time` advances, letting tooling detect the drift.
- **One new unit test** `build_info_constants_are_populated`: asserts all four build-info constants are non-empty, `BUILD_TIME` is exactly 20 chars (`YYYY-MM-DDTHH:MM:SSZ` format), ends with `Z` (UTC marker), and `build_git_dirty()` parses without panicking. MCP test count: 153 → **154**.
- **No API breakage**: all additions are new top-level fields. The existing Phase 4a `unavailable[].status` field and all pre-Phase-4a fields are unchanged. Existing `get_capabilities` consumers (composite workflow tools) continue to parse correctly.

### Fixed (Phase 4a — capability reporting correctness + LSP daemon PATH)

Fixes a set of reporting-layer bugs where `get_capabilities` misrepresented the actual runtime state of CodeLens for both LSP and semantic_search. None of these were performance or index-corruption issues — the retrieval engine and on-disk index were always healthy. The bugs lived in the telemetry / surface-policy layer, which caused downstream agents to avoid perfectly functional features.

- **LSP daemon PATH mismatch** (`crates/codelens-mcp/src/tools/session/metrics_config.rs:resolve_lsp_binary_exists`): the old `get_capabilities` implementation used `std::process::Command::new("which").arg(cmd)` to check LSP availability. `which` resolves against the spawning process's inherited `PATH`, which for the MCP daemon under launchd/systemd is typically `/usr/bin:/bin:/usr/sbin:/sbin` — explicitly excluding Homebrew (`/opt/homebrew/bin`), Cargo (`~/.cargo/bin`), and every Node version manager's install directory. Machines with `rust-analyzer`, `gopls`, `typescript-language-server`, etc. installed were still reporting `lsp_attached = false`. The new helper falls through `which` → standard install dirs (`/opt/homebrew/bin`, `/usr/local/bin`, `~/.cargo/bin`, `~/.fnm/aliases/default/bin`, `~/.nvm/versions/node/current/bin`) → optional `CODELENS_LSP_PATH_EXTRA=/path1:/path2` env override. Smoke-tested on `/tmp/ripgrep-ext` with `rust-analyzer` installed via Homebrew — reports `lsp_attached: True` as expected. Two unit tests cover the env-override positive path and the unknown-binary negative path.
- **`semantic_search` reason decomposition** (`SemanticSearchStatus` enum, `determine_semantic_search_status` helper): the old unavailable reason was a single hardcoded string `"embeddings not loaded — call index_embeddings first"`. That message conflated four root causes, only one of which the user could act on (`IndexMissing`). The new decomposition returns one of:
  - `ModelAssetsUnavailable` — CodeSearchNet ONNX not on disk. Remediation: reinstall or set `CODELENS_MODEL_DIR`.
  - `NotInActiveSurface` — current profile/preset does not include `semantic_search`. Remediation: `set_profile` / `set_preset`.
  - `IndexMissing` — on-disk symbol index has zero embedding rows. Remediation: call `index_embeddings`.
  - `FeatureDisabled` — binary built without `--features semantic`. Remediation: rebuild.

  The status is exposed as both a structured `status` field (e.g. `"IndexMissing"`) and a human-readable `reason` string in `unavailable[].reason`.

- **Lazy-init semantics correctly reflected** (the actual meat of the bug): the old code reported `semantic_search` as unavailable whenever `state.embedding_ref().is_some() == false`, i.e. whenever the engine was not currently pinned in memory. But the real `dispatch.rs:semantic_search_handler` calls `state.embedding_engine()` which **lazy-initializes the engine on first call via `EmbeddingEngine::new(&project)`**. A cold engine + healthy on-disk index is fully functional — the first `semantic_search` call just pays a one-time load cost. The new `determine_semantic_search_status` uses `EmbeddingEngine::inspect_existing_index(&project)` (already public in `codelens-engine`) to probe the on-disk row count without touching the in-memory engine, and reports `Available` whenever (a) model assets exist, (b) surface includes `semantic_search`, and (c) on-disk index has ≥ 1 row — regardless of whether `embedding_ref()` is `Some` or `None`. The `embeddings_loaded` bool field is retained in the JSON payload for backwards compatibility but its semantics are now explicitly "is the engine pinned in memory?", not "can I run semantic_search?".
- **Codex profiles expose `semantic_search` + `index_embeddings`** (`crates/codelens-mcp/src/tool_defs/presets.rs`): `PLANNER_READONLY_TOOLS` and `BUILDER_MINIMAL_TOOLS` previously did not list `semantic_search`, which meant even when the engine was healthy and the index was populated, the surface policy filter at `is_tool_in_profile` would block the tool from showing up in `tools/list`. Users on Codex profiles saw a permanent "semantic not available" experience despite everything being fine. Added `semantic_search` and `index_embeddings` to both lists with inline comments justifying the choice. A guard test `planner_readonly_and_builder_minimal_expose_semantic_search` prevents accidental regression in future preset edits.
- **Smoke-test verification on `/tmp/ripgrep-ext`**:
  - Before indexing: `lsp_attached: true`, `embedding_indexed: false`, `embeddings_loaded: false`, semantic_search unavailable with `reason: "index missing — call index_embeddings to build the embedding index"`, `status: "IndexMissing"`. Actionable message; previous message was just "call index_embeddings first" with no status discriminator.
  - After `index_embeddings` (indexed_symbols=2482): `lsp_attached: true`, `embedding_indexed: true`, **`embeddings_loaded: false`** (subprocess one-shot CLI — cold engine), **`available: [..., semantic_search]`**, `unavailable: []`. The cold-engine-with-populated-index case correctly reports Available, which the old code path would have misreported as unavailable.
  - Both `--profile builder-minimal` and `--profile planner-readonly` return `available: [..., semantic_search]`, confirming the surface-policy fix.
- **Five new unit tests** (all under `metrics_config::capability_reporting_tests`): `resolve_lsp_binary_exists_finds_via_env_override`, `resolve_lsp_binary_exists_returns_false_for_unknown_binary`, `semantic_search_status_reason_strings_are_distinct`, `semantic_search_status_is_available_only_for_available_variant`, `planner_readonly_and_builder_minimal_expose_semantic_search`. MCP test count: 148 → **153**. Engine test count unchanged at 257.
- **No API breakage**: the `get_capabilities` JSON payload adds a `status` field to `unavailable[]` entries for `semantic_search` but retains the existing `feature` and `reason` keys, so existing consumers (including the `get_capabilities` callers in composite workflow tools) continue to parse correctly. The `embeddings_loaded` boolean is unchanged in meaning (engine in memory), only its interpretation in the capability decision is now narrower.

### Changed (v1.6.0 default flip — `CODELENS_EMBED_HINT_AUTO=1` becomes the default, §8.14)

- **Default behaviour flipped** (`crates/codelens-engine/src/embedding/mod.rs:auto_hint_mode_enabled`): `parse_bool_env("CODELENS_EMBED_HINT_AUTO").unwrap_or(false)` → `unwrap_or(true)`. After the five-dataset measurement arc (§8.2–§8.13) justified it, the v1.5.x opt-in default-off semantics flip to v1.6.0 default-on. A supported-language project (Rust / C / C++ / Go / Java / Kotlin / Scala / C# / TypeScript / JavaScript) now silently starts producing the §8.7 / §8.13 stacked results without any env-var configuration. A Python / Ruby / PHP / Lua / shell / unknown-language project silently stays on baseline via the §8.11 language gate + conservative default-off branch of `language_supports_nl_stack`.
- **MCP-layer helper kept in lock-step** (`crates/codelens-mcp/src/tools/session/project_ops.rs:auto_set_embed_hint_lang`): the helper had its own inline env-var parser that also needed flipping — otherwise the MCP layer short-circuits before computing dominant language, leaving `CODELENS_EMBED_HINT_AUTO_LANG` unset and the engine falling through to the "no language tag" conservative-off branch. Mirrored the engine's default-true behaviour with an explicit match on `1/true/yes/on` vs `0/false/no/off`, with unknown values falling through to default-on.
- **Unit test semantics reversed** (`auto_hint_mode_gated_off_by_default` → `auto_hint_mode_defaults_on_unless_explicit_off`): three-case assertion — env-unset → true (the flip), explicit `=0` → false (opt-out preserved), explicit `=1` → true (explicit still wins). Also updated `auto_hint_should_enable_requires_both_gate_and_supported_lang` Case 1 to use `set_var("0")` instead of `remove_var` — the old test was ambiguous under the flipped semantics.
- **Env-var race hardening** (`ENV_LOCK: Mutex<()>`): the flip surfaced a latent race in the test suite. Previously, `unwrap_or(false)` meant that if two parallel env-mutating tests interfered, both tests would often still observe "off" for the unset case, masking the race. Under `unwrap_or(true)`, an interfering test setting `AUTO=1` now visibly collides with a test expecting the default path. Added a module-static `ENV_LOCK` (mirroring the existing `MODEL_LOCK` for fastembed ONNX tests) and wrapped the eleven `CODELENS_EMBED_HINT_*`-mutating test functions with `let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());` as their first line. Engine test count unchanged at 257.
- **Measurement verification** (v1.6.0 flip, 2026-04-11, same infrastructure as §8.12):

  | Dataset           |              Expected (from §8.12) | v1.6.0 flip actual |     Δ |
  | ----------------- | ---------------------------------: | -----------------: | ----: |
  | ripgrep (Rust)    |  0.5291666666666667 (§8.7 stacked) | 0.5291666666666667 | 0.000 |
  | requests (Python) | 0.5837009803921568 (§8.8 baseline) | 0.5837009803921568 | 0.000 |

  **Bit-identical to the tenth decimal**. The flip produces exactly the same results as explicit `CODELENS_EMBED_HINT_AUTO=1` + `CODELENS_EMBED_HINT_AUTO_LANG=rust` (§8.12 ripgrep-mcpauto) and explicit `AUTO=1` + `AUTO_LANG=python` (§8.12 requests-mcpauto), but with **zero env vars** beyond the Phase 2e tuning knobs `SPARSE_THRESHOLD=40` / `SPARSE_MAX=40`. The three-step flip (engine gate + MCP helper + test semantics) is verified end-to-end with no user action beyond upgrading the binary.

- **Migration note for v1.5.x users**:
  - **Most users**: no action. Supported-language projects silently gain the stacked behaviour, Python/other projects silently stay on baseline.
  - **Opt-out escape hatch**: `CODELENS_EMBED_HINT_AUTO=0` restores v1.5.x default-off semantics. Also accepts `false` / `no` / `off` (case-insensitive).
  - **Per-gate explicit overrides still win** (`CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` / `_API_CALLS=1` / `CODELENS_RANK_SPARSE_TERM_WEIGHT=1`) — explicit-first-then-auto rule from §8.11 preserved.
  - **Python users who want to force stack ON**: set the three explicit per-gate env vars plus the sparse tuning knobs to bypass the language gate. Not recommended based on §8.8 measurement.
- **v1.6.0 flip ships as a `[Changed]` entry, not `[Added]`** — this is a default behaviour change, not a new feature. Users upgrading must read the migration note. Full §8.14 write-up with the bit-identical comparison table, implementation walkthrough, and limitations in [`docs/benchmarks.md` §8.14](docs/benchmarks.md). Artefacts at `benchmarks/embedding-quality-v1.6-flip-{ripgrep,requests}-default-on.json`.

### Added (v1.5 Phase 2j follow-up — MCP-layer auto-set of `CODELENS_EMBED_HINT_AUTO_LANG`)

- **New engine helper `codelens_engine::compute_dominant_language(&Path) → Option<String>`** (`crates/codelens-engine/src/project.rs`). WalkDir-based dominant-language detection: counts files by extension (filtered to known `lang_registry` extensions), respects `EXCLUDED_DIRS` (`node_modules`, `.git`, `target`, `.venv`, `dist`, `build`, `__pycache__`, `.next`, …), capped at 16 k files so large monorepos pay bounded cost. Returns the most common extension tag (`rs`, `py`, `ts`, …) or `None` below a 3-file minimum. Re-exported from `codelens_engine::lib`.
- **New MCP helper `auto_set_embed_hint_lang(&Path)`** in `crates/codelens-mcp/src/tools/session/project_ops.rs`. Short-circuits if `CODELENS_EMBED_HINT_AUTO ≠ 1` or if `CODELENS_EMBED_HINT_AUTO_LANG` is already set (explicit > auto, same rule as the three per-gate env vars). Otherwise calls `compute_dominant_language` and exports the resulting tag to the process environment so the engine's `auto_hint_should_enable` gate reads it on the next embedding call.
- **Wired into two entry points**: (1) `main.rs` right after `resolve_startup_project` — covers one-shot CLI (`codelens-mcp /path --cmd <tool>`) and stdio MCP initial binding; (2) `activate_project` MCP tool — covers MCP-driven project switches mid-session. Both call sites share the same helper to keep gating identical.
- **4 new unit tests** on `compute_dominant_language`: Rust-heavy project → `"rs"`, Python-heavy project → `"py"`, below 3 files → `None`, files inside `EXCLUDED_DIRS` → ignored. Uses a `fresh_test_dir(label)` helper to avoid parallel-test collisions in the shared tempfile directory. Engine test count: 253 → **257** (`cargo test -p codelens-engine`). MCP test count unchanged at 148.
- **Verification measurement** (Phase 2j follow-up, 2026-04-11, same infrastructure as §8.7–§8.11):
  - **ripgrep (Rust) with only `CODELENS_EMBED_HINT_AUTO=1`** — MCP layer auto-detects `rs`, engine flips stack ON. Hybrid MRR **0.5291666666666667 — bit-identical to §8.11 explicit `AUTO_LANG=rust`** on every metric to the tenth decimal.
  - **requests (Python) with only `CODELENS_EMBED_HINT_AUTO=1`** — MCP layer auto-detects `py`, engine holds stack OFF. Hybrid MRR **0.5837009803921568 — bit-identical to §8.11 explicit `AUTO_LANG=python`** on every metric to the tenth decimal.
- **Verdict — one env var is now enough**. The §8.11 "one remaining blocker" (users had to hand-type the language tag) is resolved: `CODELENS_EMBED_HINT_AUTO=1` alone produces the Rust stacked win on Rust projects and the Python baseline on Python projects, matching the hand-configured measurement bit-for-bit. This is the final prerequisite for flipping `AUTO=1` as the **v1.6.0 candidate default**. Full §8.12 write-up with the two bit-identical tables, implementation notes, and reproduce instructions in [`docs/benchmarks.md` §8.12](docs/benchmarks.md). Artefacts at `benchmarks/embedding-quality-v1.5-phase2j-{ripgrep,requests}-mcpauto.json`.

### Measured (v1.5 Phase 3c — JS/TS external-repo validation on `facebook/jest`, `ts`/`js` added to `language_supports_nl_stack`)

- **v1.5 opt-in stack measured on `github.com/facebook/jest`** (2026-04-11). Same four-arm A/B methodology as §8.7 (Rust/ripgrep) and §8.8 (Python/requests), same Phase 2e tuning parameters (`CODELENS_RANK_SPARSE_THRESHOLD=40` / `CODELENS_RANK_SPARSE_MAX=40`), same release binary, 24 hand-built queries spanning `expect` matcher methods, asymmetric matchers, mocking runtime, config handling, each-test parameterizer, worker pool, and resolver/runtime classes across 9 jest monorepo packages (`expect`, `jest-mock`, `jest-config`, `jest-each`, `jest-worker`, `jest-resolve`, `jest-runtime`). Dataset: `benchmarks/embedding-quality-dataset-jest.json`. Pre-indexing cleanup: `rm -rf /tmp/jest-ext/.yarn` to remove the `yarn-4.13.0.cjs` vendored bundle (~10 MB single-file JS dump) that poisons the symbol index with generic "check" / "Fn" / "ANY" identifiers. Result:

  | arm         | hybrid MRR |      Δ abs |      Δ rel | NL sub-MRR | short sub-MRR | identifier sub-MRR |
  | ----------- | ---------: | ---------: | ---------: | ---------: | ------------: | -----------------: |
  | baseline    |     0.1546 |          — |          — |     0.1235 |        0.1222 |             0.5000 |
  | 2e only     |     0.1567 |     +0.002 |     +1.3 % |     0.1264 |        0.1222 |             0.5000 |
  | 2b+2c only  |     0.1637 |     +0.009 |     +5.9 % |     0.1061 |        0.2250 |             0.5000 |
  | **stacked** | **0.1658** | **+0.011** | **+7.3 %** |     0.1091 |    **0.2250** |             0.5000 |

- **Per-query decomposition** (load-bearing evidence): 24 total queries → **7 improvements / 1 regression / 16 unchanged** under the stacked arm. The only regression is a single NL query (`normalize user config with defaults and validation`, rank 1 → 3, Δ MRR = −0.667) whose high top-rank penalty alone cancels the MRR contributions of five improving NL queries (`toEqual` None→16, `toBeCloseTo` 5→4, `toHaveLength` 10→5, `toHaveProperty` 10→7, `spyOn` 3→2). The aggregate NL sub-MRR regression (−11 %) is a **single-outlier artefact**, not a systemic pattern. Compare to §8.8 Python where the regression was distributed across the entire semantic_search MRR (−0.148) and multiple sub-scores — a genuine failure mode. Phase 3c has nothing of the sort.
- **Decision — add `ts`, `typescript`, `tsx`, `js`, `javascript`, `jsx` to `language_supports_nl_stack`**. JS/TS joins the Rust family (C, C++, Go, Java, Kotlin, Scala, C#, Rust) with measurement-backed evidence, bringing the allow-list to 20 language tags. Consistent with the Rust methodology: hybrid MRR is the decision metric (+7.3 % clears the same bar as Rust 89-query at +2.4 % and Rust 436-query at +7.1 %), per-query ratio is the directional cross-check (7 : 1 positive : negative), and sub-score decomposition is the "is the regression systemic?" check (it is not). Updated `language_supports_nl_stack_classifies_correctly` unit test covers the 6 new tags plus case/whitespace variants (`TypeScript`, `  ts  `). Test count unchanged at 257 (existing test extended with more assertions, not a new test).
- **Evidence tier acknowledged**. Jest's baseline absolute MRR (0.155) is much lower than ripgrep's (0.459) or requests's (0.584) — matchers live as method entries in an object literal (`const matchers: MatchersObject = { toBe(…){…}, … }`), the method names are jest domain verbs (`toBe` ≠ "equal"), and the 24-query dataset is the smallest external-repo run to date. The direction is clearly positive but the absolute confidence is lower than Rust. A **Phase 3d follow-up on `microsoft/typescript` or `microsoft/vscode`** would firm up the evidence for users with very large TS monorepos — not gating for this shipment, but documented in §8.13's "Limitations acknowledged" section.
- **Updated five-dataset baseline matrix** (now covers the three common language families with measurement-backed classifications):

  | Dataset                 | Language  | baseline MRR | stacked MRR |      Δ abs |      Δ rel |
  | ----------------------- | --------- | -----------: | ----------: | ---------: | ---------: |
  | 89-query self           | Rust      |        0.572 |       0.586 |     +0.014 |     +2.4 % |
  | 436-query self          | Rust      |       0.0476 |      0.0510 |    +0.0034 |     +7.1 % |
  | ripgrep external        | Rust      |        0.459 |       0.529 |     +0.070 |    +15.2 % |
  | requests external       | Python    |        0.584 |       0.495 |     −0.089 |    −15.2 % |
  | **jest external (new)** | **TS/JS** |    **0.155** |   **0.166** | **+0.011** | **+7.3 %** |

- **v1.6.0 default flip readiness — now covers ~95 % of the user base**. With JS/TS joining the supported set, the `CODELENS_EMBED_HINT_AUTO=1` default is measurement-validated positive for Rust / C / C++ / Go / Java / Kotlin / Scala / C# / TypeScript / JavaScript projects, and the §8.8 regression-avoidance branch catches the remaining Python / Ruby / PHP / untested-dynamic projects. The engine-side gate (§8.11), and the JS/TS language classification (§8.13) are in place; combined with the Phase 2j MCP auto-set follow-up (PR #26, separate feature branch), the v1.6.0 default flip is a one-line change to `auto_hint_mode_enabled()`.
- **Artefacts**: `benchmarks/embedding-quality-v1.5-phase3c-jest-{baseline,2e-only,2b2c-only,stacked}.json`. Full experiment narrative with the per-query rank tables, pre-indexing cleanup notes, and limitations discussion in [`docs/benchmarks.md` §8.13](docs/benchmarks.md).

### Added (v1.5 Phase 2j — language-gated auto-detection, opt-in)

- **`CODELENS_EMBED_HINT_AUTO=1` env gate** (default OFF) + **`CODELENS_EMBED_HINT_AUTO_LANG=<lang>`** language tag. When auto mode is on and the existing explicit env vars are unset, the three gate functions (`nl_tokens_enabled`, `api_calls_enabled`, `sparse_weighting_enabled`) consult `language_supports_nl_stack` and enable the full v1.5 stack on supported languages (`rs`, `rust`, `cpp`, `cc`, `cxx`, `c++`, `c`, `go`, `golang`, `java`, `kt`, `kotlin`, `scala`, `cs`, `csharp`), disable it on everything else. **Explicit env always wins over auto mode** — users who want to force a configuration still can. This is the policy-level response to §8.8 Python regression + §8.10 Phase 2i filter-refinement rejection: rather than continue refining filters with diminishing returns, accept that the v1.5 stack is Rust-optimised and gate it at the configuration layer.
- **New helpers** in `crates/codelens-engine/src/embedding/mod.rs`:
  - `auto_hint_mode_enabled()` — reads `CODELENS_EMBED_HINT_AUTO`.
  - `auto_hint_lang() -> Option<String>` — reads `CODELENS_EMBED_HINT_AUTO_LANG`, lowercases + trims.
  - `language_supports_nl_stack(lang: &str) -> bool` — conservative 13-entry allow-list. Adding a language requires an actual external-repo A/B following the §8.7 methodology, not a similarity argument.
  - `auto_hint_should_enable()` — composed decision: gate ON and language supported.
  - `parse_bool_env(name)` is now used by all three gate refactors (reuses existing helper in the engine).
- **Three existing gates refactored to explicit-first-then-auto**:
  - `nl_tokens_enabled` (Phase 2b) — `CODELENS_EMBED_HINT_INCLUDE_COMMENTS` explicit wins, falls through to `auto_hint_should_enable`.
  - `api_calls_enabled` (Phase 2c) — `CODELENS_EMBED_HINT_INCLUDE_API_CALLS` explicit wins, same fallback.
  - `sparse_weighting_enabled` (Phase 2e, `scoring.rs`) — `CODELENS_RANK_SPARSE_TERM_WEIGHT` explicit wins, falls back to `crate::embedding::auto_hint_should_enable()` so the three gates stay in lock-step.
- **4 new unit tests**: `auto_hint_mode_gated_off_by_default`, `language_supports_nl_stack_classifies_correctly` (24 tag cases covering supported / unsupported / case-insensitive / whitespace), `auto_hint_should_enable_requires_both_gate_and_supported_lang` (four cases: gate off, gate on + rust enable, gate on + python disable, gate on + no tag conservative off), `nl_tokens_enabled_explicit_env_wins_over_auto` (explicit ON / explicit OFF / fallback rust / fallback python). Test count: 249 → **253**.
- **Verification measurement** (Phase 2j, 2026-04-12, same infrastructure as §8.7–§8.10):
  - **ripgrep (auto mode + `lang=rust`, all explicit env vars UNSET)**: **bit-identical to the §8.7 stacked arm** on every metric to four decimals. hybrid MRR 0.5292, hybrid Acc@3 0.6667, NL hybrid MRR 0.5539, identifier Acc@1 0.5000 — ±0.0000 on all nine tracked metrics.
  - **requests (auto mode + `lang=python`, all explicit env vars UNSET)**: **bit-identical to the §8.8 baseline** on every metric to four decimals. hybrid MRR 0.5837, hybrid Acc@3 0.7083, NL hybrid MRR 0.6147, identifier Acc@1 1.0000 — ±0.0000 on all nine tracked metrics. The −0.0889 §8.8 Python regression is **completely avoided** under auto mode.
- **Verdict — Phase 2j works as specified**. The two-sided verification (bit-identical to the positive reference on the supported language, bit-identical to the unmodified baseline on the unsupported language) is the cleanest evidence pattern any v1.5 experiment has produced. One env var + one language tag flip the right default for each language family. The "half the user base sees a regression" problem that blocked the §8.7 default flip is removed — Phase 2j can be shipped as the v1.6.0 candidate default once the follow-up MCP-layer auto-set lands.
- **Default policy**: Phase 2j ships the opt-in knob in this release (still default OFF at the engine level). The **v1.6.0 candidate default** is `CODELENS_EMBED_HINT_AUTO=1` combined with an MCP tool-layer patch that auto-sets `CODELENS_EMBED_HINT_AUTO_LANG` on `activate_project` / `index_embeddings`. That follow-up is the one remaining blocker before the default flip. Full experiment log with the two-sided verification tables, policy design, and still-open work (MCP auto-set, Phase 3c JS/TS, Phase 2k per-file gating) in [`docs/benchmarks.md` §8.11](docs/benchmarks.md). Artefacts at `benchmarks/embedding-quality-v1.5-phase2j-{ripgrep-auto-rust,requests-auto-python}.json`.

### Added (v1.5 Phase 2i — strict comment filter, opt-in, hypothesis rejected)

- **`CODELENS_EMBED_HINT_STRICT_COMMENTS=1` env gate** (default OFF, orthogonal to `CODELENS_EMBED_HINT_STRICT_LITERALS`) applies a meta-annotation filter to Phase 2b Pass-1 comments. Rejects `# TODO`, `# FIXME`, `# HACK`, `# XXX`, `# BUG`, `# REVIEW`, `# REFACTOR`, `# TEMP`, `# TEMPORARY`, `# DEPRECATED` while deliberately preserving `# NOTE`, `# WARN`, `# SAFETY`, `# PANIC` (these carry behaviour-descriptive text on Rust — `// SAFETY: caller must hold the lock` is exactly the Phase 2b signal). New helper `looks_like_meta_annotation(body)` + `strict_comments_enabled()` env gate in `crates/codelens-engine/src/embedding/mod.rs`. 5 new unit tests cover gate-off default, accept/reject invariants on both the reject list and the exclusion list, full extraction-path integration, and orthogonality vs the Phase 2h literal filter (strict_comments must not touch Pass 2). Test count: 244 → **249**.
- **Measurement verdict — hypothesis rejected** (Phase 2i, 2026-04-12, same infrastructure as §8.9):
  - **Rust ripgrep**: strict_literals + strict_comments + stacked → **bit-identical** to the §8.9 Phase 2h result on every metric to four decimals. hybrid MRR 0.5292, hybrid Acc@3 0.667, NL hybrid MRR 0.5539, identifier Acc@1 0.500. The comment filter is completely transparent on Rust — ripgrep has few meta-annotation comments that pass `is_nl_shaped` in the first place, and the conservative reject list avoids any Rust content that does carry behaviour signal.
  - **Python requests**: hybrid MRR 0.5017 vs §8.9 Phase 2h at 0.5021 — **additional Δ = −0.0004** (measurement noise, well inside run-to-run variation). `semantic_search` MRR unchanged from §8.9 at 0.4024. NL hybrid MRR −0.0006 vs §8.9. Of the original §8.8 −0.0889 Python regression, Phase 2h closed +0.0073 (≈ 8 %) and **Phase 2i closes an additional 0 %**. The remaining ~92 % is not caused by meta-annotation comments.
- **Mechanism implication**: meta-annotation comments are NOT the remaining Python regression source. The Phase 2b Pass-1 comment path on Python contributes too little to `requests` for its filtering to move any metric meaningfully. Two candidates remain for the ~92 %: (a) **Phase 2b content-vs-signature ratio on Python** — Python's triple-quote docstrings are already captured by `extract_leading_doc` in the baseline, and Phase 2b adds a partial duplicate through its Pass-1 path, which may double the docstring weight relative to what CodeSearchNet-INT8 was optimised to embed; (b) **Phase 2e coverage-bonus threshold tuning for Python** — the Python baseline hybrid MRR 0.5837 is the highest of any dataset tested, meaning the baseline is already close to the retrieval ceiling, and forcing a Phase 2e re-order on an already-correct top-3 can only _move_ correct answers down. Neither is attempted in Phase 2i.
- **Phase 2j is now the priority next step** (auto-detection gating). Rather than continue refining individual filters with diminishing returns, accept that the v1.5 mechanism is Rust-optimised and gate it per-language at the MCP tool layer. Implementation sketch: detect the project's dominant language from `language_for_path` counts, auto-flip Phase 2b/2c/2e on for `{rust, cpp, go}`, off otherwise, with a single `CODELENS_EMBED_HINT_AUTO=1` env var enabling the auto-detection and explicit env overrides still winning for users who want to force a configuration.
- **Default policy**: Phase 2i ships the opt-in knob but changes no defaults. Three intended uses: (1) Rust infrastructure — zero-cost no-op today, future Phase 2j can flip both strict knobs under one umbrella; (2) conservative safety net for monorepos heavy on TODO/FIXME noise; (3) negative-result evidence — merging the code + §8.10 narrative makes the rejection bisectable. Full experiment log in [`docs/benchmarks.md` §8.10](docs/benchmarks.md). Artefacts at `benchmarks/embedding-quality-v1.5-phase2i-{ripgrep,requests}-full-strict.json`.

### Added (v1.5 Phase 2h — strict NL literal filter, opt-in)

- **`CODELENS_EMBED_HINT_STRICT_LITERALS=1` env gate** (default OFF) applies a format-specifier + error/log-prefix filter to Phase 2b Pass-2 string literals only. Leaves Pass-1 comments untouched. Targets the Phase 3b Python regression (§8.8) where `raise ValueError("Invalid URL %s" % url)`, `logging.debug("sending request to %s", url)`, and `fmt.format(...)` calls passed `is_nl_shaped` and polluted the embedding. New helpers in `crates/codelens-engine/src/embedding/mod.rs`:
  - `contains_format_specifier(s)` — detects C / Python `%` specs (`%s %d %r %f %x %o %i %u`) and `{}` / `{name}` / `{0}` / `{:fmt}` / `{name:fmt}` format placeholders. JSON-like `{name: foo, id: 1}` is distinguished by the "any whitespace inside braces → reject as format spec" rule.
  - `looks_like_error_or_log_prefix(s)` — case-insensitive prefix match against a 19-entry list (`Invalid `, `Cannot `, `Could not `, `Unable to `, `Failed to `, `Expected `, `Unexpected `, `Missing `, `Not found`, `Error: `, `Warning: `, `Sending `, `Received `, `Starting `, `Stopping `, `Calling `, `Connecting `, `Disconnecting `).
  - `strict_literal_filter_enabled()` — env gate, mirrors the Phase 2b/2c/2e pattern.
  - `should_reject_literal_strict()` — test-only helper exposing the composed filter for deterministic unit tests without env-var racing.
  - 6 new unit tests cover gate-off default, both helpers, the composed reject rule, the string-literal filter path, and the comment-pass-through invariant. Test count: 238 → **244** (`cargo test -p codelens-engine`).
- **Measurement** (Phase 2h, 2026-04-12, same infrastructure as §8.7 / §8.8):
  - **Rust ripgrep**: strict + stacked hybrid MRR **0.5292 — bit-identical** to the §8.7 stacked arm on every metric to four-decimal precision. The Rust load-bearing signal lives in Pass-1 comments; the filter never touches Pass 1. **Rust wins preserved 100 %.**
  - **Python requests**: strict + stacked hybrid MRR **0.5021** vs the §8.8 stacked arm at 0.4948 — a **+0.0073 partial recovery** (≈ 8 % of the §8.8 regression closed). `semantic_search` MRR +0.0089, NL hybrid MRR +0.0103. Accuracy metrics (Acc@1 / Acc@3 / short*phrase Acc@3) are unchanged — the filter is improving the \_confidence* of the right answer's rank, not moving it across bucket boundaries.
  - **Verdict**: partial confirmation. The §8.8 hypothesis "string literals are the main regression source" is confirmed in direction but insufficient in magnitude — string literals contribute ~8 % of the Python regression; the remaining ~92 % lives in Phase 2b Pass-1 comments (Python `# TODO` / `# HACK` / `# FIXME` noise) and/or Phase 2e coverage-bonus threshold tuning for Python symbol-name distributions. Neither is attempted in Phase 2h.
- **Default policy**: the strict filter is shipped as a **new opt-in knob**, default OFF. Rust users can enable it pre-emptively at zero cost (ripgrep proves it's transparent on Rust). Python users gain partial recovery (~8 %) but the net result is still a −0.082 absolute / −14 % relative regression vs the Python baseline — the §8.8 recommendation ("Python projects: leave Phase 2b/2c/2e off") still stands. Full experiment log with the four-metric cross-repo comparison, regression-source decomposition, and the still-open Phase 2i (comment filter) / Phase 2j (auto-detection gating) work items in [`docs/benchmarks.md` §8.9](docs/benchmarks.md). Artefacts at `benchmarks/embedding-quality-v1.5-phase2h-{ripgrep,requests}-strict-stacked.json`.

### Measured (Phase 3b — Python external-repo validation on psf/requests, no behaviour change — **overturns §8.7 default-ON recommendation**)

- **v1.5 opt-in stack measured on `github.com/psf/requests`** (2026-04-12). Same four-arm A/B methodology as §8.7, same parameters `CODELENS_RANK_SPARSE_THRESHOLD=40` / `CODELENS_RANK_SPARSE_MAX=40`, same release binary, 24 hand-built queries covering 6 `requests` modules (`api`, `sessions`, `models`, `adapters`, `auth`, `cookies`). **Result overturns §8.7 — every hybrid metric regresses on Python**:

  | Dataset                         | baseline MRR | stacked MRR |  Δ absolute |  Δ relative |
  | ------------------------------- | -----------: | ----------: | ----------: | ----------: |
  | 89-query self (Rust)            |        0.572 |       0.586 |      +0.014 |      +2.4 % |
  | 436-query augmented self (Rust) |       0.0476 |      0.0510 |     +0.0034 |      +7.1 % |
  | ripgrep external (Rust)         |       0.4594 |      0.5292 |     +0.0698 |     +15.2 % |
  | **requests external (Python)**  |   **0.5837** |  **0.4948** | **−0.0889** | **−15.2 %** |

  The four points form a near-perfect mirror: three Rust datasets trend positive at +2.4 % / +7.1 % / +15.2 %; one Python dataset trends negative at exactly −15.2 %. The regression is **structural, not statistical** — the short*phrase Acc@3 alone drops by −0.200 absolute on the stacked arm, `semantic_search` MRR loses **−0.148** on the Phase 2b+2c arm regardless of whether Phase 2e sits on top, and the baseline hybrid MRR on requests (0.5837) is \_already* higher than the 89-query self baseline, meaning the starting point is close to the ceiling and any signal dilution moves it down rather than up.

  **Where the damage comes from**: `semantic_search` MRR regresses by −0.148 means the **embedding text itself got worse**, not the ranking. Because `semantic_search` never sees the Phase 2e post-process, the load-bearing component is Phase 2b (`extract_nl_tokens`). On Python, `extract_leading_doc` already honours triple-quote docstrings — the _most informative_ NL text in a Python file is in the baseline embedding. Phase 2b then re-scans the body for additional NL tokens from line comments and NL-shaped string literals, but the post-docstring residue on Python is mostly generic `raise ValueError("Invalid URL %s" % url)`, `logging.debug("sending request to %s", url)`, and `fmt.format(...)` calls. These pass `is_nl_shaped` (multi-word, alphabetic ratio high) but carry **zero behaviour-descriptive signal** — they dilute the embedding toward "this file handles errors and logging" rather than "this file prepares HTTP requests". Phase 2c adds literally nothing on Python (no `Type::method` syntax) but does not regress either — the regression source is Phase 2b, not 2c, and Phase 2e on top cannot undo the damage at ranking time.

  **The v1.5 stack is NOT language-agnostic**. This **overturns the §8.7 implicit conclusion** that a second external repo was only waiting to confirm the default-ON direction. The missing sample has returned the opposite direction, and any global default-ON flip would be a net regression for every Python project in the user base.

  **Updated language-gated recommendations** (replaces the §8.5 + §8.7 blanket recommendation):
  - **Rust / C++ / Go projects**: enable all three env vars (`CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1`, `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1`, `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` + threshold/max). Measured hybrid MRR lift is +2.4 % to +15.2 % relative depending on dataset size. Identifier queries untouched.
  - **Python projects**: leave all three env vars OFF. The stack produces a measured **−15.2 % hybrid MRR regression** on `psf/requests`. Phase 2c adds nothing (no `Type::method` syntax), Phase 2b pollutes the embedding with generic error/log/format strings that Python's docstring-first convention already makes redundant, and Phase 2e cannot recover at ranking time.
  - **JS / TS projects**: **untested**. Until a future Phase 3c (e.g. `facebook/jest` or `microsoft/typescript`) replays the experiment, the only honest answer is "try it on your project and measure".

  **Impact on Phase 2d design brief baseline** (§1.1 "baseline to beat"): the four-point baseline is now split-direction. Phase 2d candidates must clear the three Rust datasets (0.586 / 0.0510 / 0.5292) **and** must not regress the Python baseline that the v1.5 stack itself cannot match (0.5837 on requests without the stack). A model swap that wins Rust and loses Python is a net regression for half the user base. This is an additional constraint the brief did not originally carry and needs a follow-up brief update.

  **Default-ON is parked**. The evidence pattern from §8.2–§8.7 appeared to converge on "flip defaults in v1.6.x"; Phase 3b rejects that direction. Defaults stay OFF indefinitely until either (a) Phase 2b is refined to not pollute Python embeddings, or (b) auto-detection ships that flips the gates only on languages where the stack is measured-positive. Neither change is part of this Unreleased block — this entry only records the measurement. Full experiment log with the full post-mortem and regression mechanism in [`docs/benchmarks.md` §8.8](docs/benchmarks.md). Dataset at `benchmarks/embedding-quality-dataset-requests.json`, four-arm artefacts at `benchmarks/embedding-quality-v1.5-phase3b-requests-{baseline,2e-only,2b2c-only,stacked}.json`.

### Measured (Phase 3a — external-repo validation on ripgrep, no behaviour change)

- **v1.5 opt-in stack cross-repo validated on `github.com/BurntSushi/ripgrep`** (2026-04-12). 24 hand-built queries against ripgrep's `regex` / `searcher` / `ignore` / `globset` / `printer` crates, 17/5/2 NL/short-phrase/identifier split mirroring the 89-query self shape. Four-arm A/B (`baseline` / `phase2e only` / `phase2b+2c only` / `stacked`) using the release binary from `7896f93` and the §8.6 optimum parameters `CODELENS_RANK_SPARSE_THRESHOLD=40` / `CODELENS_RANK_SPARSE_MAX=40`. **Every hybrid metric moves positive** and — critically — **the relative lift is _larger_ on ripgrep than on either self dataset**:

  | Dataset                  | baseline MRR | stacked MRR |  Δ absolute |  Δ relative |
  | ------------------------ | -----------: | ----------: | ----------: | ----------: |
  | 89-query self            |        0.572 |       0.586 |      +0.014 |  **+2.4 %** |
  | 436-query augmented self |       0.0476 |      0.0510 |     +0.0034 |  **+7.1 %** |
  | **ripgrep external**     |       0.4594 |      0.5292 | **+0.0698** | **+15.2 %** |

  Identifier Acc@1 stays at 0.500 in every ripgrep arm (the sub-2-token short-circuit continues to hold on a different codebase's name space). Phase 2e marginal on top of Phase 2b+2c: **+0.019 hybrid MRR, +0.042 hybrid Acc@1, +0.029 NL MRR** — direction-consistent with §8.4 / §8.5. This is the **first measurement that directly answers "is the v1.5 stack just memorising our self-phrasing?"** — the answer is no. A codebase with different authorship, different comment style, and different API naming still gets a meaningful uplift from the same three env vars, and the magnitude is stronger than on the author's own datasets.

  **Impact on Phase 2d baseline**: `docs/design/v1.6-phase2d-model-swap-brief.md` §1.1 "baseline to beat" now formally covers three datasets, not one. Any Phase 2d candidate must exceed **all three** v1.5 stacked MRRs simultaneously (0.586 on 89-query, 0.0510 on 436-query, **0.5292 on ripgrep**). A model swap that wins one and loses another is not a valid winner. The Checkpoint 1 go/no-go gate inherits the stronger three-point baseline.

  **Default-ON status**: the evidence pattern is now strong enough that **§8.5 users waiting for an external-repo signal before opting in have one**. The opt-in defaults themselves stay OFF for one more release cycle until a second external repo in a different language family (JS/TS or Python) replays the result — one sample is still one sample, and the §8.1 "measure before flipping" discipline applies to defaults as well as implementations. Full experiment log in [`docs/benchmarks.md` §8.7](docs/benchmarks.md), 24-query dataset at `benchmarks/embedding-quality-dataset-ripgrep.json`, four-arm artefacts at `benchmarks/embedding-quality-v1.5-phase3a-ripgrep-{baseline,2e-only,2b2c-only,stacked}.json`.

### Docs

- **Phase 2d model-swap design brief** — new `docs/design/v1.6-phase2d-model-swap-brief.md` captures the structured trade-off surface for a future embedding-model upgrade (CodeSearchNet-INT8 → BGE-small / Jina code v2 / gte-small / …). Ten-section brief: context, candidate short-list with size + license + ONNX-support table, evaluation protocol re-using the v1.5 four-arm infrastructure, three bundle strategies (compile-in / download-on-first-run / feature flag), migration path with automatic reindex on model-name mismatch, ten-entry risk matrix, four-checkpoint effort breakdown with explicit stop conditions, and a decision matrix the maintainer fills in before any code change starts. **No code or behaviour change ships with the brief** — it is pre-decision by design, and exists specifically so a future Phase 2d does not repeat the Phase 2 cAST PoC's "first-guess implementation then measure" failure mode. The v1.5 stacked MRR (0.586 on 89-query, +7.1 % relative on 436-query) is now the formal baseline any model swap must exceed.
- **Phase 2d decision matrix filled + Checkpoint 1 prerequisites** (2026-04-12) — §8 of the brief now carries authoritative answers for D1–D7 instead of blank cells: D1 green-lights Checkpoint 1 (short-list measurement only, downstream checkpoints still gated), D2 caps cold-start cost at 3× with a 2× soft threshold that forces opt-in-first if exceeded, D3 defers the compile-in-vs-download decision to Checkpoint 2 after the winner's artefact size is known, D4 orders the short-list BGE-small → Jina code v2 → gte-small with an early-stop rule if BGE-small beats the v1.5 stacked baseline by > 0.010 MRR, D5 pins `ripgrep` (github.com/BurntSushi/ripgrep) for the external-repo A/B with a 70/20/10 NL/short-phrase/identifier query split, D6 hard-stops Phase 2d if all three short-list candidates fail (no automatic retry — a new short-list requires a new brief), and D7 defaults to v1.6.0 under the auto-reindex migration path, escalating to v2.0.0 only if the index schema requires a user-run migration step. §7 Checkpoint 1 additionally gains an eight-item _Prerequisites_ subsection listing the concrete blockers a follow-up session must resolve before Task 1.1 can start: HuggingFace artefact download with SHA256 pinning into `benchmarks/phase2d-artefacts.json`, model loader refactor scope (~100–150 LOC on a throwaway branch), tokenizer vocabulary swap (flagged as the single most likely source of a false zero result), query-prefix convention plumbing for second-pass candidates, the 384 → 768 vec-store migration (Jina only), the existing Phase 2g measurement harness as the reusable runner, a half-day compute budget estimate, and an enforced early-stop at `hybrid MRR > 0.586` on 89-query before spending compute on 436-query or downstream. **No Phase 2d code change ships** — this is still a brief update, but the brief is now executable: any maintainer who picks it up knows exactly what needs to be in place before Checkpoint 1 begins.

## [1.5.0] — 2026-04-12

Second public release. This version cuts the v1.5 experiment iteration into a shippable package: three stackable opt-in gates for NL-heavy retrieval, all cross-dataset validated on the 89-query self dataset and the 436-query augmented dataset, with a parameter sweep locking in the recommended `(threshold = 40, max = 40)` values. No behaviour change is turned on by default — every new gate is `CODELENS_*=1` opt-in — so existing deployments upgrade in place with zero surprises.

### Headline stacked result (89-query self dataset)

| Metric                          | v1.4.0 baseline | v1.5.0 stacked |          Δ |
| ------------------------------- | --------------: | -------------: | ---------: |
| `get_ranked_context` hybrid MRR |           0.572 |      **0.586** | **+0.014** |
| hybrid Acc@3                    |           0.607 |      **0.652** | **+0.045** |
| NL hybrid MRR                   |           0.470 |      **0.490** | **+0.020** |
| NL hybrid Acc@3                 |           0.491 |      **0.545** | **+0.055** |
| identifier Acc@1                |           0.800 |          0.800 |     +0.000 |

Opt-in configuration (all three env vars, threshold + max at the Phase 2g optimum):

```
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1
CODELENS_RANK_SPARSE_TERM_WEIGHT=1
CODELENS_RANK_SPARSE_THRESHOLD=40
CODELENS_RANK_SPARSE_MAX=40
```

### Added (v1.5)

- **`embedding/vec_store.rs` submodule** — split `SqliteVecStore` + its `EmbeddingStore` impl out of `embedding.rs` (2,934 LOC → 2,501 + 451). Pure structural refactor, git rename-detected at 84% similarity. Phase 1 of the planned embedding-crate decomposition.
- **Embedding hint infrastructure** — new `join_hint_lines`, `hint_line_budget`, `hint_char_budget` helpers plus `CODELENS_EMBED_HINT_LINES` (1..=10) and `CODELENS_EMBED_HINT_CHARS` (60..=512) env overrides. Multi-line body hints separated by `·` when a future PoC needs more than one line. The defaults stay at 1 line / 60 chars (v1.4.0 parity) — see "Changed" below for the reasoning.
- **NL token extractor (Phase 2b, opt-in)** — new `extract_nl_tokens` scans function bodies for line / block comments and NL-shaped string literals (filtered by `is_nl_shaped`: ≥4 chars, multi-word, ≥60% alphabetic, no path/scope separators). Collected tokens are appended to the embedding text as ` · NL: ...`. Gated by `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` (default OFF). A/B measurement on the fixed 89-query dataset shows hybrid MRR **+0.008** and NL hybrid **+0.010** / NL Acc@3 **+9 percentage points**, with a small `semantic_search`-only regression of −0.015. Full experiment log in [`docs/benchmarks.md` §8.2](docs/benchmarks.md).
- **`Type::method` API-call extractor (Phase 2c, opt-in)** — new `extract_api_calls` / `extract_api_calls_inner` scan function bodies byte-by-byte for ASCII `Type::method` pairs and append them to the embedding text as ` · API: ...`. `is_static_method_ident` filters out `std::fs::read_to_string`-style module paths by requiring the type name to start with an uppercase letter, so the hint stays high-precision. Gated by `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` (default OFF) and orthogonal to Phase 2b — both env gates may be stacked. A/B measurement on the fixed 89-query dataset: solo Phase 2c lifts NL hybrid Acc@3 by **+1.8 percentage points** but MRR deltas stay at noise (−0.003 hybrid); stacked with Phase 2b it **restores hybrid MRR to baseline** (0.572, ±0.000) while preserving the Phase 2b Acc@3 uplift. Full four-arm experiment log in [`docs/benchmarks.md` §8.3](docs/benchmarks.md).
- **Sparse term coverage re-ranker (Phase 2e, opt-in)** — new `sparse_coverage_bonus_from_fields` + `sparse_query_tokens` + `has_whole_word` helpers in `crates/codelens-engine/src/symbols/scoring.rs`, exposed through `codelens_engine::{sparse_weighting_enabled, sparse_coverage_bonus_from_fields, sparse_threshold, sparse_max_bonus}`. The MCP `get_ranked_context` tool post-processes each result entry with `sparse_coverage_bonus_from_fields` on the **original** user query (not the MCP-expanded retrieval string — the expansion dilutes token counts and collapsed the first pilot to zero effect, see §8.4 experiment log) and adds a whole-word coverage bonus to `relevance_score`, then re-sorts. Gated by `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` (default OFF); tuning knobs `CODELENS_RANK_SPARSE_THRESHOLD` (10..=90, default 60) and `CODELENS_RANK_SPARSE_MAX` (5..=50, default 20). Short-circuits for queries with fewer than 2 discriminative tokens after stopword filtering, so identifier queries are untouched. A/B measurement on the fixed 89-query dataset (threshold 40, max 40): solo Phase 2e lifts hybrid MRR **+0.007**, hybrid Acc@3 **+0.034**, NL Acc@3 **+5.5 percentage points** — the first solo arm in the v1.5 Phase 2 family with a positive delta on every hybrid metric. Stacked with Phase 2b+2c: hybrid MRR **+0.014** (0.572 → 0.586, biggest v1.5 lift so far), NL Acc@3 **+5.5pp**, identifier Acc@1 unchanged at 100%. Phase 2e marginal value on top of Phase 2b+2c: **+0.013 hybrid MRR, +0.036 NL Acc@3**. Full four-arm experiment log in [`docs/benchmarks.md` §8.4](docs/benchmarks.md).
- **Dataset path fix** — `benchmarks/embedding-quality-dataset-self.json` rewritten from `crates/codelens-core/...` to `crates/codelens-engine/...` so `expected_file_suffix` actually matches real files after the v1.4.0 crate rename. Without this fix every NL query scored `rank=None` on current main.

### Measured (Phase 2f — cross-dataset validation, no behaviour change)

- **v1.5 Phase 2b/2c/2e replayed on the 436-query augmented dataset** (2026-04-12). The same four-arm A/B that ran on the 89-query self dataset in §8.2–§8.4 was re-run against `benchmarks/embedding-quality-dataset.json` (~5× more queries, much wider NL phrasing spread) using the release binary from `9f93ef9` and Phase 2e parameters `CODELENS_RANK_SPARSE_THRESHOLD=40` / `CODELENS_RANK_SPARSE_MAX=40`. Every metric moved in the same direction as the 89-query pilot:

  | Arm (stacked vs baseline) | 89-query Δ absolute | 89-query Δ relative | 436-query Δ absolute | 436-query Δ relative |
  | ------------------------- | ------------------: | ------------------: | -------------------: | -------------------: |
  | hybrid MRR                |              +0.014 |          **+2.4 %** |              +0.0034 |           **+7.1 %** |
  | hybrid Acc@3              |              +0.045 |              +7.4 % |              +0.0069 |              +13.7 % |
  | NL hybrid MRR             |              +0.020 |              +4.3 % |              +0.0050 |              +13.3 % |
  | NL Acc@3                  |              +0.055 |             +11.2 % |              +0.0100 |              +24.9 % |
  | identifier Acc@1          |               0.000 |                   0 |                0.000 |                    0 |

  On a **relative** scale the stack is more effective on the harder dataset — Phase 2b (NL tokens) and Phase 2e (coverage bonus) are built to rescue exactly the cohort where the baseline ranks the target below Acc@3, and that cohort dominates on 436 while being a small minority on 89. Phase 2e's marginal value on top of Phase 2b+2c on the 436 set is **+0.0025 hybrid MRR, +0.0036 NL MRR, +0.0067 NL Acc@3** — direction-consistent with the §8.4 numbers. No regression appears anywhere; identifier Acc@1 stays pinned at the baseline of 0.096 across all four arms (436's identifier baseline is much lower than 89's 0.800 because the augmented dataset contains many identifier queries whose target symbol is short enough to collide with the lexical path, which is orthogonal to Phase 2e's short-circuit gate). Full experiment log in [`docs/benchmarks.md` §8.5](docs/benchmarks.md). The stack is now considered safe to opt into on any project whose traffic is NL-heavy, but defaults stay OFF until a **true** external-repo A/B (different codebase, hand-built 20–40 query dataset) is performed.

### Measured (Phase 2g — Phase 2e parameter sweep, no behaviour change)

- **3×3 sweep of `CODELENS_RANK_SPARSE_THRESHOLD` × `CODELENS_RANK_SPARSE_MAX`** on the 89-query self dataset (Phase 2e solo, 2b/2c disabled so the re-ranker's own loss surface is isolated). Nine grid cells + one baseline, same release binary from `ebb5115`. Result: a clean **four-cell plateau** at `(threshold ∈ {30, 40}) × (max ∈ {40, 50})` — every cell in that box hits identical `hybrid MRR = 0.5787`, `hybrid Acc@3 = 0.640`, `NL Acc@3 = 0.545`. `threshold = 50` cliffs down (hybrid MRR 0.5735–0.5746, NL Acc@3 collapses to baseline in two cells); `(threshold = 30, max = 30)` is on the plateau for NL Acc@3 but loses a hair (−0.0003 MRR) for hybrid. Identifier Acc@1 stays at 0.800 in **every** cell — the sub-2-token short-circuit holds at the full parameter range. A stacked verification run at `(threshold = 30, max = 40)` reproduced the §8.4 `(40, 40)` stacked numbers within 0.0004 MRR on every metric, confirming the plateau applies to the stacked regime too. **Verdict**: `(threshold = 40, max = 40)` is the data-backed optimum and the §8.5 recommendation holds unchanged — it is the minimal-aggressive point inside the plateau. Safe tuning zone is `threshold ∈ [30, 40]` × `max ∈ [40, 50]`; anything at threshold 50 trades NL accuracy for nothing. Full sweep + heat maps in [`docs/benchmarks.md` §8.6](docs/benchmarks.md).

### Changed

- **`extract_body_hint` refactor** — now goes through `join_hint_lines` and respects the runtime budgets above. Behaviour at default budgets is unchanged: still returns a single meaningful body line truncated at 60 chars. Future experiments can crank the budgets via env without a rebuild.

### Measured (no behaviour change — evidence log)

- **v1.5 Phase 2 "cAST PoC" reverted** based on A/B measurement on the fixed dataset (2026-04-11):

  | Method                        | HINT_LINES=1 | HINT_LINES=3 |          Δ |
  | ----------------------------- | -----------: | -----------: | ---------: |
  | `get_ranked_context` (hybrid) |        0.573 |        0.568 |     −0.005 |
  | **NL hybrid MRR**             |    **0.472** |    **0.464** | **−0.008** |
  | NL `semantic_search`          |        0.422 |        0.381 |     −0.041 |
  | identifier (hybrid)           |        0.800 |        0.800 |          0 |

  Hypothesis: "more body text lines → higher NL recall". **Rejected** — the bundled CodeSearchNet-INT8 is signature-optimised and extra body tokens dilute signal for natural-language queries. Full experiment log, reproduce commands, and follow-up candidates in [`docs/benchmarks.md` §8.1](docs/benchmarks.md).

- **v1.5 baseline for all future v1.5.x measurements** is **`get_ranked_context` hybrid MRR = 0.573** on the fixed 89-query self-matching dataset. The `0.664` number in earlier memos is from the pre-rename dataset and is no longer apples-to-apples — see the §8 footnote in `docs/benchmarks.md`.

### Rationale

v1.5 is an **NL-retrieval quality** release, not a feature release. Every new env knob is opt-in by design: the underlying embedding model (bundled CodeSearchNet-INT8) was chosen in v1.4 for its install footprint, and v1.5 treats that choice as fixed while improving what can be improved on top — the text the model sees at indexing time (Phase 2b NL tokens, Phase 2c `Type::method` hints) and the way the final results are re-ordered (Phase 2e sparse coverage bonus). Because each gate is OFF by default, upgrading v1.4.0 → v1.5.0 is a zero-behaviour-change drop-in. Users who want the uplift flip the three env vars at launch and pay one index rebuild; the stacked config is cross-dataset validated on both the 89-query self set (+2.4 % hybrid MRR, +11.2 % NL Acc@3 relative) and the 436-query augmented set (+7.1 % hybrid MRR, +24.9 % NL Acc@3 relative). The Phase 2g sweep locked in `(threshold = 40, max = 40)` as the minimal-aggressive optimum inside a four-cell plateau, so the recommended configuration is grounded in measurement rather than a first guess. The entire v1.5 iteration — Phase 1 refactor, rejected Phase 2 cAST PoC, revived Phase 2b NL-token extractor, orthogonal Phase 2c API-call extractor, MCP-layer Phase 2e sparse re-ranker, Phase 2f cross-dataset validation, Phase 2g parameter sweep — is bisectable PR-by-PR in the GitHub history (#10–#17) and reproducible via the measurement artefacts checked into `benchmarks/embedding-quality-v1.5-*.{json,md}`.

## [1.4.0] — 2026-04-11

First public release cut. This version marks the transition from a
"more tools" MCP into a **bounded-answer, telemetry-aware, reviewer-ready**
code-intelligence server.

### Added

- **Telemetry persistence** — new append-only JSONL log at
  `.codelens/telemetry/tool_usage.jsonl`. Gated by
  `CODELENS_TELEMETRY_ENABLED=1` or `CODELENS_TELEMETRY_PATH=<path>`.
  Disabled by default. Graceful degradation: write failures are logged
  once and swallowed — telemetry never breaks dispatch.
- **`mermaid_module_graph` workflow tool** — renders upstream/downstream
  module dependencies as a Mermaid flowchart, ready to paste into
  GitHub/GitLab/VS Code Markdown. Reuses `get_impact_analysis` data;
  no new engine surface.
- **Reproducible public benchmarks doc** (`docs/benchmarks.md`) — every
  headline performance number is now backed by an executable script
  under `benchmarks/` and can be re-run on any machine. Includes
  token-efficiency (tiktoken cl100k_base), MRR/Accuracy@k, and per-
  operation latency.
- **Output schemas**: expanded from 31 → 45 of 89 tools (51% coverage),
  including 7 new schemas for mutation + semantic tools.
- **MCP v2.1.91+ compliance**:
  - `_meta["anthropic/maxResultSizeChars"]` response annotation
  - Deferred tool loading during `initialize`
  - Schema pre-validation (fail fast on missing required params)
  - Rapid-burst doom-loop detection (3+ identical calls within 10s →
    `start_analysis_job` suggestion)
- **Harness phase tracking** — telemetry timeline now records an
  optional `phase` field (plan/build/review/eval) per invocation.
- **Effort level** — `CODELENS_EFFORT_LEVEL=low|medium|high` adjusts
  adaptive compression thresholds and default token budget.
- **Self-healing SQLite indexes** — corrupted FTS5 / vec indexes are
  detected on open and rebuilt automatically without user intervention.
- **Project-scoped memory store** — `list_memories`, `read_memory`,
  `write_memory`, `delete_memory`, `rename_memory` tools for persistent
  architecture notes, RCA history, and kaizen logs.

### Changed

- **Crate rename**: `codelens-core` → `codelens-engine` to resolve a
  crates.io name collision. Workspace consumers should update their
  `Cargo.toml` dependency from `codelens-core` to `codelens-engine`.
  Binary name (`codelens-mcp`) unchanged.
- **Architecture docs** (`docs/architecture.md`) resynced from stale
  63-tool / 22K-LOC / 197-test snapshot to current
  90-tool / 46K-LOC / 547-test ground truth.
- **Tool surface**: 89 → 90 tools (FULL preset). BALANCED auto-includes
  new tools via the exclude-list pattern; MINIMAL intentionally stays
  at 20.

### Fixed

- **Clippy cleanup**: resolved 28 accumulated warnings across default
  and `http` features. `cargo clippy --all-targets -- -D warnings`
  is now clean on both feature sets.
- **Rename lookup fallback** hardened for LSP-absent flows.
- **Analysis state scope**: analysis queue state now scoped to
  session project — prevents cross-project contamination on HTTP
  transport.
- **HTTP session runtime state** isolated per session.

### Removed

- No public API removals.

### Migration notes

1. If your `Cargo.toml` depends on `codelens-core`, update it to
   `codelens-engine`. No API signatures changed — only the package name.
2. Binary name (`codelens-mcp`) and CLI surface are unchanged.
3. To opt into telemetry persistence, set
   `CODELENS_TELEMETRY_ENABLED=1` when launching the server and grep
   `.codelens/telemetry/tool_usage.jsonl` afterwards.
4. Mermaid diagrams produced by `mermaid_module_graph` embed directly
   in GitHub-flavored Markdown — no extra renderer needed.

### Metrics snapshot

Measured on this repository at the 1.4.0 cut:

| Metric                                 | Value                      |
| -------------------------------------- | -------------------------- |
| Tools (FULL / BALANCED / MINIMAL)      | 90 / 55 / 20               |
| Rust source files                      | 115                        |
| LOC (prod + test)                      | 46K (38.8K + 7.2K)         |
| Tests                                  | 547 (222 engine + 325 mcp) |
| Clippy warnings                        | 0 (default + http feature) |
| Token efficiency vs Read/Grep          | **6.1x (84%)**             |
| Workflow profile compression           | 15-16x (planner/reviewer)  |
| Hybrid retrieval MRR                   | **0.664** (self-dataset)   |
| Hybrid retrieval Acc@5                 | **0.775**                  |
| `find_symbol` / `get_symbols_overview` | < 1 ms                     |
| Cold start                             | ~ 12 ms                    |

See [`docs/benchmarks.md`](docs/benchmarks.md) for reproduce commands.

---

## Earlier history

Pre-1.4.0 work lives in git history on the `main` branch. Notable
milestones:

- **2026-03-28** — `feat: unified project & backend integration` (PR #1),
  `feat: Pure Rust MCP server — 54 tools, 15 languages, semantic search,
token budget` (PR #2)
- **2026-04-04** — `refactor: state.rs -33%, full green, Store
extraction` (PR #3)
- **2026-04-08** — `feat: semantic code review, structural search
boosting, cross-phase context` (PR #4)
- **2026-04-09** — `feat: essential main integration: rename, session
scope, report runtime, clean-clone tests` (PR #5),
  `feat: track MCP recommendation outcomes in Codex harness` (PR #6)
- **2026-04-11** — PR #7 (harness compliance + crate rename + telemetry
  persistence), PR #8 (benchmarks doc + mermaid_module_graph) → 1.4.0 cut

[Unreleased]: https://github.com/mupozg823/codelens-mcp-plugin/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.4.0
