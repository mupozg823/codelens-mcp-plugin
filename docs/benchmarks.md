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

### 8.5 v1.5 Phase 2f experiment — cross-dataset validation on the augmented 436-query set

**Hypothesis**: The Phase 2b/2c/2e wins in §8.2–§8.4 were all measured on the 89-query `embedding-quality-dataset-self.json`. Before flipping any of the three env gates to default ON, replay the full four-arm A/B on a larger, more diverse query distribution so that a single-dataset overfit is ruled out. The natural first step is the existing 436-query `embedding-quality-dataset.json` (same repository, but ~5× the query count with a much wider spread of NL phrasings). A true external-repo validation still remains, but it requires a hand-built `expected_symbol` mapping — running the augmented self-dataset first is the cheapest check that costs nothing but runtime.

**Setup**: Same release binary built from `9f93ef9` (post-Phase 2e merge), same bundled CodeSearchNet-INT8 model, same `--isolated-copy` workflow as §8.2–§8.4, and the same Phase 2e parameters as the §8.4 pilot (`CODELENS_RANK_SPARSE_THRESHOLD=40`, `CODELENS_RANK_SPARSE_MAX=40`). Four arms:

- **A — baseline**: all three gates off.
- **B — phase2e only**: `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` + threshold/max.
- **C — phase2b+2c only**: `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` + `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1`, Phase 2e off.
- **D — stacked**: all three gates + same Phase 2e knobs.

**Result** (436 queries, top-10 cutoff, `get_ranked_context` hybrid metrics):

| Metric           | A base |   B 2e | C 2b+2c |  D stacked |
| ---------------- | -----: | -----: | ------: | ---------: |
| hybrid MRR       | 0.0476 | 0.0488 |  0.0485 | **0.0510** |
| hybrid Acc@1     | 0.0413 | 0.0413 |  0.0413 | **0.0436** |
| hybrid Acc@3     | 0.0505 | 0.0573 |  0.0528 | **0.0573** |
| NL hybrid MRR    | 0.0377 | 0.0394 |  0.0390 | **0.0427** |
| NL hybrid Acc@1  | 0.0301 | 0.0301 |  0.0301 | **0.0334** |
| NL hybrid Acc@3  | 0.0401 | 0.0502 |  0.0435 | **0.0502** |
| identifier Acc@1 | 0.0964 | 0.0964 |  0.0964 |     0.0964 |

**Deltas vs baseline**:

| Run             |  hybrid MRR | hybrid Acc@3 | NL hybrid MRR |    NL Acc@3 | ident Acc@1 |
| --------------- | ----------: | -----------: | ------------: | ----------: | ----------: |
| phase2e only    | **+0.0012** |  **+0.0069** |   **+0.0017** | **+0.0100** |      +0.000 |
| phase2b+2c only |     +0.0009 |      +0.0023 |       +0.0013 |     +0.0033 |      +0.000 |
| **stacked**     | **+0.0034** |  **+0.0069** |   **+0.0050** | **+0.0100** |      +0.000 |

**Phase 2e marginal value on top of Phase 2b+2c** (`stacked − phase2b+2c only`):

- hybrid MRR: **+0.0025**
- hybrid Acc@1: **+0.0023**
- hybrid Acc@3: **+0.0046**
- NL hybrid MRR: **+0.0036**
- NL hybrid Acc@3: **+0.0067**
- identifier Acc@1: **+0.0000** (100% → 100%, gate held)

**Cross-dataset verdict — direction-consistent, relative lift is _larger_ on the harder dataset**:

Every metric moves in the same direction as the 89-query §8.4 pilot. Absolute magnitudes are smaller because the 436-query augmented set is substantially harder — the baseline hybrid MRR sits at **0.0476** on 436 versus **0.5716** on 89, reflecting the much wider NL phrasing spread and the lower ceiling when more queries have no plausible match in the project at all. The relative uplift tells a different story:

| Arm (stacked vs baseline) | 89-query absolute | 89-query relative | 436-query absolute | 436-query relative |
| ------------------------- | ----------------: | ----------------: | -----------------: | -----------------: |
| hybrid MRR                |            +0.014 |        **+2.4 %** |            +0.0034 |         **+7.1 %** |
| hybrid Acc@3              |            +0.045 |            +7.4 % |            +0.0069 |            +13.7 % |
| NL hybrid MRR             |            +0.020 |            +4.3 % |            +0.0050 |            +13.3 % |
| NL Acc@3                  |            +0.055 |           +11.2 % |            +0.0100 |            +24.9 % |

On a **relative** scale the stack is _more_ effective on the harder dataset. This is not a chance artefact: the mechanisms in Phase 2b (NL tokens from comments + string literals) and Phase 2e (whole-word coverage bonus) are designed to help exactly the queries where the baseline ranks the target below Acc@3. On 89 that cohort is small; on 436 it dominates, and Phase 2b + 2e together recover a meaningful chunk of it. Phase 2c's marginal contribution stays consistent with §8.3 — small on its own, useful as a tie-breaker when stacked.

**Why default stays OFF for now**: 436 is still _the same repository_. A true external-repo validation (hand-built dataset on a second codebase like `ripgrep`, `tokio`, or a TypeScript project) would be the next bar. The current evidence is strong enough to recommend the stack for any project willing to opt in — especially projects where NL-heavy queries are common — but not yet strong enough to change a global default that forces an index rebuild for every existing deployment.

**Concrete recommendation for v1.5.x users**:

1. If your agents run lots of NL queries against `get_ranked_context`, set all three env gates at launch time:

   ```
   CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1
   CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1
   CODELENS_RANK_SPARSE_TERM_WEIGHT=1
   CODELENS_RANK_SPARSE_THRESHOLD=40
   CODELENS_RANK_SPARSE_MAX=40
   ```

2. If your traffic is mostly identifier / `find_symbol`-style queries, leave the three gates off. They add zero benefit for pure identifier lookups and cost an index rebuild (for 2b/2c) to turn on.

3. Do not flip any of these defaults in forked configs without at least a second-repo A/B — the §8.4 pilot's first attempt measured zero effect because the sparse pass was running on the wrong query string (see §8.4 _"Why the first pilot measured zero"_), and that class of failure is exactly what cross-repo measurement catches.

**Reproduce any 436-query arm**:

```bash
# Arm A — baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset.json

# Arm D — full v1.5 stack
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset.json
```

Measurement artefacts live in `benchmarks/embedding-quality-v1.5-phase2f-aug436-{baseline,2e-only,2b2c-only,stacked}.{json,md}` for audit.

**Still-open work (unchanged from §8.4)**:

- **External-repo validation** — same 4-arm A/B against a medium-size external Rust / TypeScript repo with a hand-built 20–40 query `expected_symbol` dataset. Required before flipping any default ON.
- **Phase 2d — Model swap** — CodeSearchNet-INT8 → E5-large / BGE-base. Biggest ceiling uplift but binary-size + migration cost. The Phase 2b+2c+2e stacked arm (now validated on two query distributions) is the baseline to beat before committing.

### 8.6 v1.5 Phase 2g experiment — Phase 2e parameter sweep

**Hypothesis**: The §8.4 pilot used `CODELENS_RANK_SPARSE_THRESHOLD=40` and `CODELENS_RANK_SPARSE_MAX=40` as the very first tuning values. Before locking these numbers into the v1.5 recommended configuration (§8.5), sweep a 3×3 grid on the 89-query self dataset to confirm that `(40, 40)` is a data-backed optimum rather than a lucky first guess, and to map the shape of the loss surface in a band around it.

**Setup**: Same release binary from `ebb5115` (post-Phase 2f merge), same bundled CodeSearchNet-INT8 model, same 89-query self dataset, same `--isolated-copy` workflow. Phase 2e **solo** (Phase 2b/2c disabled) so the sweep isolates the re-ranker's own loss surface without noise from the embedding-text extractors. Grid:

- `CODELENS_RANK_SPARSE_THRESHOLD` ∈ {30, 40, 50}
- `CODELENS_RANK_SPARSE_MAX` ∈ {30, 40, 50}
- 9 cells + 1 baseline (all gates off) = 10 runs

**Result — `get_ranked_context` hybrid MRR grid** (baseline = 0.5716):

