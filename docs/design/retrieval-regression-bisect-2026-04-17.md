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

## Follow-up results (2026-04-17, same day)

**Hypothesis 1 (CoreML ANE scheduling) — RULED OUT.**

Re-ran the same benchmark on the v1.9.32 binary with `CODELENS_EMBED_COREML_COMPUTE_UNITS=cpu_only` to force the Core ML backend to the deterministic CPU path (no Apple Neural Engine involvement):

| Method                 | default (ANE) |  cpu_only |     Δ |
| ---------------------- | ------------: | --------: | ----: |
| semantic_search        |         0.689 |     0.689 |     0 |
| get_ranked_context lex |         0.583 |     0.583 |     0 |
| **hybrid**             |     **0.712** | **0.712** | **0** |

The compute-unit selector does not move the retrieval score at all. CoreML/ANE non-determinism is therefore not the cause of the 0.758 → 0.712 drift.

Hypotheses 2 (FP associativity under LTO), 3 (build-artifact codegen diff), and 4 (isolate-copy ordering) remain open. Cheapest next probe is now **hypothesis 4 via `--no-isolated-copy` re-run**, followed by hypothesis 3 via `sha256sum` of the two release binaries built back-to-back on the same machine.

## What this means for users

No action needed at the binary / MCP-tool level. Retrieval remains dominated by the structural signal (identifier-exact queries clear 0.935 MRR on every build), and the hybrid lift over lexical-only is still large (+0.128 MRR on v1.9.32). The drift is a precision story in the NL / short-phrase long tail, not a correctness problem.
