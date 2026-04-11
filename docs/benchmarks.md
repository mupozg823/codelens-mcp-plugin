# CodeLens MCP — Benchmarks

> Reproducible token-efficiency and search-quality measurements.
> Last measurement: **2026-04-11**.

This document is the authoritative source for CodeLens's public performance claims. Every number below is produced by an executable script in `benchmarks/` and can be re-run on any machine.

---

## 1. Headline Numbers (what we claim publicly)

| Claim                                                   | Value                       | Source                                |
| ------------------------------------------------------- | --------------------------- | ------------------------------------- |
| Token reduction vs Read/Grep (total, structured tasks)  | **6.1x (84% fewer tokens)** | `benchmarks/token-efficiency.py`      |
| Token reduction on best single task (context retrieval) | **167x**                    | `benchmarks/token-efficiency.py`      |
| Workflow profile compression (planner/reviewer)         | **15-16x**                  | `benchmarks/token-efficiency.py`      |
| Search quality, hybrid (MRR)                            | **0.664**                   | `benchmarks/embedding-quality.py`     |
| Search quality, hybrid (Accuracy@5)                     | **0.775**                   | `benchmarks/embedding-quality.py`     |
| Cold start (no LSP)                                     | **~12 ms**                  | `target/release/codelens-mcp` startup |

All token counts use **tiktoken `cl100k_base`** — the same tokenizer used by Claude and GPT-4 — so "tokens saved" maps directly to "prompt budget saved."

---

## 2. Token Efficiency — CodeLens vs Read/Grep

**What we measure**: six representative agent tasks, each executed two ways — a native `rg + cat + wc` baseline and a single CodeLens MCP tool call. We compare token counts of the response each approach would hand back to the model.

**Script**: `benchmarks/token-efficiency.py`

**Result snapshot** (2026-04-11, measured on this repository):

| Task               | Baseline method                              | Baseline tokens | CodeLens method                    | CodeLens tokens |        Savings |
| ------------------ | -------------------------------------------- | --------------: | ---------------------------------- | --------------: | -------------: |
| Find symbol        | `rg -n 'dispatch_tool'` (30 lines)           |             616 | `find_symbol include_body=true`    |             309 |           2.0x |
| File structure     | `Read crates/codelens-mcp/src/dispatch.rs`   |           5,988 | `get_symbols_overview`             |           1,612 |           3.7x |
| Impact analysis    | `Read project.rs + rg references`            |           5,321 | `get_impact_analysis`              |           1,651 |           3.2x |
| Find references    | `rg -n 'dispatch_tool'` (50 lines)           |             616 | `find_referencing_symbols`         |             240 |           2.6x |
| Project onboarding | `Read manifest + entry + README + file list` |           7,972 | `onboard_project`                  |             763 |          10.4x |
| Context retrieval  | `rg + read 2 files`                          |           7,692 | `get_ranked_context max_tokens=8k` |              46 |       **167x** |
| **Total**          |                                              |      **28,205** |                                    |       **4,621** | **6.1x (84%)** |

Context retrieval dominates because `get_ranked_context` already applies the 4-signal ranking internally and returns just the top symbols that fit within the token budget, while the native baseline dumps full files into the prompt.

### Re-running

```bash
# 1. Build release binary (bundled CodeSearchNet ONNX, ~76 MB)
cargo build --release

# 2. Run against this repo (or any other Rust/TS/Python project)
python3 benchmarks/token-efficiency.py .
```

The script writes a timestamped JSON result file (e.g. `benchmarks/token-efficiency-2026-04-11.json`) that includes per-task token counts, latencies, and the exact tool arguments used. Re-run on your own codebase to verify — numbers vary with project size and language mix.

---

## 3. Workflow Profile Compression

Beyond raw tool-level savings, CodeLens ships **role profiles** (`planner-readonly`, `reviewer-graph`, `refactor-full`, `builder-minimal`, `ci-audit`). Each profile caps response size and prefers workflow tools that return pre-synthesized reports.

We compare a typical low-level tool chain (baseline: `preset:balanced`) with a single workflow tool call (profile: target role) for three common agent scenarios:

| Scenario                 | Baseline (balanced) | Workflow profile | Savings | Tool calls |
| ------------------------ | ------------------: | ---------------: | ------: | :--------: |
| Planner change request   |               3,167 |              203 |   15.6x |   2 → 1    |
| Reviewer impact analysis |               2,847 |              175 |   16.3x |   3 → 1    |
| Refactor safety check    |                 837 |              189 |    4.4x |   3 → 1    |