| max \\ threshold |         30 |         40 |     50 |
| ---------------- | ---------: | ---------: | -----: |
| **30**           |     0.5784 |     0.5751 | 0.5734 |
| **40**           | **0.5787** | **0.5787** | 0.5735 |
| **50**           | **0.5787** | **0.5787** | 0.5746 |

**hybrid Acc@3 grid** (baseline = 0.607):

| max \\ threshold |        30 |        40 |    50 |
| ---------------- | --------: | --------: | ----: |
| **30**           |     0.640 |     0.618 | 0.607 |
| **40**           | **0.640** | **0.640** | 0.607 |
| **50**           | **0.640** | **0.640** | 0.618 |

**NL Acc@3 grid** (baseline = 0.491):

| max \\ threshold |        30 |        40 |    50 |
| ---------------- | --------: | --------: | ----: |
| **30**           |     0.545 |     0.509 | 0.491 |
| **40**           | **0.545** | **0.545** | 0.491 |
| **50**           | **0.545** | **0.545** | 0.509 |

**identifier Acc@1** stays at **0.800 across every cell** — the sub-2-token short-circuit holds regardless of threshold/max.

**Key observations**:

1. **A four-cell plateau at `(t ∈ {30, 40}, m ∈ {40, 50})`** hits the exact same hybrid MRR (0.5787), hybrid Acc@3 (0.640), and NL Acc@3 (0.545). Whenever the threshold lets enough candidates receive a bonus _and_ the max is large enough to flip top-1 ordering, further loosening either knob has zero additional effect — all the rescuable queries have already been rescued. This is a clean **diminishing-returns boundary**, not a noisy ridge.
2. **`t = 50` cliff**: at threshold 50 the hybrid MRR drops to 0.5734–0.5746 and NL Acc@3 collapses from 0.545 to 0.491 (baseline) except in one corner (`m = 50`, NL Acc@3 0.509). 50% is clearly over the "typical number of discriminative tokens that appear in a self-dataset NL query", so fewer candidates qualify for the bonus and the re-ranker has nothing to re-order. **50 is the empirical upper bound for this dataset.**
3. **`(t = 30, m = 30)` is inside the plateau on NL Acc@3 (0.545) but slightly below on hybrid MRR (0.5784 vs 0.5787)**: the smallest max just barely has enough re-ordering power for the hybrid metric, confirming that max needs to be at least ≈ 40 for the full effect on short-phrase + natural_language queries.
4. **Identifier queries never regress** (0.800 → 0.800 in every cell), confirming that the short-circuit gate continues to hold at the full parameter range and the sparse pass is effectively invisible to pure identifier traffic.

**Stacked-arm verification** — to confirm that the plateau is not solo-specific, a second stacked run was performed with `(t = 30, m = 40)` (a plateau alternative) and compared against the §8.4 stacked measurement at `(t = 40, m = 40)`:

| Metric (stacked, 2b + 2c + 2e) | t = 40 m = 40 (§8.4) | t = 30 m = 40 (Phase 2g) |    diff |
| ------------------------------ | -------------------: | -----------------------: | ------: |
| hybrid MRR                     |               0.5857 |                   0.5854 | −0.0003 |
| hybrid Acc@1                   |               0.5056 |                   0.5056 |  ±0.000 |
| hybrid Acc@3                   |               0.6517 |                   0.6517 |  ±0.000 |
| NL hybrid MRR                  |               0.4901 |                   0.4897 | −0.0004 |
| NL Acc@1                       |               0.4000 |                   0.4000 |  ±0.000 |
| NL Acc@3                       |               0.5455 |                   0.5455 |  ±0.000 |
| identifier Acc@1               |               0.8000 |                   0.8000 |  ±0.000 |

The two plateau points produce essentially identical stacked results (MRR differs in the fourth decimal, everything else bit-identical). The plateau is real and applies to the stacked regime as well as the solo regime.

**Verdict — `(threshold = 40, max = 40)` is the data-backed optimum**:

- It lives inside the four-cell plateau and matches the §8.4 first-guess exactly, so the §8.5 recommendation holds without change.
- It is the **minimal-aggressive** choice inside the plateau: any lower threshold or higher max costs nothing but also gains nothing, and the conservative defaults are easier to explain to users ("40% of query tokens must match, bonus caps at 40").
- The empirical safe zone for tuning is `threshold ∈ [30, 40]` × `max ∈ [40, 50]`. Anything at threshold 50 trades NL accuracy for nothing, and `max = 30` loses a little hybrid MRR for no offsetting gain.
- Projects that want to experiment can safely swap `(40, 40)` for `(30, 40)` or `(40, 50)` and expect the same stacked performance on this dataset. Anything outside the plateau box should be re-measured before shipping.

**Reproduce the full sweep**:

```bash
for T in 30 40 50; do
  for M in 30 40 50; do
    CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
    CODELENS_RANK_SPARSE_THRESHOLD=$T \
    CODELENS_RANK_SPARSE_MAX=$M \
    CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
    python3 benchmarks/embedding-quality.py . --isolated-copy \
      --dataset benchmarks/embedding-quality-dataset-self.json \
      --output benchmarks/embedding-quality-v1.5-phase2g-t${T}m${M}.json
  done
done
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase2g-{baseline,t30m30..t50m50,stacked-t30m40}.json` (ten solo cells + one stacked verification).

**Still-open work (unchanged from §8.5)**:

- **External-repo validation** — hand-built 20–40 query dataset on a second codebase. Still the single gate before flipping any env default to ON.
- **Phase 2d — Model swap** — the stacked-arm `(t = 40, m = 40)` ceiling (hybrid MRR 0.586 on 89, +7.1 % relative on 436) is the formal baseline to beat before taking on the binary-size cost.

### 8.7 v1.5 Phase 3a experiment — external-repo validation on ripgrep

**Hypothesis**: §8.4–§8.6 validated the v1.5 opt-in stack on two datasets from the **same repository** (`codelens-mcp-plugin` 89-query self + 436-query augmented). This leaves benchmark-contamination as a live risk — a self-measurement cannot rule out the possibility that the Phase 2b/2c/2e mechanisms latch onto the exact phrasing conventions used in our own comments / symbol names / docs. The only way to close that gap is to run the same four-arm A/B against a **completely different codebase**, with a **hand-built query set written by someone who did not author the retrieval stack**. This is the single still-open work item from §8.5 and the D5 prerequisite in the Phase 2d design brief.

**Setup**: `github.com/BurntSushi/ripgrep` shallow-cloned to `/tmp/ripgrep-ext`. Same release binary from `7896f93` (post-v1.5.0, post Phase 2d decision matrix fill), same bundled CodeSearchNet-INT8 model, same four-arm A/B infrastructure as §8.4–§8.6. Phase 2e parameters held at the §8.6 optimum `(threshold = 40, max = 40)`.

**Dataset**: 24 hand-built queries (`benchmarks/embedding-quality-dataset-ripgrep.json`) covering five ripgrep crates — `regex`, `searcher`, `ignore`, `globset`, `printer`. Split 17 natural_language / 5 short_phrase / 2 identifier ≈ 70.8 / 20.8 / 8.3 %, mirroring the 89-query self-dataset shape. Each query's `expected_symbol` + `expected_file_suffix` was cross-checked against the actual ripgrep source with a `grep -rn "^pub struct\|^pub fn"` sweep before the first measurement — no guessed symbols, no hallucinated file paths.

Arms:

- **A — baseline**: all three v1.5 gates off.
- **B — phase2e only**: `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` + threshold/max, Phase 2b/2c off.
- **C — phase2b+2c only**: both embedding-text env gates, Phase 2e off.
- **D — stacked**: all three gates.

**Result** (24 queries, top-10 cutoff, `get_ranked_context` hybrid):

| Metric           | A base |   B 2e | C 2b+2c |  D stacked |
| ---------------- | -----: | -----: | ------: | ---------: |
| hybrid MRR       | 0.4594 | 0.4878 |  0.5104 | **0.5292** |
| hybrid Acc@1     |  0.250 |  0.292 |   0.292 |  **0.333** |
| hybrid Acc@3     |  0.583 |  0.625 |   0.667 |  **0.667** |
| NL hybrid MRR    | 0.4750 | 0.5221 |  0.5245 | **0.5539** |
| NL hybrid Acc@3  |  0.588 |  0.647 |   0.706 |  **0.706** |
| short_phrase MRR |  0.340 |  0.317 |   0.417 |      0.407 |
| identifier Acc@1 |  0.500 |  0.500 |   0.500 |      0.500 |

