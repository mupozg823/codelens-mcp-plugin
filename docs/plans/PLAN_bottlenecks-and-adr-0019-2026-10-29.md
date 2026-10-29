# Implementation Plan: Test/CI Bottlenecks and ADR-0019 Follow-ups

**Status**: ✅ Complete
**Started**: 2026-10-29
**Last Updated**: 2026-10-29 (P1 rejected; P2–P6 done)

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

Request (owner, 2026-10-29): finish the rest of ADR-0019 to the goal, and find and remove the excessive bottlenecks, including inefficiently structured tests.

### Measured starting point (main `99a8cb4a`)

| Area | Measurement | Method |
|---|---|---|
| CI `test` job | 6.5 min; five nextest runs 45 + 57 + 44 + 46 + 52 s | `gh run view 40841330390` step times |
| Test executions per CI run | 9,664 executions of 2,126 distinct tests | `cargo nextest list --message-format json` per feature set, name union |
| Two-run cover | `--features http,semantic` ∪ `--no-default-features` = all 2,126 (0 missing), 3,806 executions | same lists |
| In-body feature branches in tests | 1 (`integration_tests/workflow/capabilities.rs:110`, `cfg!(semantic)`), both sides covered by the two runs | `rg 'cfg!\(feature'` on test files |
| Duplicate CI steps | `cargo check` ×6 duplicates the `cargo clippy --all-targets` step of the same features; `scip-backend` is a default feature, so its check and clippy repeat the default ones | `.github/workflows/ci.yml` |
| Local daemon-set suite | 2,124 tests, 41.7 s wall, 441 s summed; 200 tests ≥ 1 s hold 328 s (74%) | `cargo nextest run --features http,semantic --status-level pass` |
| Slowest group | `server::http_tests` 96 tests ≥ 1 s, 145 s | same |
| Why | the test constructor `AppState::new` (`state/constructors.rs:79`) takes the startup path with `start_watcher = true`: every test starts and stops an FSEvents watcher; 72% of a sampled test's setup was a libdispatch semaphore wait under `project_runtime.rs:568` | `sample` on a serial `server::http_tests` run |
| analysis-lite (ADR-0019) | winnation cold 15–19 s, warm 4–6 s, 1.8–2.3 GB; the 77 untracked 2.4 MB bundles are nearly all of its 163 MB of JS/TS | `benchmarks/analysis-lite-latency.py`, `footprint` |

### Success Criteria
- [ ] ~~Local daemon-set summed test time drops by ≥ 25%~~ — not met: the measured cause was machine contention, not the suite (P1)
- [x] CI `test` job runs each distinct test at least once with ≤ 2 nextest runs (#475: 9,664 → 3,806 executions, 0 lost; runner time per push 45.2 → 29.9 min)
- [x] analysis-lite on the winnation working tree: cold first call ≤ 5 s and session footprint ≤ 600 MB, with skipped files reported (#476: 18.7 → 3.8 s, 1.82 GB → 0.58–0.64 GB; #477: 7.3 → 4.7 s on the bound path's first call)
- [x] ADR-0019 status updated with what was done and measured (#477)

---

## 🏗️ Architecture Decisions (approaches considered)

| Decision | Options considered | Picked | Why |
|---|---|---|---|
| Test constructor | (a) keep watcher, (b) `AppState::new` without watcher + explicit `new_with_watcher` for watcher tests, (c) global env switch to disable watchers in tests | (b) | explicit per test, no hidden global; watcher tests stay real |
| CI test matrix | (a) keep 5 runs, (b) daemon set + no-default (2 runs), (c) 3 runs adding default | (b) | measured 0 tests lost; each feature is exercised on and off; (c) only adds production-branch combinations no current test distinguishes |
| Duplicate checks | remove `cargo check` steps whose features clippy already builds; drop `scip-backend` variants (default) | remove | clippy `--all-targets` compiles a superset of check |
| Large generated files (ADR-0019 B) | (a) size cap, (b) minified-shape detection, (c) gitignore-only | (a) 1 MiB, env override, reported | the stalling bundles are not minified (max line 1,245 chars); size alone separates them |
| Index-free lite runtime (A), warm path (E) | per ADR-0019 | measure after B | ADR sets the thresholds (cold > 2 s, warm > 1.5 s) |

---

## 🚀 Implementation Phases

### Phase 1: Watcher-free test constructor — ❌ rejected by measurement
- [x] Ran the full daemon-set suite with `AppState::new` not starting the watcher: all 2,124 passed (no test depends on it)
- [x] Measured, alternating, same binaries: libtest CPU 76.0 → 75.1 s (median of 3); process-per-test (nextest-like) for the 60 slowest tests 53.4 → 51.6 s, median per-test saving 0.01 s — noise
- [x] Reverted. The 2.2 s per HTTP test seen under nextest was machine contention (load average 84–170 from other jobs): serially the 122 `server::http_tests` take 10.6 s (87 ms each, 6.4 s CPU); samples are mostly waits, not CPU
- Quality gate: full daemon-set and no-default nextest green; clippy `--all-targets` for both
- Rollback: revert the constructor change

### Phase 2: CI matrix
- [x] Replace five nextest runs with two; remove duplicate `cargo check` steps and `scip-backend` duplicates; keep named gates
- [x] Record the union proof (counts above) in the workflow comment
- [x] macOS: drop the default run (default tests ⊂ http tests, 0 missing)
- [x] Remove `build.yml` (`Build & Verify`): test gate ⊂ CI `test`; release build ⊂ `release-dry-run`; its `cargo bench --bench indexing` (281 s) output was never read (`benchmark.yml` and nightly run it with baselines). No branch protection requires it.
- Quality gate: workflow YAML parses; the PR's own CI run passes
- Rollback: revert the workflow file

### Phase 3: ADR-0019 F — maintenance mode
- [x] Record the policy (ADR status, CLAUDE.md/AGENTS.md line) — #477

### Phase 4: ADR-0019 B — skip large files with a report
- [x] RED: engine test — a project with a > threshold file: the file is not read, `truncation.large_files` lists it, status is partial, the target is never skipped
- [x] GREEN: size check before reading in `DeclarationGraph::build`; `CODELENS_ANALYSIS_LITE_MAX_FILE_BYTES` (0 = no cap)
- [x] Measure winnation cold/warm/footprint; deploy with rollback note — #476 `a1698f2e`: first call 18.7 → 3.8 s, later 1.5–1.9 s, 1.82 GB → 0.6 GB, results identical
- Rollback: env `CODELENS_ANALYSIS_LITE_MAX_FILE_BYTES=0` or revert

### Phase 5: ADR-0019 A/E by threshold
- [x] Measure fresh-project cold call and warm call after Phase 4; implement only what crosses the ADR thresholds
  - A (cold 8.4/8.7 s > 2 s): done in #477 — source-only reads, 3.0/3.4 s, no `.codelens` written, rows identical
  - E (warm 1.1–1.4 s < 1.5 s): not taken

### Phase 6: Close-out
- [x] Audit doc and ADR-0019 updated with measurements

---

## 📝 Notes & Learnings
- `cargo nextest list` with flags in a zsh variable needs `${=flags}`; unsplit flags fail with exit 2.
- Mutation checks on #477: the strengthened refusal tests fail when source-only skips root derivation (3 of 5); removing the source-only `home_binding_guard` call alone fails none, because root derivation refuses `$HOME` first — the guard is a second line.