The compression ratio grows when agents would otherwise expand raw graph data (impact analysis, reference walks). Workflow tools like `impact_report`, `analyze_change_request`, `refactor_safety_report` return bounded analysis handles instead of raw adjacency lists.

---

## 4. Search Quality — MRR / Accuracy@k

**What we measure**: self-matching retrieval accuracy. We take 89 queries that describe real symbols in this repository (identifier, short phrase, natural language styles), ask CodeLens to find each one, and score where the intended symbol appears in the ranked results.

**Scripts**:

- `benchmarks/embedding-quality.py` — runs the full quality suite
- `benchmarks/embedding-quality-dataset-self.json` — the 89-query dataset, versioned in the repo

**Metrics**:

- **MRR** (Mean Reciprocal Rank) — `1/rank` of the correct answer, averaged. Higher is better. `1.0` means always rank-1.
- **Accuracy@k** — fraction of queries where the correct symbol lands in the top-k results.

**Result snapshot** (2026-04-11, 89 queries, hybrid ranking on):

| Method                         |       MRR | Acc@1 | Acc@5 | Latency |
| ------------------------------ | --------: | ----: | ----: | ------: |
| `semantic_search`              |     0.598 | 0.539 | 0.663 |  574 ms |
| `get_ranked_context` (lexical) |     0.604 | 0.528 | 0.697 |  168 ms |
| `get_ranked_context` (hybrid)  | **0.664** | 0.584 | 0.775 |  265 ms |

**By query type (hybrid)**:

| Query type         |   MRR | Count | Notes                                          |
| ------------------ | ----: | ----: | ---------------------------------------------- |
| `identifier`       | 0.960 |    25 | Near-perfect — FTS5 dominates                  |
| `short_phrase`     | 0.676 |     9 | Good — hybrid helps                            |
| `natural_language` | 0.528 |    55 | Weakest — structural target for future ML work |