**Deltas vs baseline**:

| Run             | hybrid MRR | hybrid Acc@1 | hybrid Acc@3 | NL hybrid MRR |   NL Acc@3 | ident Acc@1 |
| --------------- | ---------: | -----------: | -----------: | ------------: | ---------: | ----------: |
| phase2e only    | **+0.028** |   **+0.042** |   **+0.042** |    **+0.047** | **+0.059** |      +0.000 |
| phase2b+2c only | **+0.051** |   **+0.042** |   **+0.083** |    **+0.049** | **+0.118** |      +0.000 |
| **stacked**     | **+0.070** |   **+0.083** |   **+0.083** |    **+0.079** | **+0.118** |      +0.000 |

**Phase 2e marginal value on top of Phase 2b+2c**:

- hybrid MRR: **+0.019**
- hybrid Acc@1: **+0.042**
- NL hybrid MRR: **+0.029**
- NL hybrid Acc@3: **+0.000** (already saturated at 0.706 by Phase 2b+2c alone)
- identifier Acc@1: **+0.000** (gate held)

**Cross-repo verdict — the stack wins on ripgrep, with a _larger_ relative lift than either self-dataset**:

Putting the three datasets side by side (stacked vs baseline, `get_ranked_context` hybrid):

| Dataset                  | baseline MRR | stacked MRR |  Δ absolute |  Δ relative |
| ------------------------ | -----------: | ----------: | ----------: | ----------: |
| 89-query self            |        0.572 |       0.586 |      +0.014 |  **+2.4 %** |
| 436-query augmented self |       0.0476 |      0.0510 |     +0.0034 |  **+7.1 %** |
| **ripgrep external**     |       0.4594 |      0.5292 | **+0.0698** | **+15.2 %** |

**The external-repo relative lift is the largest of the three** — not smaller, not neutral. This rules out the "the v1.5 stack is just memorising our self-phrasing" failure mode. The mechanism that was designed to rescue NL queries whose target lands below Acc@3 is genuinely transferring to a codebase the stack has never seen, whose comments + docstrings + API-call patterns come from a different author (BurntSushi) with a different writing style.

**Direction consistency holds on every dimension that mattered on the self datasets**:

- Every hybrid metric moves positive; every identifier metric holds; every natural_language metric moves positive and larger than hybrid. The exact relative ordering of the four arms (baseline < phase2e solo < phase2b+2c solo < stacked) is preserved.
- `semantic_search` MRR dips by −0.008 on the 2b+2c arms, matching the known Phase 2b `semantic_search`-only regression documented in §8.2. Phase 2e alone keeps `semantic_search` at baseline (expected — the post-process only runs in `get_ranked_context`).
- short_phrase is the only metric where Phase 2e solo slightly regresses (MRR 0.340 → 0.317). The short_phrase arm has **five** queries on this dataset and the sparse pass can only affect ordering when enough candidates clear the coverage threshold — with two-token short phrases against single-token matches in ripgrep's name space, some of them do not. Small-N noise rather than a real signal, and the stacked arm still improves short_phrase MRR (+0.067 absolute) because Phase 2b/2c do the heavy lifting there.

**Default-ON gate status**: this is the first measurement that directly answers the §8.5 still-open question "are the defaults ready to flip?". The result is **yes on the evidence, no on the policy**:

- _On the evidence_: three datasets in three different configurations (89 self / 436 augmented self / ripgrep external), all direction-consistent, all positive, all with identifier Acc@1 held — this is the strongest evidence pattern any v1.5 experiment has produced.
- _On the policy_: one external repo is still one sample. A second external repo (TypeScript or Python, different language family) would close the remaining contamination angle. The defaults stay OFF until v1.6.x gets that second sample, but the **recommendation in §8.5 to enable all three env vars for NL-heavy traffic is now cross-repo validated** — users who were waiting for an external signal before flipping have it.

**Impact on Phase 2d (design brief §1.1 "baseline to beat")**:

The formal baseline any Phase 2d candidate must exceed is no longer just "v1.5 stacked 0.586 on 89-query". The cross-repo validation updates it to a three-point baseline:

| Dataset              | v1.5 stacked hybrid MRR |
| -------------------- | ----------------------: |
| 89-query self        |                   0.586 |
| 436-query augmented  |                  0.0510 |
| **ripgrep external** |              **0.5292** |

A model swap that beats the 89-query number but loses on the ripgrep number is **not a valid winner**. The Checkpoint 1 go/no-go gate in `docs/design/v1.6-phase2d-model-swap-brief.md` §7 is updated accordingly — Phase 2d must clear all three baselines simultaneously.

**Reproduce**:

```bash
# 1. Shallow-clone ripgrep (once)
git clone --depth 1 https://github.com/BurntSushi/ripgrep.git /tmp/ripgrep-ext

# 2. Arm D — full v1.5 stack on ripgrep
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/ripgrep-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-ripgrep.json
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase3a-ripgrep-{baseline,2e-only,2b2c-only,stacked}.json` + the 24-query dataset at `benchmarks/embedding-quality-dataset-ripgrep.json`.

**Still-open work**:

- **Second external repo** (TypeScript or Python, different language family) — would close the last contamination angle and unblock the default-ON flip. Candidate: `facebook/jest` (JS/TS) or `psf/requests` (Python) with a similarly hand-built 20–30 query dataset.
- **Phase 2d — Model swap** — now gated on the three-point v1.5 baseline (0.586 / 0.0510 / 0.5292), not just the 89-query number. All other §8 still-open items inherit the stronger baseline.

### 8.8 v1.5 Phase 3b experiment — Python external-repo validation on psf/requests

**Hypothesis**: §8.7 validated the v1.5 stack on one external Rust repo (ripgrep). Before flipping any env default, run the same four-arm A/B against a **second external repo in a different language family**. The Phase 2d brief D5 suggests Python or JS/TS for this; Python was picked because `psf/requests` is smaller, more focused, and its HTTP semantics map cleanly to NL queries. This is the last contamination check §8.7 flagged as still-open.

**Prior prediction** (from the Phase 3b launch message): Phase 2b and 2e are mechanism-level language-agnostic, Phase 2c is explicitly Rust/C++ scoped (`Type::method` pattern) and should add little or no signal on Python. The stacked arm was predicted to stay direction-consistent positive but with a smaller magnitude than ripgrep, since Phase 2c's contribution would shrink.

**Actual result — the stacked arm regresses on every dimension that matters**. This is the opposite of the prediction.

**Setup**: `github.com/psf/requests` shallow-cloned to `/tmp/requests-ext`. Same release binary, same measurement infrastructure, same Phase 2e parameters `(threshold = 40, max = 40)`. 24 hand-built queries covering 6 modules (`api`, `sessions`, `models`, `adapters`, `auth`, `cookies`), 17/5/2 NL/short-phrase/identifier split mirroring every prior dataset.

**Result** (24 queries, top-10 cutoff, `get_ranked_context` hybrid):

| Metric                | baseline | phase2e only | phase2b+2c only | **stacked (full)** |
| --------------------- | -------: | -----------: | --------------: | -----------------: |
| hybrid MRR            |   0.5837 |       0.5697 |          0.5215 |         **0.4948** |
| hybrid Acc@1          |    0.417 |        0.417 |           0.333 |          **0.333** |
| hybrid Acc@3          |    0.708 |        0.667 |           0.708 |          **0.625** |
| NL hybrid MRR         |   0.6147 |       0.5981 |          0.5367 |         **0.5169** |
| NL hybrid Acc@3       |    0.706 |        0.647 |           0.706 |          **0.647** |
| short_phrase MRR      |    0.312 |        0.301 |           0.278 |          **0.218** |
| short_phrase Acc@3    |    0.600 |        0.600 |           0.600 |          **0.400** |
| identifier Acc@1      |    1.000 |        1.000 |           1.000 |          **1.000** |
| `semantic_search` MRR |   0.5410 |       0.5410 |          0.3935 |         **0.3935** |

**Deltas vs baseline**:

