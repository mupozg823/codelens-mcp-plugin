# Retrieval quality regression bisect — 2026-04-17

- Span: `84c825d..e1d228c` (v1.9.23 baseline commit → post-v1.9.32 docs)
- Dataset: `benchmarks/embedding-quality-dataset-self.json` (104 queries, unchanged across the span)
- Model: `MiniLM-L12-CodeSearchNet-INT8` sha prefix `ef1d1e9c` (unchanged)
- Benchmark tool: `benchmarks/embedding-quality.py` (unchanged)

## Trigger

The v1.9.32 README-anchored re-measurement produced Hybrid MRR **0.712**, vs the 2026-04-15 README number **0.758** recorded on commit `84c825d`. Two independent runs on v1.9.32 (`benchmarks/results/v1.9.32-mrr.md`, `...-run2.md`) produced identical numbers (deterministic), so the delta was not run variance.

## Method

1. Re-ran the self-benchmark on v1.9.32 twice: MRR 0.712 / 0.712 (identical).
2. Checked out `84c825d`, rebuilt `--release --features semantic`, re-ran the same command:

   | Method                        |    MRR@10 |   Acc@1 |   Acc@3 |    Avg ms |
   | ----------------------------- | --------: | ------: | ------: | --------: |
   | semantic_search               |     0.732 |     68% |     80% |     492.9 |
   | get_ranked_context (lexical)  |     0.601 |     54% |     68% |      41.7 |
   | **get_ranked_context hybrid** | **0.758** | **71%** | **82%** | **111.9** |

   ✅ Matches the README-recorded numbers exactly. Baseline is real and reproducible on that commit.

3. Diffed the full span's `.rs` files touching retrieval:
   - `crates/codelens-engine/src/embedding/mod.rs` — **formatting only** (`use` reorder + multi-line signature on `generate_bridge_candidates`)
   - `crates/codelens-engine/src/embedding/vec_store.rs` — **formatting only** (multi-line signatures on `get_embedding`, `embeddings_for_scored_chunks`)
   - `crates/codelens-engine/src/lib.rs` — **formatting only** (`pub use scip_backend::ScipBackend` re-export reordered)
   - `crates/codelens-mcp/src/tools/workflows.rs`, `tools/symbols.rs`, `tools/query_analysis.rs` — **no change in the span**