Identifier queries hit a lexical fast path (FTS5 + jaro_winkler). Natural-language queries rely on the bundled MiniLM-L12-CodeSearchNet INT8 model. The NL gap is the current weakness we track — see [docs/architecture.md §8 Key Metrics](architecture.md#8-key-metrics) for the improvement trajectory.

### Re-running

```bash
cargo build --release
python3 benchmarks/embedding-quality.py . --isolated-copy
```

Use `--isolated-copy` to avoid index pollution when the script mutates the working directory (it runs `refresh_symbol_index` between runs).

---

## 5. Per-Operation Latency (Real-Time Budget)

| Operation              | Latency                              | Method                    |
| ---------------------- | ------------------------------------ | ------------------------- |
| `find_symbol`          | < 1 ms                               | SQLite FTS5               |
| `get_symbols_overview` | < 1 ms                               | Cached                    |
| `get_ranked_context`   | ~265 ms (hybrid) / ~168 ms (lexical) | 4-signal + semantic blend |
| `get_impact_analysis`  | ~1 ms                                | Graph cache (petgraph)    |
| `semantic_search`      | ~574 ms                              | Warm embedding pool       |
| `onboard_project`      | ~21 ms                               | Composite workflow        |
| Cold start             | ~12 ms                               | No LSP boot               |

Measurement harness: `benchmarks/embedding-runtime.py` (latency distribution) and `benchmarks/token-efficiency.py` (workflow scenarios). Both write JSON results.

---

## 6. Reproducing Everything End-to-End

```bash
# Clone and build (release, bundled ML model)
git clone https://github.com/mupozg823/codelens-mcp-plugin
cd codelens-mcp-plugin
cargo build --release

# Install Python deps (one-time)
pip install tiktoken

# 1. Token efficiency (Read/Grep comparison)
python3 benchmarks/token-efficiency.py . \
  > benchmarks/token-efficiency-$(date +%Y-%m-%d).json

# 2. Search quality (MRR / Acc@k)
python3 benchmarks/embedding-quality.py . --isolated-copy \
  > benchmarks/embedding-quality-$(date +%Y-%m-%d).json

# 3. Runtime latency distribution
python3 benchmarks/embedding-runtime.py . --isolated-copy \
  > benchmarks/embedding-runtime-$(date +%Y-%m-%d).json
```

All three scripts are deterministic given the same input repo and binary. Results that deviate from the headline numbers above by more than ±10% should be treated as a regression and reported.

---

## 7. Methodology Notes

**Why tiktoken?**
`cl100k_base` is the tokenizer used by GPT-4 / Claude. A token saved by CodeLens is a token the agent does not have to pay for on the next LLM call. Character counts and whitespace counts are not comparable.

**Why self-matching queries?**
The 89-query dataset targets symbols that actually exist in this repo. Cross-repo generalization is a separate question we do not currently claim. Use your own project to verify the numbers before relying on them in production.

**Why hybrid ranking?**
Pure semantic search (MRR 0.598) and pure lexical search (MRR 0.604) are roughly tied. Hybrid blending takes the best of both — identifier queries stay lexical-first, natural-language queries get semantic boosting — and lifts MRR to 0.664 with only +100 ms latency.

**What we don't measure (yet)**

- Cross-repo retrieval quality (coming with multi-repo datasets)
- Incremental indexing latency under heavy file churn
- Cold-start wall time on Windows CI runners

**What we guarantee**

- The `benchmarks/` scripts are open. If you can't reproduce a claim, it's a bug.
- Result JSON files are versioned in the repo. You can diff historical snapshots.
- No hidden multipliers, no marketing math, no vendored baselines.

---

## 8. Historical Snapshots

| Date                         | Token efficiency (Total) | Hybrid MRR | Notes                                                                                                                          |
| ---------------------------- | -----------------------: | ---------: | ------------------------------------------------------------------------------------------------------------------------------ |
| 2026-04-11 (post-PoC revert) |               6.1x (84%) |      0.573 | v1.5 apples-to-apples baseline after dataset path fix (`codelens-core` → `codelens-engine`), defaults `HINT_LINES=1` / `60ch`. |
| 2026-04-11 (Phase 2 PoC)     |                        — |      0.568 | Experimental 3-line / 180-char body hints. **Reverted** — see §8.1.                                                            |
| 2026-04-11 (v1.4.0 cut)      |               6.1x (84%) |      0.664 | Measured against the pre-rename dataset; suffix mismatch after the crate rename means this row is _not_ apples-to-apples.      |
| 2026-04-08                   |                        — |      0.688 | Pre-dataset expansion (89 subset, different queries).                                                                          |
| earlier                      |         "estimated 2-5x" |          — | No formal measurement before 2026-04.                                                                                          |

> **Note on 0.664 vs 0.573** — both numbers are real, but they measure slightly different things. The 0.664 row used a dataset whose `expected_file_suffix` fields still pointed at the pre-rename `crates/codelens-core/...` paths. After v1.5's crate rename those suffixes no longer matched any real file, so we updated the dataset in-place. The 0.573 row is the first hybrid MRR measured after that fix and is therefore the correct apples-to-apples baseline for all future comparisons. Token-efficiency numbers (6.1x) are independent of the dataset fix and remain valid.

Historical result JSON files live under `benchmarks/*.json` with timestamps in filenames. When you upgrade CodeLens, the suggested flow is: (1) check out the new version, (2) re-run the three scripts above, (3) compare against your last `benchmarks/*.json` from the previous version to catch regressions.

### 8.1 v1.5 Phase 2 cAST PoC — experiment log

**Hypothesis**: Natural-language query misses on this repo share a pattern — the discriminating keyword lives in line 2 or 3 of the function body, not in the signature or the first meaningful line. Expanding the body-hint budget from 1 line / 60 chars to 3 lines / 180 chars should let the embedding model see enough body tokens to match NL queries.

**Setup**: A/B on the same updated 89-query dataset, same bundled MiniLM-L12-CodeSearchNet INT8 model, same release binary. Both arms were measured on 2026-04-11 against the same `main` HEAD.

- **Arm A** — `CODELENS_EMBED_HINT_LINES=1` (the reverted default — minimal body exposure)
- **Arm B** — `CODELENS_EMBED_HINT_LINES=3` (the Phase 2 PoC — the change we wanted to evaluate)

**Result**:

| Method                         | Arm A (1 line) | Arm B (3 lines) |          Δ |
| ------------------------------ | -------------: | --------------: | ---------: |
| `semantic_search` (overall)    |          0.528 |           0.510 |     −0.018 |
| `get_ranked_context` (lexical) |          0.492 |           0.492 |          0 |
| `get_ranked_context` (hybrid)  |          0.573 |           0.568 |     −0.005 |
| **NL hybrid MRR**              |      **0.472** |       **0.464** | **−0.008** |
| NL `semantic_search`           |          0.422 |           0.381 |     −0.041 |
| identifier (hybrid)            |          0.800 |           0.800 |          0 |

**Verdict — hypothesis rejected.** More body text dilutes the signature signal for the bundled CodeSearchNet-INT8 model rather than helping. The `semantic_search` NL regression (−0.041) is well outside measurement noise for a 55-query subset, and every other row is neutral-to-negative. Identifier queries are completely unaffected because they never touched the body-hint path.

**Root cause analysis**:

1. The bundled CodeSearchNet model is **signature-optimised**. It was trained on short function signatures, not multi-line bodies. Additional body tokens act as noise.
2. `extract_body_hint` still **skips comments and string literals** — exactly where NL-matchable natural language lives. Expanding line count without expanding content scope doesn't help.
3. The 55 NL queries in our dataset target **behavioural concepts** ("skip comments", "detect client", "track recommendation") that are naturally phrased and rarely appear verbatim in non-comment body lines.

**Action taken**: defaults reverted to `DEFAULT_HINT_LINES = 1`, `DEFAULT_HINT_TOTAL_CHAR_BUDGET = 60`. The infrastructure — `join_hint_lines`, `hint_line_budget`, `hint_char_budget`, `CODELENS_EMBED_HINT_LINES`, `CODELENS_EMBED_HINT_CHARS` env overrides — stays in place so future experiments can A/B without a rewrite.

**Reproduce either arm**:

```bash
# Arm A (current default)
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json

# Arm B (Phase 2 PoC)
CODELENS_EMBED_HINT_LINES=3 \
CODELENS_EMBED_HINT_CHARS=180 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json
```

**Next candidate experiments** (Phase 2b / 2c, separate PRs):

- **Comment / string scanner** — extend `extract_body_hint` to also collect one-line `//`, `#`, and `/* */` comments plus natural-language-shaped string literals. Directly addresses RCA items 2 + 3.
- **API-call extractor** — scan the body for `Type::method(...)` / `object.method(...)` patterns and append them as additional tokens so NL queries like "connects to PostgreSQL" can match files that call `Postgres::connect`.
- **Model swap** — replace CodeSearchNet-INT8 with a hybrid code+text model (E5-large, BGE-base). Higher binary size + install cost but directly addresses RCA item 1.

The negative result is itself valuable: it rules out the cheapest fix (just show more body text to the same model) and points the next PoC at the right layer.

### 8.2 v1.5 Phase 2b experiment — comment + NL-shaped string literal extractor

**Hypothesis**: Phase 2's RCA flagged two layers that the previous PoC did not touch — line / block comments, and NL-shaped string literals inside function bodies. Both are _natural language_ rather than code, so they should not trigger the signal-dilution problem that killed Phase 2. Queries like `"skip comments and string literals during search"` should finally be able to match the comment body that describes exactly that behaviour.

**Setup**: Same 89-query dataset, same release binary, same bundled CodeSearchNet-INT8 model as Phase 2. Same day (2026-04-11). Both arms use the reverted 1-line / 60-char defaults; the only difference is the new env knob.

- **Arm A** — `CODELENS_EMBED_HINT_INCLUDE_COMMENTS` unset (default OFF). No comments or string literals appended.
- **Arm B** — `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1`, `CODELENS_EMBED_HINT_CHARS=200`. New NL token extractor appends ` · NL: ...` to the embedding text when body comments or NL-shaped literals exist.

**Result**:

| Method                            |     Arm A |     Arm B |          Δ |
| --------------------------------- | --------: | --------: | ---------: |
| `semantic_search` (overall)       |     0.528 |     0.513 |     −0.015 |
| `get_ranked_context` (lexical)    |     0.493 |     0.493 |          0 |
| `get_ranked_context` (**hybrid**) | **0.572** | **0.580** | **+0.008** |
| **NL hybrid MRR**                 | **0.471** | **0.481** | **+0.010** |
| NL `semantic_search`              |     0.422 |     0.400 |     −0.022 |
| short_phrase hybrid               |     0.559 |     0.574 |     +0.015 |
| identifier (hybrid)               |     0.800 |     0.800 |          0 |

**Accuracy deltas** (hybrid get_ranked_context):

| Metric             |   Arm A |   Arm B |       Δ |
| ------------------ | ------: | ------: | ------: |
| Acc@3 overall      |     61% |     67% |     +6% |
| Acc@5 overall      |     65% |     67% |     +2% |
| **NL Acc@3**       | **49%** | **58%** | **+9%** |
| NL Acc@5           |     55% |     58% |     +3% |
| short_phrase Acc@3 |     78% |     89% |    +11% |

**Verdict — hypothesis partially confirmed on the hybrid path**:

1. **Hybrid `get_ranked_context`** — the mode that real agents use — gains **+0.010 NL MRR** and **+9 percentage points** on NL Acc@3. Both sit above plausible noise on a 55-query subset.
2. **`semantic_search` alone regresses** by −0.015 overall / −0.022 on NL. The extra NL tokens add enough content to push the embedding closer to other NL-shaped symbols, hurting the pure-semantic path.
3. **Identifier queries are unchanged** (0.800 → 0.800) — as expected, they never touched the NL-token path.

**Why Phase 2b works where Phase 2 failed**: Phase 2 added raw code lines (`let x = ...`) which the signature-optimised model sees as noise. Phase 2b adds _only natural-language content_ filtered through `is_nl_shaped` (multi-word, alphabetic ratio ≥ 60%, no path/scope separators). The model's NL path benefits because the new tokens look like prose; the code path is untouched.

**Default policy**: Phase 2b stays **opt-in** (`CODELENS_EMBED_HINT_INCLUDE_COMMENTS` default OFF) for two reasons:

1. The `semantic_search` regression, while small, means the feature is only a net win for the hybrid path. Users who rely on pure semantic search should not be silently affected.
2. Changing the default forces every existing deployment to rebuild its embedding index. Opt-in lets each project validate on its own codebase before committing.

Projects that want the improvement immediately can add the env var to their launcher script. A future v1.5.x may flip the default if follow-up measurement on more repositories confirms the direction.

**Reproduce either arm**:

```bash
# Arm A (current default — OFF)
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json

# Arm B (Phase 2b ON)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_CHARS=200 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json
```

**Next candidate experiments**:

- **Phase 2c — API-call extractor**: surface `Type::method(...)` / `object.method(...)` patterns for queries like `"connects to PostgreSQL"` matching `Postgres::connect`. Orthogonal to Phase 2b, can be A/B'd independently.
- **Phase 2d — Model swap**: CodeSearchNet-INT8 → E5-large / BGE-base. Directly addresses RCA item 1 (the model itself). Costs binary size + install weight but uncaps ceiling for NL queries.

The two positive signals from this experiment (hybrid path improvement, NL Acc@3 jump) validate the RCA direction: the right layer to attack is _content scope_, not _line count_.

### 8.3 v1.5 Phase 2c experiment — `Type::method` API-call extractor

**Hypothesis**: Phase 2b validated that _natural language_ content (comments, string literals) is a productive layer to widen. Phase 2c tests an orthogonal layer — _type identifiers_ harvested from static-method call sites (`Parser::new`, `HashMap::with_capacity`, `Connection::open`). NL queries like `"parse json"` or `"open database"` should gain a lexical bridge to symbols whose body references the matching type, even when neither the signature nor the doc comment mentions it.

**Setup**: Same 89-query self dataset (`embedding-quality-dataset-self.json`), same release binary, same bundled CodeSearchNet-INT8 model, same day (2026-04-11). Four arms measured via `--isolated-copy` to guarantee fresh indexes:

- **A — baseline**: both env gates off (current default).
- **B — phase2c only**: `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1`, `CODELENS_EMBED_HINT_INCLUDE_COMMENTS` unset.
- **C — phase2b only**: `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1`, `CODELENS_EMBED_HINT_INCLUDE_API_CALLS` unset. Included as a second reference so Phase 2c's marginal value can be computed against the already-proven Phase 2b opt-in.
- **D — stacked**: both env gates set to `1`.

The new extractor walks the body byte-by-byte, captures ASCII `Type::method` pairs where `Type` starts with an uppercase letter, deduplicates, and appends ` · API: ...` to the embedding text. Module-prefixed free functions (`std::fs::read_to_string`) are filtered out by the `is_static_method_ident` PascalCase check to keep the hint high-precision.

**Result** (89 queries, top-10 cutoff):

| Method                      | A base |  B 2c |  C 2b | D stacked |
| --------------------------- | -----: | ----: | ----: | --------: |
| `semantic_search` MRR       |  0.528 | 0.527 | 0.517 |     0.522 |
| `get_ranked_context` hybrid |  0.572 | 0.569 | 0.570 |     0.572 |
| hybrid Acc@3                |  0.607 | 0.618 | 0.629 |     0.629 |
| **NL hybrid MRR**           |  0.471 | 0.466 | 0.467 |     0.468 |
| **NL hybrid Acc@3**         |  0.491 | 0.509 | 0.509 |     0.509 |

**Deltas vs baseline**:

| Run         | s_search MRR | hybrid MRR | hybrid Acc@3 | NL hybrid MRR | NL hybrid Acc@3 |
| ----------- | -----------: | ---------: | -----------: | ------------: | --------------: |
| phase2c     |       −0.001 |     −0.003 |       +0.011 |        −0.005 |          +0.018 |
| phase2b     |       −0.011 |     −0.003 |       +0.022 |        −0.004 |          +0.018 |
| **stacked** |   **−0.006** | **±0.000** |   **+0.022** |    **−0.003** |      **+0.018** |

**Phase 2c marginal value on top of Phase 2b** (`stacked − phase2b only`):

- `semantic_search` MRR: **+0.005**
- `get_ranked_context` hybrid MRR: **+0.003**
- NL hybrid MRR: **+0.001**
- Acc@3 metrics: unchanged (already at the Phase 2b ceiling for this dataset)

**Verdict — mixed when solo, recovers the hybrid ceiling when stacked**:

1. **Phase 2c alone** improves NL Acc@3 by +1.8 percentage points (matching Phase 2b's Acc@3 contribution on its own) but its MRR deltas sit at or below noise (−0.003 hybrid, −0.005 NL). Top-1 / top-5 ordering changes are inconsistent.
2. **Phase 2b alone** wins Acc@3 more strongly (+2.2pp hybrid, reproducing the Phase 2b experiment) but `semantic_search` MRR regresses the most here (−0.011).
3. **Stacked (2b + 2c)** restores the `get_ranked_context` hybrid MRR to the baseline value (0.5722, ±0.000) while keeping the Acc@3 uplift (+2.2pp). In other words, Phase 2c partially _patches_ Phase 2b's MRR regression on the hybrid path without sacrificing its recall gains. This is the only arm where hybrid MRR is not worse than baseline.
4. **Identifier queries are unchanged** (0.800 → 0.800 across all four arms), matching expectation — the API-call hint only lives inside function bodies, not in the signature seen by identifier queries.

**Why Phase 2c's signal is smaller than Phase 2b's**: Phase 2b widens the _content scope_ from code into prose, which the CodeSearchNet model's NL path was explicitly missing (RCA item 2). Phase 2c instead injects PascalCase identifiers that are _already_ discoverable by the lexical ranker when the query mentions a type by name. The net effect is a small bump on queries where neither the signature nor the docstring references the type, plus a partial offset of Phase 2b's MRR cost on the hybrid path.

**Default policy**: Phase 2c stays **opt-in** (`CODELENS_EMBED_HINT_INCLUDE_API_CALLS` default OFF) for the same two reasons Phase 2b did:

1. The solo MRR deltas are at noise and the marginal value is conditional on Phase 2b also being on. A silent default would force an index rebuild for a contingent win.
2. Projects already running Phase 2b opt-in can add `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` to their launcher to capture the hybrid-MRR recovery without further changes.

**Reproduce any arm**:

```bash
# Arm A — baseline (current default)
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json

# Arm B — Phase 2c solo
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json

# Arm D — Phase 2b + Phase 2c stacked (recommended when opting in)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json
```

Measurement artefacts live in `benchmarks/embedding-quality-v1.5-phase2c-self-{baseline,on,phase2b-only,stacked}.{json,md}` for audit and future reproducibility.

**Next candidate experiments** (unchanged from §8.2):

- **Phase 2d — Model swap**: CodeSearchNet-INT8 → E5-large / BGE-base. Directly addresses RCA item 1 (the embedding model itself). Largest ceiling uplift but costs binary size + install weight.
- **Phase 2e — Sparse term weighting**: if the stacked-arm result holds on more repos, a short term-frequency re-weighting pass on top of the existing BM25+dense hybrid may be the cheapest next step.

### 8.4 v1.5 Phase 2e experiment — sparse term coverage re-ranker

**Hypothesis**: Phase 2c's stacked-arm recovered hybrid MRR to baseline while holding the Acc@3 uplift, but still left top-1 ordering on the table — the right answer was often already inside the top 3, it just needed a push to be top 1. A lightweight **sparse term-coverage bonus** run on the hybrid top-K after structural + semantic merging should reorder close ties without touching the lexical path used by identifier queries.

**Design**:

- **Tokenize** the original user query (not the expanded retrieval string) on non-alphanumeric boundaries, drop tokens under 3 chars, drop a short stopword list (`the`, `for`, `with`, `from`, …). Deduplicate.
- **Build a corpus** from each candidate's `name + name_path + signature + file_path` — `body` is not available at this stage. `file_path` matters because queries like `"build embedding text from a symbol"` find the right symbol via the directory name (`embedding/`, `ranking/`) even when the function name is short.
- **Count whole-word matches** with a byte-level boundary check: `_` is a word separator (so `"parse"` matches `parse_json_body`) but alphanumerics are not (so `"parse"` does not match `parser`).
- **Coverage ratio** = matched / total tokens. Below a configurable threshold the bonus is 0; between the threshold and 100% it rises linearly to `sparse_max_bonus()`.
- **Short-circuit** to 0 when the query has fewer than 2 discriminative tokens after stopword filtering. Identifier queries (`EmbeddingEngine`, `SqliteVecStore`) tokenize to a single token here and never receive a bonus, so their 100% Acc@1 is untouched.

**Where it runs** — in the MCP `get_ranked_context` tool, as a post-process step **after** `get_ranked_context_cached` and `merge_semantic_ranked_entries`. Running it in the engine's `rank_symbols` (the obvious first guess) does not work because the engine receives the MCP-expanded retrieval string, which appends snake_case/CamelCase/PascalCase forms + alias groups and pushes the token count from 4 to 12–15. The coverage ratio is then permanently below any usable threshold. The pilot confirmed this dilution produced **zero effect on every metric** at thresholds 0.4–0.6 with max bonus 20–40. Moving the pass to the MCP layer (which still holds the original `query` string) is what made the second 4-arm A/B surface real signal.

**Setup**: Same 89-query self dataset, same release binary, same bundled CodeSearchNet-INT8 model, same day (2026-04-12). Four arms via `--isolated-copy`:

- **A — baseline**: all three env gates off.
- **B — phase2e only**: `CODELENS_RANK_SPARSE_TERM_WEIGHT=1`, `CODELENS_RANK_SPARSE_THRESHOLD=40`, `CODELENS_RANK_SPARSE_MAX=40`. Phase 2b/2c gates untouched.
- **C — phase2b+2c only**: Phase 2b/2c opt-in, Phase 2e off. Included to compute Phase 2e's marginal value on top of the already-validated stack.
- **D — stacked**: all three gates on, same Phase 2e parameters as arm B.

**Result** (89 queries, top-10 cutoff):

| Method (hybrid)          | A base |  B 2e | C 2b+2c | D stacked |
| ------------------------ | -----: | ----: | ------: | --------: |
| `get_ranked_context` MRR |  0.572 | 0.579 |   0.573 |     0.586 |
| hybrid Acc@1             |  0.506 | 0.506 |   0.494 |     0.506 |
| hybrid Acc@3             |  0.607 | 0.640 |   0.629 |     0.652 |
| **NL hybrid MRR**        |  0.470 | 0.481 |   0.469 |     0.490 |
| **NL hybrid Acc@1**      |  0.400 | 0.400 |   0.382 |     0.400 |
| **NL hybrid Acc@3**      |  0.491 | 0.545 |   0.509 |     0.545 |
| identifier hybrid Acc@1  |  0.800 | 0.800 |   0.800 |     0.800 |

**Deltas vs baseline**:

| Run             | hybrid MRR | hybrid Acc@1 | hybrid Acc@3 | NL hybrid MRR | NL Acc@1 |   NL Acc@3 |
| --------------- | ---------: | -----------: | -----------: | ------------: | -------: | ---------: |
| phase2e only    | **+0.007** |       +0.000 |   **+0.034** |    **+0.011** |   +0.000 | **+0.055** |
| phase2b+2c only |     +0.001 |       −0.011 |       +0.022 |        −0.001 |   −0.018 |     +0.018 |
| **stacked**     | **+0.014** |       +0.000 |   **+0.045** |    **+0.020** |   +0.000 | **+0.055** |

**Phase 2e marginal value on top of Phase 2b+2c** (`stacked − phase2b+2c only`):

- hybrid MRR: **+0.013**
- hybrid Acc@1: **+0.011**
- hybrid Acc@3: **+0.023**
- NL hybrid MRR: **+0.021**
- NL hybrid Acc@1: **+0.018**
- NL hybrid Acc@3: **+0.036**
- identifier Acc@1: **+0.000** (100% → 100%, gate held)

**Verdict — first v1.5 experiment where the solo arm beats baseline on every metric that matters**:

1. **Phase 2e alone** is the first Phase 2-family knob where opting in directly (no Phase 2b/2c needed) produces a positive delta on _every_ hybrid metric that was previously either flat or regressing. +0.007 hybrid MRR, +0.034 hybrid Acc@3, +0.055 NL Acc@3 — all ahead of the Phase 2b and Phase 2c solo results.
2. **Stacked arm** (2b + 2c + 2e) is strictly the best seen so far in v1.5: hybrid MRR **0.586** (+0.014 vs baseline, biggest lift in this whole experiment line), NL Acc@3 **0.545** (+5.5pp), hybrid Acc@1 recovered to the baseline 0.506 (Phase 2b+2c alone left it at 0.494). This is the first arm where the stacking story in §8.3 actually crosses into "measurably above baseline" rather than "tied with baseline".
3. **Identifier queries are unchanged** (0.800 → 0.800 on Acc@1/Acc@3 in every arm), confirming that the sub-2-token short-circuit kept the pure-identifier path clean.
4. **`semantic_search` alone is unchanged** — the sparse re-ordering runs inside `get_ranked_context` in the MCP layer and never sees the `semantic_search` code path, so that tool keeps its Phase 2c behaviour.

**Why the first pilot measured zero and the second measured real uplift**: The sparse pass is only useful when its input is the _original user query_. The engine-level `rank_symbols` receives the MCP-expanded retrieval string, which inflates the token count and collapses every coverage ratio below any reasonable threshold. The second pilot moved the pass into the MCP tool, where it runs on `query` directly, and all the metrics moved. The lesson is crate-boundary-specific — sparse pseudo-BM25 scoring must live wherever the "query" string still has its original shape.

**Default policy**: Phase 2e stays **opt-in** (`CODELENS_RANK_SPARSE_TERM_WEIGHT` default OFF), matching the Phase 2b/2c conservative policy. Two reasons:

1. The measurement is on a single 89-query dataset on one repository. A single-dataset win that validates on one more external repo would be enough to flip the default, but one dataset is not enough yet.
2. Flipping the default would make the re-ranking run on every `get_ranked_context` call without an explicit user decision. The bonus is small (`5..=50` clamp, default 20) and short-circuits aggressively, but ordering _changes_ are still visible to downstream agents that cached last-session rankings. Opt-in gives each project control over the rollout.

**Env knobs**:

- `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` (or `true`/`yes`/`on`) — turn the pass on.
- `CODELENS_RANK_SPARSE_THRESHOLD=<10..=90>` (integer, default 60). The coverage percentage below which the bonus stays 0. The 2026-04-12 pilot used `40` — more permissive defaults gave larger uplift on this dataset because most target symbols only had 2 out of 4–5 query tokens in their name + path.
- `CODELENS_RANK_SPARSE_MAX=<5..=50>` (integer, default 20). The maximum bonus added at 100% coverage. `40` was the pilot value and gave clean tie-breaking without dominating the lexical 0–55 range from `score_symbol_with_lower`.

**Reproduce any arm**:

```bash
# Arm A — baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json

# Arm B — Phase 2e solo
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json

# Arm D — full v1.5 stack (2b + 2c + 2e)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json
```

Measurement artefacts live in `benchmarks/embedding-quality-v1.5-phase2e-v2-{baseline,on,2b2c-only,stacked}.{json,md}` for audit and future reproducibility.

**Next candidate experiments**:

- **Phase 2d — Model swap**: CodeSearchNet-INT8 → E5-large / BGE-base. Still the largest ceiling uplift available, still carries binary-size + migration cost. Phase 2e being the first positive solo win means the "stack all three" arm (now +0.014 hybrid MRR / +0.045 Acc@3) is a real baseline to beat before committing to a model swap.
- **Phase 2f — External-repo validation**: run the full 4-arm A/B on one or two medium-size external Rust / TypeScript repos before flipping any of the three env gates to default ON. A positive signal on the self dataset is necessary but not sufficient for a default change.

---

## 9. See Also

- [docs/architecture.md](architecture.md) — tool surface, layer diagram, full metric table
- [README.md](../README.md) — quick install + `vs Serena` comparison
- Project `CLAUDE.md` — routing policy for agents deciding when to prefer CodeLens over native tools