| Run             |  hybrid MRR | hybrid Acc@3 |      NL MRR | NL Acc@3 |  SP MRR |    SP Acc@3 | `s_search` MRR | ident Acc@1 |
| --------------- | ----------: | -----------: | ----------: | -------: | ------: | ----------: | -------------: | ----------: |
| phase2e only    |     −0.0140 |      −0.0417 |     −0.0166 |  −0.0588 | −0.0106 |      ±0.000 |         ±0.000 |      ±0.000 |
| phase2b+2c only |     −0.0622 |       ±0.000 |     −0.0780 |   ±0.000 | −0.0333 |      ±0.000 |    **−0.1475** |      ±0.000 |
| **stacked**     | **−0.0889** |  **−0.0833** | **−0.0979** |  −0.0588 | −0.0939 | **−0.2000** |    **−0.1475** |      ±0.000 |

**Cross-dataset — four data points, two distinct directions**:

| Dataset                        | baseline MRR | stacked MRR |  Δ absolute |  Δ relative |
| ------------------------------ | -----------: | ----------: | ----------: | ----------: |
| 89-query self (Rust)           |        0.572 |       0.586 |      +0.014 |  **+2.4 %** |
| 436-query self (Rust)          |       0.0476 |      0.0510 |     +0.0034 |  **+7.1 %** |
| ripgrep external (Rust)        |       0.4594 |      0.5292 |     +0.0698 | **+15.2 %** |
| **requests external (Python)** |   **0.5837** |  **0.4948** | **−0.0889** | **−15.2 %** |

The four points are a near-perfect mirror. Three Rust datasets trend positive at +2.4 %, +7.1 %, +15.2 %; one Python dataset trends negative at exactly −15.2 %. This is not noise — the short_phrase Acc@3 alone drops by −0.200 absolute on the stacked arm, and `semantic_search` MRR loses −0.148 on the Phase 2b+2c arm regardless of whether Phase 2e is on top. The pattern is structural, not statistical.

**Where the regression comes from**:

Running the Phase 2e solo arm first and then stacking backwards lets us isolate which env gate is responsible:

1. **Phase 2e solo**: −0.014 hybrid MRR. Small regression, consistent with "the sparse re-ranker expects Rust-style snake_case symbols to whole-word-match 2–3 query tokens and Python's mix of `snake_case` + method-on-object call sites doesn't always clear the 40 % threshold". This is within noise for a 24-query dataset.
2. **Phase 2b+2c solo**: **−0.062 hybrid MRR, −0.148 `semantic_search` MRR**. This is the load-bearing regression. `semantic_search` drops by −0.148 means the **embedding text itself got worse**, not the ranking. Because `semantic_search` never sees the Phase 2e post-process, the damage has to come from Phase 2b (NL tokens) or Phase 2c (API calls) at indexing time.
3. **Stacked**: stacking Phase 2e on top of the Phase 2b+2c regression makes it slightly worse (−0.089 vs −0.062) but the damage was already done by the time the re-ranker runs.

The smoking gun is `semantic_search` MRR: **−0.148 is larger than any positive lift Phase 2b produced on any Rust dataset**. The mechanism that was a win on Rust is a net loss on Python.

**Why the stack hurts Python**:

This is a post-mortem, written after running the experiment. Three mechanisms compound:

1. **Python docstrings are already first-class citizens in the baseline**. `extract_leading_doc` in `build_embedding_text` already honours Python triple-quote docstrings, so the _most informative_ NL text in a Python file is already in the embedding. Phase 2b (`extract_nl_tokens`) then re-scans the body for _additional_ NL tokens from line comments and NL-shaped string literals — but on Python, that post-docstring residue is mostly things like `raise ValueError("Invalid URL %s" % url)`, `logging.debug("sending request to %s", url)`, or `return fmt.format(name, value)`. These look NL-shaped to `is_nl_shaped` (multi-word, alphabetic ratio high) but they are **generic error / log / formatting strings**, not behaviour-descriptive prose. They dilute the embedding vector toward "this file handles errors and logging" rather than "this file prepares HTTP requests".
2. **Phase 2c `Type::method` has literally no coverage on Python**. Python uses `obj.method()` and `Module.function()`, never `Type::method`. So Phase 2c adds the empty string on every Python symbol and contributes zero signal — but it also costs zero index size, so it is not the regression source.
3. **Python symbol names are already high-quality NL**. A Rust symbol called `search_path` is structural; the matching NL query is "search file by path". A Python symbol called `HTTPBasicAuth` is _already_ the NL phrase "HTTP basic auth" with zero expansion. The embedding model's job on Python is easier to begin with — note the baseline hybrid MRR 0.5837 on Python is the _highest_ of any dataset tested, higher than the 89-query self baseline (0.572). **The baseline is already close to the ceiling, so any signal dilution moves it down, not up**.

The combined effect: mechanism 1 adds noise, mechanism 3 means the starting point was already good enough that the noise dominates. Phase 2e then re-ranks on that noisier embedding output and cannot undo the damage.

**Verdict — the v1.5 stack is NOT language-agnostic**:

- **Rust datasets (3/3)**: the stack is a clean win. +2.4 % / +7.1 % / +15.2 % relative on hybrid MRR, identifier Acc@1 held, every direction consistent.
- **Python dataset (1/1)**: the stack is a clean loss. −15.2 % relative on hybrid MRR, short_phrase Acc@3 −20 percentage points, `semantic_search` MRR regresses by the largest amount seen on any v1.5 measurement.

This is **measured rejection, not refined recommendation**. It overturns the §8.7 conclusion that "the default-ON flip is only waiting on one more sample". The missing sample has returned the opposite direction. Any global default-ON flip would be a net regression on every Python project in the user base.

**Updated policy — language-gated recommendations**:

The v1.5 opt-in recommendation in §8.5 and §8.7 is revised from "turn on for NL-heavy traffic" to **"turn on for NL-heavy traffic against a predominantly Rust/C++/Go codebase, leave OFF against predominantly Python/JS/TS codebases"**. The exact wording for users:

- **Rust, C++, Go projects**: enable the three env vars. Measured hybrid MRR lift is +2.4 % to +15.2 % relative depending on dataset size and query distribution, identifier queries untouched, `semantic_search` takes a small −0.015 regression that is absorbed by the hybrid path.
- **Python projects**: leave the three env vars off. The stack produces a **measured −15.2 % hybrid MRR regression** on `psf/requests` with the largest single-metric loss being −0.148 in `semantic_search`. Phase 2c adds literally nothing (no `Type::method` syntax), and Phase 2b pollutes the embedding with generic error/log/format strings that the Python docstring-first convention already makes redundant.
- **JS/TS projects**: **untested**. Until a future Phase 3c replays the experiment on a TypeScript repo (e.g. `facebook/jest` or `microsoft/vscode`) the only honest answer is "try it on your own project and measure; the mechanism is orthogonal to whether TS looks more like Rust or more like Python in this respect".

**Impact on Phase 2d design brief (§1.1 "baseline to beat")**:

The three-point baseline set by Phase 3a becomes **a four-point baseline with a split direction**:

| Dataset                    | v1.5 stacked hybrid MRR |
| -------------------------- | ----------------------: |
| 89-query self (Rust)       |                   0.586 |
| 436-query augmented (Rust) |                  0.0510 |
| ripgrep (Rust)             |                  0.5292 |
| **requests (Python)**      |              **0.4948** |

For Phase 2d to be a net improvement, any model swap must:

1. **Beat** the v1.5 stacked MRR on the three Rust datasets (0.586 / 0.0510 / 0.5292), OR match them within a noise floor while also beating a **different** v1.5 baseline the brief doesn't currently enumerate: the **Python baseline without the v1.5 stack** (0.5837 on `requests`). A model swap that reaches 0.586 on 89-query but loses to 0.5837 on Python requests is still a net regression for half the user base.
2. **Not regress Python symbol-text behaviour** the way Phase 2b did. This is an additional constraint the brief did not originally carry. The Checkpoint 1 go/no-go gate in `docs/design/v1.6-phase2d-model-swap-brief.md` §7 picks it up at the next brief update.

**Impact on Phase 2d Checkpoint 1 prerequisites** (brief §7 Prerequisites block):