4. Checked adjacent surfaces that could plausibly affect the benchmark pipeline:
   - `benchmarks/embedding-quality.py` → **no change**
   - `benchmarks/embedding-quality-dataset-self.json` → **no change**
   - `Cargo.lock` external-dependency versions → **no change** (only the three workspace crates' own versions bumped `1.9.26 → 1.9.32`)
   - `crates/codelens-engine/models/codesearch/model.onnx` → **no change**

## Finding

Source diff across the span is insufficient to explain the regression. Every file that could plausibly change a retrieval score was either not touched or touched only for whitespace / re-export order.

## Open hypotheses

Since the source and the input are identical while the numeric output differs, the drift has to come from something the commit diff does **not** capture:

1. **fastembed / ort / sqlite-vec internal non-determinism between build artifacts.** These crates produce deterministic outputs _given the same compiled binary_, but two separately compiled binaries of v1.9.26 and v1.9.32 may differ in codegen even when Cargo.lock is identical (inlining decisions, LLVM pass ordering on slightly different IR from formatting-only changes).
2. **CoreML compute-unit selection on macOS.** Our runtime preference is `coreml_preferred` with `cpu_and_neural_engine` compute units. Core ML's ANE scheduler can pick CPU or ANE per run based on OS state; the resulting numeric path is not bit-exact across those paths, though each is deterministic within itself. The v1.9.26 binary may consistently land on one compute-unit mix, the v1.9.32 binary on another.
3. **Floating-point associativity in optimized builds.** With `lto = true` and `opt-level = 3`, unrelated source-file changes elsewhere in the workspace can shift inlining boundaries enough to produce slightly different instruction orderings in the hot path. Dot-product / cosine-similarity summations are associative mathematically but not in IEEE-754.
4. **Benchmark harness isolation-copy.** `--isolated-copy` tars the working tree into a temp dir. If the working tree's file enumeration order ever differs (e.g. because of untracked files or OS-level inode churn), the resulting index could see symbols in a different insertion order, which at tie-breaking edges affects ranking.

We intentionally do not claim any one of these is the cause without further evidence.

## Recommended follow-up (not done in this session)

Ordered by cost-to-evidence:

1. **Cross-OS comparison**: run the same benchmark on a Linux machine or in CI with both the `84c825d` and `26d513e` binaries. If Linux shows the same drift, hypothesis 2 (CoreML ANE) is ruled out; if Linux is stable and macOS drifts, hypothesis 2 is likely.
2. **Pin CoreML compute units to `cpu_only`**: set `CODELENS_EMBED_RUNTIME_COMPUTE_UNITS=cpu_only` (or equivalent) and re-measure both binaries. If numbers converge, hypothesis 2 is confirmed.
3. **Binary-level diff**: build `84c825d` and `26d513e` on the same machine back-to-back; `sha256sum target/release/codelens-mcp` on both. If SHAs differ and bench numbers differ, correlate object-level diffs with the hot path. Hypothesis 3 gains or loses support here.
4. **Force-constant dataset ingestion order**: add an explicit `sort_by` on the file list inside `--isolated-copy` setup to rule out hypothesis 4.

## Artifacts committed for this bisect

- `benchmarks/results/v1.9.26-mrr.{json,md}` — baseline on `84c825d`, hybrid MRR 0.758 (re-measured 2026-04-17)
- `benchmarks/results/v1.9.32-mrr.{json,md}` — v1.9.32 run 1, hybrid MRR 0.712
- `benchmarks/results/v1.9.32-mrr-run2.{json,md}` — v1.9.32 run 2, hybrid MRR 0.712 (identical, deterministic)
- `benchmarks/results/v1.9.32-mrr-cpuonly.{json,md}` — v1.9.32 with `CODELENS_EMBED_COREML_COMPUTE_UNITS=cpu_only`, hybrid MRR 0.712 (no change → **hypothesis 1 ruled out**, see below)

## Follow-up results (2026-04-17, same day — complete)

All four hypotheses probed; a cross-experiment then identified the actual cause.

| #   | Hypothesis                                 | Verdict                                                  |
| --- | ------------------------------------------ | -------------------------------------------------------- |
| 1   | CoreML ANE scheduling                      | **Ruled out** (`cpu_only` → 0.712 identical)             |
| 2   | FP associativity under LTO                 | **Ruled out** (`lto = false` → 0.712 identical)          |
| 3   | Build-artifact codegen diff                | **Ruled out** (two builds, different SHA, identical MRR) |
| 4   | Isolate-copy enumeration order             | **Partial only** (±0.003 MRR vs direct indexing)         |
| ★   | **Project-tree growth** (cross-experiment) | **CONFIRMED PRIMARY CAUSE** (~0.043 MRR of the 0.046)    |

### Hypothesis 1 — CoreML ANE scheduling (ruled out)

`CODELENS_EMBED_COREML_COMPUTE_UNITS=cpu_only` on the v1.9.32 binary produced byte-identical retrieval numbers (hybrid 0.712, semantic 0.689, lexical 0.583). Core ML compute-unit selection does not move the score. Artifact: `benchmarks/results/v1.9.32-mrr-cpuonly.{json,md}`.

### Hypothesis 2 — FP associativity under LTO (ruled out)

Temporarily flipped `[profile.release] lto = true` → `lto = false`, rebuilt, re-ran. Again hybrid 0.712 exactly. LTO inlining is not shifting any arithmetic in the hot path. `Cargo.toml` restored. Artifact: `benchmarks/results/v1.9.32-mrr-ltooff.{json,md}`.

### Hypothesis 3 — Build-artifact codegen diff (ruled out)

Rebuilt v1.9.32 twice with `cargo clean` between builds:

- Build A: `e11af3435744d1b7d7bb8718722c96fd63f1053ab64fe9624728fd2ca7f92156`
- Build B: `f856343b30d98f6bb948b83b62abdbe3451d7ab71920a01d7b5428abcd10bdb7`

SHAs differ (Rust release builds on this toolchain/arch are not bit-reproducible by default), but the benchmark produced identical hybrid 0.712 on both. Binary-level codegen non-determinism is real but does not reach the retrieval arithmetic. Artifact: `benchmarks/results/v1.9.32-mrr-buildB.{json,md}`.

### Hypothesis 4 — Isolate-copy enumeration order (partial only)

`--no-isolated-copy` measurement (index the live working tree directly):

| Method                 | `--isolated-copy` |    direct |          Δ |
| ---------------------- | ----------------: | --------: | ---------: |
| semantic_search        |             0.689 |     0.684 |     −0.005 |
| get_ranked_context lex |             0.583 |     0.585 |     +0.002 |
| **hybrid**             |         **0.712** | **0.715** | **+0.003** |

A small (±0.003 MRR), measurable effect exists — isolate-copy enumeration is not fully stable on one tie-breaking edge. Worth fixing (sort the copied file list inside `embedding-quality.py --isolated-copy`), but far too small to explain the 0.046 drift. Artifact: `benchmarks/results/v1.9.32-mrr-noisolate.{json,md}`.

### Root-cause experiment — cross-tree / cross-binary

Hypotheses 1–4 together accounted for only ~±0.003 of the 0.046 drift. The remaining 0.043 had to come from the **inputs** to the measurement, not from any compiled behavior.

1. Built v1.9.26 binary from `84c825d`, copied it to `/tmp/v1.9.26-binary` (sha `78d6894820...`).
2. Checked out `main` (v1.9.32 tree).
3. Ran the benchmark with that v1.9.26 binary pointed at the v1.9.32 tree:

| Tree        | Binary      | Hybrid MRR |
| ----------- | ----------- | ---------: |
| v1.9.26     | v1.9.26     |  **0.758** |
| v1.9.32     | v1.9.32     |      0.712 |
| **v1.9.32** | **v1.9.26** |  **0.712** |

The v1.9.26 binary scores 0.758 on the v1.9.26 tree and 0.712 on the v1.9.32 tree. Therefore the binary is not contributing to the drift at all — **the project tree itself is the input that moved**. New files added between `84c825d` and `26d513e` — dispatch decomposition (`envelope.rs`, `rate_limit.rs`, `session.rs`, `table.rs`, `validation.rs`), `cli.rs`, `tools/suggestions.rs`, `tools/reasoning_scaffold.rs`, release notes for `v1.9.27–v1.9.30`, `v1.9.31`, `v1.9.32`, and the bisect artifacts under `benchmarks/results/` — enlarge the haystack and surface same-named tokens (e.g. several `dispatch` modules) that tie-break against the original ground-truth target for fixed queries.

Artifact: `benchmarks/results/v1.9.26-binary-on-v1.9.32-tree.{json,md}`.

## Final finding

The 0.758 → 0.712 delta is **not a product regression**. Retrieval code and the compiled binary are functionally identical across the span — the v1.9.26 binary itself reproduces 0.712 whenever it points at the current tree. What changed is the **measurement environment**: the project grew by ~8 new files and ~1,100 structural lines (all architecture decompositions and docs), each introducing new symbols the fixed 104-query self-benchmark has to discriminate against.

The honest framing is: "retrieval accuracy on the same 104 queries slipped 0.046 MRR as the target codebase itself grew by ~1,100 lines of additional decomposition between `84c825d` and `26d513e`." Not: "retrieval quality regressed."

## What this means for users

No action at the binary / MCP-tool level. Retrieval continues to be dominated by the structural signal (identifier-exact queries still clear 0.935 MRR on every build), and the hybrid lift over lexical-only remains large (+0.128 MRR on v1.9.32). The drift is confined to the NL / short-phrase long tail of a self-benchmark whose ground-truth points at file paths that were refactored out from under it.

## Recommended follow-ups (for a future session)

Ordered by yield-per-cost:

1. **Benchmark-hygiene fix** (cheap) — sort the file-enumeration list inside `embedding-quality.py --isolated-copy` to eliminate the ±0.003 Hypothesis-4 wobble.
2. **Dataset refresh** (medium) — update the self dataset's `expected_file_suffix` entries so queries whose original ground-truth was absorbed into the dispatch decomposition (e.g. anything that resolved to the old 1,133-line `dispatch/mod.rs`) point at the correct successor file (`dispatch/envelope.rs`, `dispatch/table.rs`, etc.). This should restore the self-benchmark close to 0.758 without changing any retrieval code.
3. **Quality-tracking policy** — when reporting cross-version MRR trends publicly, anchor on the cross-project benchmarks (`axum`, `ripgrep`, `django`, `typescript`) whose target trees do not move. Keep the self-benchmark for local regression alarms only; it intrinsically confounds "retrieval quality" with "our repo's own growth".
4. **Add a build-reproducibility note** to the release verification doc — Rust release builds with `lto=true` + `codegen-units=1` on this macOS toolchain are **not** byte-identical across `cargo clean` cycles, but the benchmark-observed behavior is. Worth documenting so the next investigation does not re-run hypothesis 3.