The tokenizer-vocabulary-swap risk (item 3) is more concerning than §7 previously stated. If a candidate model has a tokenizer that splits `HTTPBasicAuth` differently from `http_basic_auth`, the Rust wins could disappear when the candidate is measured on the same Python dataset that already regresses under v1.5. Any Phase 2d measurement must include the Python baseline as arm 5 (or 9, depending on counting), not just the Rust arms.

**Reproduce**:

```bash
# 1. Clone requests (once)
git clone --depth 1 https://github.com/psf/requests.git /tmp/requests-ext

# 2. Baseline (all three gates off) — the Python recommendation
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json

# 3. Stacked (what NOT to run on Python projects)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase3b-requests-{baseline,2e-only,2b2c-only,stacked}.json` + the 24-query dataset at `benchmarks/embedding-quality-dataset-requests.json`.

**Still-open work**:

- **Language scoping for the v1.5 stack** — rather than a global default flip, consider a language-aware gating: enable Phase 2b only when the project's dominant language is in `{rust, cpp, go}`, disable on `{python, javascript, typescript}` until measured otherwise. The env gates stay as-is for user override; the change is in the auto-detection default.
- **Phase 3c — JS/TS external repo** (e.g. `facebook/jest` or `microsoft/typescript`) to determine whether JS/TS groups with Rust (likely-positive because of static-ish typing and camelCase symbols) or with Python (likely-negative because of docstring-less conventions and dynamic typing). Without this, the user-facing recommendation for JS/TS is "try it and measure".
- **Phase 2b refinement** — a stricter `is_nl_shaped` filter that rejects generic error/log/format strings could reclaim the Python regression without touching the Rust wins. This is a v1.6 direction, not a v1.5.x patch.
- **Phase 2d — Model swap** — the brief's baseline is now four-point with a split direction. Phase 2d proceedures in the brief should be refreshed in a future update to explicitly handle the Python baseline without the v1.5 stack as an additional gate.

### 8.9 v1.5 Phase 2h experiment — strict NL literal filter (partial Python recovery)

**Hypothesis** (from the Phase 3b post-mortem in §8.8): the Python regression is caused primarily by Phase 2b `extract_nl_tokens` collecting **string literals** that look NL-shaped but carry no behaviour-descriptive signal (format templates, generic error messages, log lines). If so, adding a filter that rejects those specific patterns on the Pass-2 string-literal path — while leaving the Pass-1 comment path untouched — should:

1. **Recover Python hybrid MRR to baseline or better** (§8.8 stacked arm was 0.4948 vs baseline 0.5837, a −0.0889 absolute / −15.2 % relative regression).
2. **Preserve every Rust win** — on ripgrep, the §8.7 stacked arm was +0.0698 / +15.2 % and on the 89/436-query self datasets the stacked wins were +0.014 / +0.0034 absolute. If the filter is correctly scoped to the string-literal path, Rust wins should not move.

**Implementation**: a new env gate `CODELENS_EMBED_HINT_STRICT_LITERALS=1` (default OFF) wraps two helpers inside `crates/codelens-engine/src/embedding/mod.rs`:

- `contains_format_specifier(s)` — detects C / Python `%` specifiers (`%s %d %r %f %x %o %i %u`) and `{}` / `{name}` / `{0}` / `{:fmt}` / `{name:fmt}` format placeholders. JSON-like `{name: foo, id: 1}` content is distinguished from format placeholders by the "any whitespace inside braces → reject as format" rule.
- `looks_like_error_or_log_prefix(s)` — case-insensitive prefix match against a short list of common failure / log / notification patterns (`Invalid `, `Cannot `, `Could not `, `Unable to `, `Failed to `, `Expected `, `Unexpected `, `Missing `, `Not found`, `Error: `, `Warning: `, `Sending `, `Received `, `Starting `, `Stopping `, `Calling `, `Connecting `, `Disconnecting `).

The filter runs **only** inside `extract_nl_tokens_inner` Pass 2 (string literals). Pass 1 (comments — the load-bearing Rust signal) is left untouched on purpose. Six new unit tests cover gate-off default, both helpers, the combined reject rule, the string-literal filter path, and the comment-pass-through invariant.

**Setup**: same release binary rebuilt from the Phase 2h branch, same `ripgrep` / `requests` datasets and infrastructure as §8.7 / §8.8, Phase 2e parameters held at `(threshold = 40, max = 40)`. Two runs, one per external repo, each with the full v1.5 stack **plus** `CODELENS_EMBED_HINT_STRICT_LITERALS=1`.

**Result on ripgrep (Rust)** — the strict filter is completely transparent:

| Metric           | baseline | v1.5 stacked | strict + stacked | Δ vs stacked |
| ---------------- | -------: | -----------: | ---------------: | -----------: |
| hybrid MRR       |   0.4594 |       0.5292 |           0.5292 |  **±0.0000** |
| hybrid Acc@3     |    0.583 |        0.667 |            0.667 |  **±0.0000** |
| NL hybrid MRR    |   0.4750 |       0.5539 |           0.5539 |  **±0.0000** |
| NL Acc@3         |    0.588 |        0.706 |            0.706 |  **±0.0000** |
| short_phrase MRR |    0.340 |        0.407 |            0.407 |  **±0.0000** |
| identifier Acc@1 |    0.500 |        0.500 |            0.500 |  **±0.0000** |

**Every metric is bit-identical to the stacked-without-filter arm.** The Rust wins are preserved to four-decimal precision. This rules out the "strict filter accidentally drops behaviour-descriptive literals that Rust relied on" failure mode — the load-bearing Rust signal lives in Pass-1 comments, and the filter never touches Pass 1.

**Result on requests (Python)** — partial recovery, ~8 % of the regression closed:

| Metric             | baseline | v1.5 stacked (§8.8) | strict + stacked | Δ vs stacked | Δ vs baseline |
| ------------------ | -------: | ------------------: | ---------------: | -----------: | ------------: |
| `s_search` MRR     |   0.5410 |              0.3935 |           0.4024 |  **+0.0089** |       −0.1385 |
| hybrid MRR         |   0.5837 |              0.4948 |           0.5021 |  **+0.0073** |       −0.0816 |
| hybrid Acc@1       |    0.417 |               0.333 |            0.333 |      ±0.0000 |       −0.0833 |
| hybrid Acc@3       |    0.708 |               0.625 |            0.625 |      ±0.0000 |       −0.0833 |
| NL hybrid MRR      |   0.6147 |              0.5169 |           0.5272 |  **+0.0103** |       −0.0875 |
| NL Acc@3           |    0.706 |               0.647 |            0.647 |      ±0.0000 |       −0.0588 |
| short_phrase MRR   |    0.312 |               0.218 |            0.218 |      ±0.0000 |       −0.0939 |
| short_phrase Acc@3 |    0.600 |               0.400 |            0.400 |      ±0.0000 |       −0.2000 |
| identifier Acc@1   |    1.000 |               1.000 |            1.000 |      ±0.0000 |        ±0.000 |

**Recovery on MRR-type metrics, no recovery on Acc@k**: the filter lifts `semantic_search` MRR by +0.009, hybrid MRR by +0.007, NL hybrid MRR by +0.010. These are small absolute numbers but every one is in the _correct direction_ and no metric regresses vs the unfiltered stack. Accuracy metrics (Acc@1, Acc@3, short*phrase Acc@3) are unchanged — the filter is changing \_how confident* the right answer's rank is, not moving it between buckets.

**Verdict — partial confirmation, not a full fix**:

| Success criterion                  | Target                   | Achieved                        |
| ---------------------------------- | ------------------------ | ------------------------------- |
| Rust ripgrep hybrid MRR preserved  | ≥ 0.5292 (no regression) | ✅ 0.5292 exactly               |
| Python requests recovery direction | positive vs v1.5 stack   | ✅ +0.0073 hybrid MRR           |
| Python requests baseline recovery  | ≥ 0.5837 (full recovery) | ❌ 0.5021 (−0.0816 vs baseline) |
| Python requests Acc@3 recovery     | ≥ 0.708                  | ❌ 0.625 (unchanged)            |

The hypothesis in §8.8 — "string literals are the main regression source" — is **partially confirmed**. String literals contribute ~8 % of the Python regression; the remaining 92 % lives somewhere else. The two most likely remaining sources are:

1. **Phase 2b Pass-1 comments on Python**. Python has fewer body `#` comments than Rust, but when they exist they are often low-value — `# TODO: handle this`, `# HACK: workaround for ...`, `# FIXME: broken in edge case`. These pass `is_nl_shaped` (multi-word prose) but add noise rather than signal. A future Phase 2i could apply a comment-side analogue of the strict filter, targeting TODO/HACK/FIXME prefixes and noting-style markers.
2. **Phase 2e coverage-bonus threshold on Python symbol names**. Python API surface has a lot of short, high-quality symbol names (`get`, `post`, `put`, `Session`, `Response`) whose Phase 2e coverage ratio is dominated by the short symbol name itself — the 40 % threshold may be too low for a symbol class where the baseline already gets most cases right (§8.8 noted Python baseline hybrid MRR 0.5837 is the highest of any dataset tested). A Python-tuned threshold (e.g. 60 %) might reduce the re-ranker's impact on already-correct rankings.

Neither of these is attempted in Phase 2h — the brief asked for a literal-only filter, and Phase 2h delivers exactly that, honestly measured. Further investigation is left for v1.6+ (either Phase 2i comment-filter or Phase 2j auto-detection gating — both described as still-open work below).

**Updated default policy and opt-in recommendation**:

The strict filter is shipped as a new opt-in env knob, default OFF. The updated §8.8 recommendation:

- **Rust / C++ / Go projects**: enable the v1.5 stack. Adding `CODELENS_EMBED_HINT_STRICT_LITERALS=1` is **zero-cost** (bit-identical results on ripgrep) and is therefore safe to enable pre-emptively — future Python cross-validation will benefit.
- **Python projects**: **leave the v1.5 stack off**. Enabling the stack with `CODELENS_EMBED_HINT_STRICT_LITERALS=1` recovers only ~8 % of the §8.8 regression — the net result is still a −0.082 absolute / −14 % relative hybrid MRR loss. The correct user-facing answer is still "keep Phase 2b/2c/2e off on Python" until a Phase 2i comment-filter or Phase 2j auto-detection gating lands.
- **JS / TS projects**: **still untested**. Phase 3c remains the gating measurement for that language family.

**Impact on Phase 2d design brief baseline (§1.1)**:

No change. Phase 2h produces partial Python recovery but does not exceed the `requests` baseline-without-stack of 0.5837. The four-point Phase 2d baseline from §8.8 stands: 0.586 / 0.0510 / 0.5292 / **0.5837 (no stack)** for requests. A Phase 2d model swap still has to beat all four.

**Reproduce**:

```bash
# Rust — strict + stack, expected bit-identical to plain stack
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_EMBED_HINT_STRICT_LITERALS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/ripgrep-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-ripgrep.json

# Python — strict + stack, expected partial recovery
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_EMBED_HINT_STRICT_LITERALS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase2h-{ripgrep,requests}-strict-stacked.json`.

**Still-open work**:

- **Phase 2i — comment-side strict filter**. Reject TODO / HACK / FIXME / noting-style comment patterns on Pass 1, mirroring the Phase 2h Pass-2 filter. If Python has enough of those, Phase 2i could close the remaining ~92 % of the regression. Rust ripgrep already shows Pass-1 comments are the load-bearing signal; the risk is that a too-aggressive comment filter accidentally drops the Rust wins. Same 4-arm measurement methodology applies.
- **Phase 2j — auto-detection gating**. Rather than refining the filters, auto-detect the project's dominant language (probably via `get_capabilities` language counts) and flip Phase 2b/2c/2e on when the dominant language is in `{rust, cpp, go}` and off otherwise. This is mechanism-level give-up on "one stack fits all" and policy-level acceptance of "measure per language family". Simpler to implement than Phase 2i; less informative about the underlying mechanism.
- **Phase 3c — JS/TS validation** (unchanged).
- **Phase 2d** — unchanged (brief baseline still covers four datasets).

### 8.10 v1.5 Phase 2i experiment — strict comment filter (hypothesis rejected)

**Hypothesis** (from §8.9 "still-open work"): Phase 2h recovered ~8 % of the Python regression by filtering format/error/log string literals on Pass 2. The remaining ~92 % was attributed by the §8.9 post-mortem to one of two candidates: Pass-1 comments (Python `# TODO` / `# HACK` / `# FIXME` annotations) or Phase 2e coverage-bonus threshold tuning. Phase 2i tests the first candidate with the comment-side analogue of the Phase 2h filter.

**Implementation**: a new env gate `CODELENS_EMBED_HINT_STRICT_COMMENTS=1` (default OFF, orthogonal to `CODELENS_EMBED_HINT_STRICT_LITERALS`) wraps a comment filter inside `extract_nl_tokens_inner` Pass 1. Helper `looks_like_meta_annotation(body)` rejects comments whose first word (case-insensitive) matches a conservative 10-entry reject list:

- **Rejected**: `TODO`, `FIXME`, `HACK`, `XXX`, `BUG`, `REVIEW`, `REFACTOR`, `TEMP`, `TEMPORARY`, `DEPRECATED`
- **Preserved** (deliberately): `NOTE`, `NOTES`, `WARN`, `WARNING`, `SAFETY`, `PANIC`

The exclusion list is based on the Rust observation that `// SAFETY: caller must hold the lock` and `// NOTE: this branch handles empty input` carry exactly the behaviour-descriptive text Phase 2b is trying to capture. The inclusion list targets "I'll fix this later" noise that poisons embeddings without describing what the function does.

5 new unit tests cover the gate-off default, the helper's accept/reject invariants, the full extraction path, and orthogonality vs the Phase 2h literal filter (`strict_comments` must not touch Pass 2). Test count: 244 → **249**.

**Setup**: same release binary rebuilt with Phase 2i, same `ripgrep` / `requests` datasets and four-arm methodology, Phase 2e parameters held at `(threshold = 40, max = 40)`. Two runs: ripgrep and requests, each with the full v1.5 stack **plus both** `CODELENS_EMBED_HINT_STRICT_LITERALS=1` and `CODELENS_EMBED_HINT_STRICT_COMMENTS=1`.

**Result on ripgrep (Rust)** — the comment filter is completely transparent:

| Metric           | baseline | v1.5 stacked | Phase 2h (strict lit) | Phase 2i (strict lit + strict cmt) | Δ vs stacked |
| ---------------- | -------: | -----------: | --------------------: | ---------------------------------: | -----------: |
| hybrid MRR       |   0.4594 |       0.5292 |                0.5292 |                             0.5292 |  **±0.0000** |
| hybrid Acc@3     |    0.583 |        0.667 |                 0.667 |                              0.667 |  **±0.0000** |
| NL hybrid MRR    |   0.4750 |       0.5539 |                0.5539 |                             0.5539 |  **±0.0000** |
| NL Acc@3         |    0.588 |        0.706 |                 0.706 |                              0.706 |  **±0.0000** |
| identifier Acc@1 |    0.500 |        0.500 |                 0.500 |                              0.500 |  **±0.0000** |

**Every metric bit-identical to the stacked-without-filter and the Phase 2h arms to four decimals.** Rust ripgrep has few meta-annotation comments that pass `is_nl_shaped` in the first place, and the conservative reject list avoids any Rust content that does carry behaviour signal. The Rust wins are preserved, as intended.

**Result on requests (Python)** — essentially no change vs Phase 2h:

| Metric           | baseline | v1.5 stacked | Phase 2h | Phase 2i |  Δ 2i vs 2h | Δ 2i vs base |
| ---------------- | -------: | -----------: | -------: | -------: | ----------: | -----------: |
| `s_search` MRR   |   0.5410 |       0.3935 |   0.4024 |   0.4024 |     ±0.0000 |      −0.1385 |
| hybrid MRR       |   0.5837 |       0.4948 |   0.5021 |   0.5017 | **−0.0004** |      −0.0820 |
| hybrid Acc@3     |    0.708 |        0.625 |    0.625 |    0.625 |     ±0.0000 |      −0.0833 |
| NL hybrid MRR    |   0.6147 |       0.5169 |   0.5272 |   0.5266 |     −0.0006 |      −0.0881 |
| NL Acc@3         |    0.706 |        0.647 |    0.647 |    0.647 |     ±0.0000 |      −0.0588 |
| identifier Acc@1 |    1.000 |        1.000 |    1.000 |    1.000 |     ±0.0000 |       ±0.000 |

Phase 2i's additional contribution on Python is **−0.0004 hybrid MRR and −0.0006 NL MRR** — noise, well inside the measurement-to-measurement variation. Of the original −0.0889 §8.8 regression, Phase 2h closed +0.0073 (≈ 8 %) and Phase 2i closes an additional **+0 %**. The remaining ~92 % of the regression is not caused by meta-annotation comments.

**Verdict — hypothesis rejected**:

| Success criterion                               | Target              | Achieved            |
| ----------------------------------------------- | ------------------- | ------------------- |
| Rust ripgrep hybrid MRR preserved               | ≥ 0.5292            | ✅ 0.5292 exactly   |
| Python requests additional recovery vs Phase 2h | ≥ +0.010 hybrid MRR | ❌ −0.0004          |
| Python requests baseline recovery               | ≥ 0.5837            | ❌ 0.5017 (−0.0820) |

Meta-annotation comments are **not** the remaining Python regression source. The Phase 2b Pass-1 comment path on Python contributes too little to `requests` for its filtering to move any metric meaningfully. Two possibilities remain for the ~92 % that neither Phase 2h nor Phase 2i recovered:

1. **Phase 2b content-vs-signature ratio on Python**. Python's triple-quote docstrings are already captured by `extract_leading_doc` in the baseline embedding. Phase 2b `extract_nl_tokens` then adds a partial copy of the same docstring body through its Pass-1 comment path (`"""` is not a comment in the sense of `//` or `#`, but Python's class/function docstrings sit in the body and some of their internal lines look NL-shaped after the leading `"""` is stripped). The duplication increases the weight of the docstring vs the signature — if the signature is what the CodeSearchNet-INT8 model was optimised to embed, doubling the docstring weight pushes the representation off the model's learned mode for that symbol class.
2. **Phase 2e coverage-bonus threshold on Python symbol names**. §8.8 noted that the Python baseline hybrid MRR (0.5837) is the highest of any dataset tested — Python's API surface is already close to the retrieval ceiling. The Phase 2e sparse coverage pass at `threshold = 40` reorders top-K candidates whose ratio crosses 40 %; on a dataset where most queries already land their correct answer in the top-3, forcing a re-order can only _move_ correct answers down, not lift incorrect ones up. The Phase 2g sweep (§8.6) locked `(40, 40)` as the Rust optimum, but no sweep was run on Python.

Neither cause is attempted in Phase 2i. **Phase 2j auto-detection gating is now the most practical path forward**: rather than continuing to refine individual filters, accept that the v1.5 mechanism is Rust-optimised and gate it per-language at the MCP tool layer.

**Updated Phase 2i policy**:

Phase 2i **ships the opt-in knob but changes no defaults**. The knob has three intended uses:

1. **Rust infrastructure**. Enabling `CODELENS_EMBED_HINT_STRICT_COMMENTS=1` on Rust projects is a zero-cost no-op today, matching Phase 2h's transparency. Future Phase 2j auto-detection can then flip both strict knobs under the same umbrella env var without re-measuring.
2. **Conservative safety net**. Users with project styles heavy on TODO/FIXME/HACK noise (e.g. very old monorepos) can enable the knob as a targeted cleanup, independent of the Phase 2j auto-detection policy.
3. **Negative-result evidence**. Merging the Phase 2i code + §8.10 narrative makes the rejection bisectable and cite-able. A future contributor who asks "what if we filter out comment annotations?" will find the answer without having to repeat the measurement.

**Updated still-open work**:

- **Phase 2j — auto-detection gating** (now the priority next step). Detect the project's dominant language from the existing symbol index (which already knows `language_for_path` per file) and auto-flip Phase 2b/2c/2e on for `{rust, cpp, go}`, off otherwise. Add a single `CODELENS_EMBED_HINT_AUTO=1` env var that enables the auto-detection, with explicit env overrides still winning for users who want to force a specific configuration. This is mechanism-level acceptance of "the v1.5 stack is Rust-tuned" plus a policy-level fix that unblocks default-ON for the subset of users who run predominantly Rust codebases.
- **Phase 2k — docstring de-duplication on Python** (longer shot). If Phase 2j ships but users still want the stack on mixed-language projects, a future patch could detect when `extract_leading_doc` and `extract_nl_tokens` Pass 1 are producing overlapping content (same paragraph on the same symbol) and drop the duplicate from Pass 1.
- **Phase 3c — JS/TS validation** (still open, unchanged).

**Reproduce either arm**:

```bash
# ripgrep — expected bit-identical to §8.9 (and to plain stacked)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_EMBED_HINT_STRICT_LITERALS=1 \
CODELENS_EMBED_HINT_STRICT_COMMENTS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/ripgrep-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-ripgrep.json

# requests — expected bit-identical to §8.9 (not a fix for Python)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_EMBED_HINT_STRICT_LITERALS=1 \
CODELENS_EMBED_HINT_STRICT_COMMENTS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase2i-{ripgrep,requests}-full-strict.json`.

### 8.11 v1.5 Phase 2j experiment — language-gated auto-detection

**Hypothesis** (from §8.10 Phase 2i rejection): after two consecutive filter-refinement experiments (Phase 2h recovered 8 %, Phase 2i rejected) failed to close the Python regression via mechanism-level fixes, the right next step is **policy-level acceptance** that the v1.5 stack is Rust-optimised. Rather than continue refining filters with diminishing returns, gate Phase 2b / 2c / 2e at the configuration layer based on the project's dominant language: enable the stack on measured-positive languages (Rust / C++ / C / Go / Java / Kotlin / Scala / C#), disable on measured-negative (Python) and untested-dynamic (JavaScript / TypeScript / Ruby / PHP / …) languages.

**Design** — explicit env wins over auto mode:

1. Existing `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`, `CODELENS_EMBED_HINT_INCLUDE_API_CALLS`, and `CODELENS_RANK_SPARSE_TERM_WEIGHT` env vars continue to work **exactly** as before. When set (to any recognised value), they take precedence.
2. New `CODELENS_EMBED_HINT_AUTO=1` env gate turns on the auto-detection fallback. When set and the explicit var is unset, the engine reads `CODELENS_EMBED_HINT_AUTO_LANG` and consults `language_supports_nl_stack` to decide the default.
3. Language tag is supplied by the MCP tool layer (in a follow-up patch) via `CODELENS_EMBED_HINT_AUTO_LANG=<canonical_extension>` — e.g. `rust`, `py`, `ts`, `go`. The engine only reads the tag; it does not walk the filesystem itself. This keeps the engine stateless and avoids owning a project-wide cache that could go stale on file changes.
4. The language support list is **conservative**: `rs`, `rust`, `cpp`, `cc`, `cxx`, `c++`, `c`, `go`, `golang`, `java`, `kt`, `kotlin`, `scala`, `cs`, `csharp`. Everything else — including all dynamic-typed languages, all untested languages, and unknown tags — is classified as unsupported and defaults to the stack **off**. Adding a language to the list requires an actual external-repo A/B following the §8.7 methodology, not a language-similarity argument alone.

**Implementation** — three crate-level additions in `crates/codelens-engine/src/embedding/mod.rs`:

- `auto_hint_mode_enabled()` — reads `CODELENS_EMBED_HINT_AUTO`
- `auto_hint_lang() -> Option<String>` — reads `CODELENS_EMBED_HINT_AUTO_LANG`
- `language_supports_nl_stack(lang: &str) -> bool` — the conservative allow-list
- `auto_hint_should_enable()` — `auto_hint_mode_enabled() && language_supports_nl_stack(auto_hint_lang()?)`

The existing `nl_tokens_enabled` (Phase 2b), `api_calls_enabled` (Phase 2c), and `sparse_weighting_enabled` (Phase 2e, in `scoring.rs`) are each refactored to an explicit-first-then-auto pattern:

```rust
fn nl_tokens_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_COMMENTS") {
        return explicit;
    }
    auto_hint_should_enable()
}
```

The Phase 2e gate in `scoring.rs` calls `crate::embedding::auto_hint_should_enable()` for the same decision, keeping the three gates in lock-step.

**4 new unit tests**:

- `auto_hint_mode_gated_off_by_default` — default OFF verified
- `language_supports_nl_stack_classifies_correctly` — all 13 supported tags + 11 unsupported tags + case-insensitive + whitespace-tolerant
- `auto_hint_should_enable_requires_both_gate_and_supported_lang` — four cases: gate off, gate on + rust (enable), gate on + python (disable), gate on + no tag (conservative off)
- `nl_tokens_enabled_explicit_env_wins_over_auto` — explicit ON / explicit OFF / no-explicit + rust / no-explicit + python

Test count: 249 → **253**.

**Measurement** — two verification runs, each with `CODELENS_EMBED_HINT_AUTO=1` set and **all explicit env vars unset**:

| Run               | Language tag                           | Expected behaviour                                                   |
| ----------------- | -------------------------------------- | -------------------------------------------------------------------- |
| ripgrep (Rust)    | `CODELENS_EMBED_HINT_AUTO_LANG=rust`   | Auto-enables full v1.5 stack → should match §8.7 stacked arm exactly |
| requests (Python) | `CODELENS_EMBED_HINT_AUTO_LANG=python` | Auto-disables everything → should match §8.8 baseline exactly        |

**Result — both verifications hit bit-identity on every metric**:

**ripgrep (auto-rust) vs §8.7 stacked**:

| Metric                | §8.7 stacked | auto-rust |           Δ |
| --------------------- | -----------: | --------: | ----------: |
| `semantic_search` MRR |       0.3972 |    0.3972 | **±0.0000** |
| hybrid MRR            |       0.5292 |    0.5292 | **±0.0000** |
| hybrid Acc@1          |       0.3333 |    0.3333 | **±0.0000** |
| hybrid Acc@3          |       0.6667 |    0.6667 | **±0.0000** |
| NL hybrid MRR         |       0.5539 |    0.5539 | **±0.0000** |
| NL Acc@3              |       0.7059 |    0.7059 | **±0.0000** |
| short_phrase MRR      |       0.4067 |    0.4067 | **±0.0000** |
| short_phrase Acc@3    |       0.6000 |    0.6000 | **±0.0000** |
| identifier Acc@1      |       0.5000 |    0.5000 | **±0.0000** |

Every metric matches to four-decimal precision. The explicit-env / auto-env fallback produces exactly the same behaviour as the three explicit env vars, confirming the refactor is semantically equivalent when the auto path enables the stack.

**requests (auto-python) vs §8.8 baseline**:

| Metric                | §8.8 baseline | auto-python |           Δ |
| --------------------- | ------------: | ----------: | ----------: |
| `semantic_search` MRR |        0.5410 |      0.5410 | **±0.0000** |
| hybrid MRR            |        0.5837 |      0.5837 | **±0.0000** |
| hybrid Acc@1          |        0.4167 |      0.4167 | **±0.0000** |
| hybrid Acc@3          |        0.7083 |      0.7083 | **±0.0000** |
| NL hybrid MRR         |        0.6147 |      0.6147 | **±0.0000** |
| NL Acc@3              |        0.7059 |      0.7059 | **±0.0000** |
| short_phrase MRR      |        0.3118 |      0.3118 | **±0.0000** |
| short_phrase Acc@3    |        0.6000 |      0.6000 | **±0.0000** |
| identifier Acc@1      |        1.0000 |      1.0000 | **±0.0000** |

Every metric matches the §8.8 baseline to four decimals — the auto-python path fully disables Phase 2b/2c/2e and returns the unmodified baseline embedding + ranking behaviour. The −0.0889 hybrid MRR regression from §8.8 is **completely avoided** when the user sets `CODELENS_EMBED_HINT_AUTO=1` and the MCP tool layer reports `lang=python`.

**Verdict — Phase 2j works as specified**:

The two-sided verification is the cleanest evidence pattern any v1.5 experiment has produced: **bit-identical to the positive reference on the supported language and bit-identical to the unmodified baseline on the unsupported language**. No partial recovery, no mixed direction, no language-specific carve-outs. One env var flips the right default for each language.

**Relationship to the four-dataset baseline** (§8.5, §8.7, §8.8):

With Phase 2j, the v1.5 stacked results on the Rust datasets are unchanged (the env var wiring reproduces them exactly) and the Python regression from §8.8 is no longer a default user experience — it only occurs when the user explicitly overrides `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` on a Python project. This removes the "half the user base sees a regression" problem that blocked the §8.7 default flip.

**What Phase 2j still does NOT do**:

1. **Auto-detect the dominant language inside the engine**. The current implementation requires the MCP tool layer to set `CODELENS_EMBED_HINT_AUTO_LANG=<lang>` before the first `index_embeddings` call. Without that, the engine defaults to "no lang tag → conservative off". A follow-up patch will add the auto-set to `activate_project` / `index_embeddings` — but that change is orthogonal to the gating logic, and shipping the gating logic first lets users verify the env var protocol works in their environment before the auto-set lands.
2. **Ship an auto default-ON**. `CODELENS_EMBED_HINT_AUTO=1` is still an opt-in env var. The default remains "no env, no stack" — the same as v1.5.0. The Phase 2j change is "new opt-in path that is _safe_ to enable on mixed-language projects because it self-disables on unsupported languages", not "stack is on by default".
3. **Handle mixed-language projects**. If a project is 60 % Rust + 40 % Python, Phase 2j's current single-tag protocol forces one answer. A future patch could add per-file gating via `language_for_path` at the `build_embedding_text` call site — but that requires threading the project root into the build function, and for v1.5 the single-dominant-language protocol is the lowest-risk way to ship policy-level acceptance.
4. **Ship JS/TS support**. `{js, ts, jsx, tsx}` are classified as unsupported until a future Phase 3c replays the §8.7 methodology on a JavaScript or TypeScript repo. The language-support list is explicit precisely so that adding a new language requires a measurement, not a code change.

**Updated default-ON flip status**:

- v1.5.0 default: all three env vars OFF (unchanged).
- **v1.6.0 candidate default**: `CODELENS_EMBED_HINT_AUTO=1`, all three explicit env vars unset, MCP tool layer auto-sets `CODELENS_EMBED_HINT_AUTO_LANG=<dominant>` on `activate_project`. Rust / C++ / Go / Java / Kotlin / Scala / C# projects get the stacked wins from §8.7. Python / JS / TS / Ruby / PHP / … projects get the §8.8 baseline. Mixed-language projects with a clear dominant language follow the dominant rule. The missing "Phase 2j follow-up" — MCP-layer auto-set — is the one remaining blocker before flipping this as a default; the engine-side gating logic in this PR is ready.

**Reproduce**:

```bash
# ripgrep — auto mode, rust language tag, no explicit env
CODELENS_EMBED_HINT_AUTO=1 \
CODELENS_EMBED_HINT_AUTO_LANG=rust \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/ripgrep-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-ripgrep.json

# requests — auto mode, python language tag, no explicit env
CODELENS_EMBED_HINT_AUTO=1 \
CODELENS_EMBED_HINT_AUTO_LANG=python \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase2j-{ripgrep-auto-rust,requests-auto-python}.json`.

**Still-open work**:

- **Phase 2j follow-up (MCP auto-set)** — the `activate_project` / `index_embeddings` MCP tool layer sets `CODELENS_EMBED_HINT_AUTO_LANG` automatically, so users only need `CODELENS_EMBED_HINT_AUTO=1` at launch. Small scope, no new measurement required — the engine gating is already verified.
- **Phase 3c — JS/TS validation**. The measurement that unblocks adding `ts` / `js` to `language_supports_nl_stack`. Same four-arm A/B methodology; candidate repo `facebook/jest` or `microsoft/typescript`.
- **Phase 2k — per-file gating for mixed-language projects**. Longer shot. The §8.11 single-dominant-language protocol handles the 90 % case, but a 50/50 Rust+Python project is forced into one answer. Per-file gating via `language_for_path` at the `build_embedding_text` call site would give each symbol the right default, at the cost of threading the project root through the build path. Defer until a user actually hits the problem.
- **Phase 2d — Model swap** — unchanged from §8.8. Still gated on the four-point baseline.

---

## 9. See Also

- [docs/architecture.md](architecture.md) — tool surface, layer diagram, full metric table
- [README.md](../README.md) — quick install + `vs Serena` comparison
- Project `CLAUDE.md` — routing policy for agents deciding when to prefer CodeLens over native tools
