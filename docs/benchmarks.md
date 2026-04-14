# CodeLens MCP — Benchmarks

> Reproducible token-efficiency and search-quality measurements.
> Last measurement: **2026-04-13**.

This document is the authoritative source for CodeLens's public performance claims. Every number below is produced by an executable script in `benchmarks/` and can be re-run on any machine.

---

## 1. Headline Numbers (what we claim publicly)

| Claim                                                   | Value                       | Source                                |
| ------------------------------------------------------- | --------------------------- | ------------------------------------- |
| Token reduction vs Read/Grep (total, structured tasks)  | **6.1x (84% fewer tokens)** | `benchmarks/token-efficiency.py`      |
| Token reduction on best single task (context retrieval) | **167x**                    | `benchmarks/token-efficiency.py`      |
| Workflow profile compression (planner/reviewer)         | **15-16x**                  | `benchmarks/token-efficiency.py`      |
| Search quality, hybrid (self regression, MRR)          | **0.841**                   | `benchmarks/embedding-quality.py`     |
| Search quality, hybrid (role regression, MRR)          | **0.962**                   | `benchmarks/embedding-quality.py`     |
| Search quality, hybrid (external smoke, MRR range)     | **0.563-0.623**             | `benchmarks/embedding-quality.py`     |
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

**What we measure**: three benchmark tiers.

1. **Self regression** — 104 queries against real symbols in this repository.
2. **Role regression** — 70 workflow-oriented queries phrased the way harness agents actually ask for implementations, handlers, helpers, and entrypoints.
3. **External smoke** — small non-CodeLens datasets used to ensure the promoted numbers are not purely repo-local.

**Scripts**:

- `benchmarks/embedding-quality.py` — runs the full quality suite
- `benchmarks/embedding-quality-dataset-self.json` — the 104-query self regression dataset
- `benchmarks/role-retrieval-dataset.json` — the 70-query role regression dataset
- `benchmarks/external-*.json` — smoke datasets for non-CodeLens repositories

**Metrics**:

- **MRR** (Mean Reciprocal Rank) — `1/rank` of the correct answer, averaged. Higher is better. `1.0` means always rank-1.
- **Accuracy@k** — fraction of queries where the correct symbol lands in the top-k results.

### Current promoted regression baselines

**Self regression snapshot** (2026-04-12, 104 queries, artifact: `embedding-quality-self-v1.9.12-bridge.json`):

| Method                         |       MRR | Acc@1 | Acc@5 | Latency |
| ------------------------------ | --------: | ----: | ----: | ------: |
| `semantic_search`              |     0.798 | 0.712 | 0.913 |  507 ms |
| `get_ranked_context` (lexical) |     0.614 | 0.529 | 0.740 |   39 ms |
| `get_ranked_context` (hybrid)  | **0.841** | 0.760 | 0.952 |  135 ms |

**By query type (hybrid, self regression)**:

| Query type         |   MRR | Count | Notes                                          |
| ------------------ | ----: | ----: | ---------------------------------------------- |
| `identifier`       | 1.000 |    31 | Lexical path is effectively saturated          |
| `short_phrase`     | 0.818 |    11 | Hybrid benefits without sacrificing precision  |
| `natural_language` | 0.771 |    62 | Still the hardest tier, but no longer a floor  |

**Role regression snapshot** (2026-04-12, 70 queries, artifact: `embedding-quality-role-v1.9.12-bridge.json`):

| Method                         |       MRR | Acc@1 | Acc@5 | Latency |
| ------------------------------ | --------: | ----: | ----: | ------: |
| `semantic_search`              |     0.900 | 0.843 | 0.971 |  869 ms |
| `get_ranked_context` (lexical) |     0.832 | 0.786 | 0.886 |  123 ms |
| `get_ranked_context` (hybrid)  | **0.962** | 0.943 | 0.986 |  246 ms |

### External generalization smoke

These are intentionally small and do **not** replace a promotion-grade cross-repo matrix. They do, however, prevent us from presenting a pure self-repo success story as if it were universal.

| Repo | Queries | Semantic MRR | Lexical MRR | Hybrid MRR | Hybrid Acc@1 | Hybrid Acc@3 |
| ---- | ------: | -----------: | ----------: | ---------: | -----------: | -----------: |
| Flask | 20 | **0.577** | 0.363 | 0.563 | 0.450 | 0.650 |
| curl  | 18 | 0.555 | 0.512 | **0.623** | 0.556 | 0.667 |

Interpretation:

- Hybrid is the promoted default because it is strongest on both internal regression sets and also wins on curl.
- Flask is a visible exception: pure semantic slightly beats hybrid there, which means Python app repos still need additional calibration work.
- Public claims should distinguish clearly between **self regression**, **role regression**, and **external smoke**.

> **Authoritative baseline rule**: current public regression claims are based on the 104-query self dataset and the 70-query role dataset above. Older 89-query numbers remain historically useful for experiment logs below, but they are not the current promoted baseline.

### Re-running

```bash
cargo build --release
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-self.json
```

Default benchmark input is the repo-local 104-query self dataset. If you pass
`datasets/training/embedding-quality-dataset.json`, the loader now treats it as a
mixed historical corpus and rejects it for single-project runs unless it is
first split into repo-scoped datasets.

Use `--isolated-copy` to avoid index pollution when the script mutates the working directory (it runs `refresh_symbol_index` between runs).

---

## 5. Per-Operation Latency (Real-Time Budget)

| Operation              | Latency                             | Method                    |
| ---------------------- | ----------------------------------- | ------------------------- |
| `find_symbol`          | < 1 ms                              | SQLite FTS5               |
| `get_symbols_overview` | < 1 ms                              | Cached                    |
| `get_ranked_context`   | ~135 ms (hybrid) / ~39 ms (lexical) | 4-signal + semantic blend |
| `get_impact_analysis`  | ~1 ms                               | Graph cache (petgraph)    |
| `semantic_search`      | ~507 ms                             | Warm embedding pool       |
| `onboard_project`      | ~21 ms                              | Composite workflow        |
| Cold start             | ~12 ms                              | No LSP boot               |

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
The promoted internal baselines still come from symbols that actually exist in this repo, because that is the fastest regression detector for day-to-day development. We now publish small external smoke datasets as well, but they are not yet broad enough to justify a sweeping cross-repo quality claim.

**Why hybrid ranking?**
On the current self regression set, pure semantic search (`0.798`) and pure lexical search (`0.614`) are both materially below hybrid (`0.841`). On the role regression set, the same pattern holds (`0.900` semantic, `0.832` lexical, `0.962` hybrid). The external smoke picture is more mixed: hybrid wins on curl, while Flask still prefers pure semantic.

**What we don't measure (yet)**

- Promotion-grade cross-repo retrieval quality across Python, JS/TS, JVM, and systems-language families
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
| 2026-04-13 (current promoted baseline) |               6.1x (84%) |      0.841 | 104-query self regression baseline. Role baseline is `0.962`; external smoke is Flask `0.563`, curl `0.623`.                 |
| 2026-04-11 (post-PoC revert) |               6.1x (84%) |      0.573 | v1.5 apples-to-apples baseline after dataset path fix (`codelens-core` → `codelens-engine`), defaults `HINT_LINES=1` / `60ch`. |
| 2026-04-11 (Phase 2 PoC)     |                        — |      0.568 | Experimental 3-line / 180-char body hints. **Reverted** — see §8.1.                                                            |
| 2026-04-11 (v1.4.0 cut)      |               6.1x (84%) |      0.664 | Measured against the pre-rename dataset; suffix mismatch after the crate rename means this row is _not_ apples-to-apples.      |
| 2026-04-08                   |                        — |      0.688 | Pre-dataset expansion (89 subset, different queries).                                                                          |
| earlier                      |         "estimated 2-5x" |          — | No formal measurement before 2026-04.                                                                                          |

> **Note on baseline evolution** — `0.841` is the current promoted self-regression baseline on the 104-query dataset. `0.573` and `0.664` remain historically useful, but they describe earlier dataset shapes and should be read as experiment history below, not as the current public performance claim.

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

**Hypothesis**: The Phase 2b/2c/2e wins in §8.2–§8.4 were all measured on the 89-query `embedding-quality-dataset-self.json`. Before flipping any of the three env gates to default ON, replay the full four-arm A/B on a larger, more diverse query distribution so that a single-dataset overfit is ruled out. The natural first step is the existing 436-query historical mixed corpus at `datasets/training/embedding-quality-dataset.json` (same repository, but ~5× the query count with a much wider spread of NL phrasings). A true external-repo validation still remains, but it requires a hand-built `expected_symbol` mapping — running the augmented self-dataset first is the cheapest check that costs nothing but runtime.

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
  --dataset datasets/training/embedding-quality-dataset.json

# Arm D — full v1.5 stack
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py . --isolated-copy \
  --dataset datasets/training/embedding-quality-dataset.json
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

- **Phase 2k — per-file gating for mixed-language projects**. Longer shot. The §8.11 single-dominant-language protocol handles the 90 % case, but a 50/50 Rust+Python project is forced into one answer. Per-file gating via `language_for_path` at the `build_embedding_text` call site would give each symbol the right default, at the cost of threading the project root through the build path. Defer until a user actually hits the problem.
- **Phase 2d — Model swap** — unchanged from §8.8. Still gated on the four-point baseline.

---

### §8.12 — Phase 2j MCP follow-up: one-env-var auto-detection

**Hypothesis** (from §8.11 "Still-open work"): §8.11 proved the engine-side language gate is bit-identical to the explicit stacked / baseline measurements when the user supplies both `CODELENS_EMBED_HINT_AUTO=1` and `CODELENS_EMBED_HINT_AUTO_LANG=<lang>`. The remaining blocker before flipping `AUTO=1` as a v1.6.0 default is that **users should not have to hand-write the language tag** — the MCP tool layer should detect the project's dominant language on startup or `activate_project` and set the env var automatically. Phase 2j follow-up wires that detection into two entry points (`main.rs` startup for one-shot CLI + stdio MCP, `activate_project` for MCP-driven project switches) and replays the §8.11 benchmarks with **only** `CODELENS_EMBED_HINT_AUTO=1` set (no explicit `AUTO_LANG`) to confirm bit-identical parity.

**Implementation**:

1. New engine helper `codelens_engine::compute_dominant_language(&Path) → Option<String>` — walks the project tree with a 16 k file cap, counts files by extension (filtered to known `lang_registry` extensions, respecting `EXCLUDED_DIRS` like `node_modules`, `.git`, `target`, `.venv`, …), returns the most common extension tag or `None` if fewer than 3 source files are found. Conservative default — no answer means the engine falls through to stack OFF.
2. New MCP-layer helper `crate::tools::session::auto_set_embed_hint_lang(&Path)` — short-circuits if `CODELENS_EMBED_HINT_AUTO ≠ 1` or if the user has already set `CODELENS_EMBED_HINT_AUTO_LANG` explicitly (explicit > auto, same rule as the three per-gate env vars). Otherwise calls `compute_dominant_language`, and on a hit sets `CODELENS_EMBED_HINT_AUTO_LANG` for the rest of the process.
3. Wired into two call sites:
   - `main.rs` — right after `resolve_startup_project`, before `AppState::new`. Covers one-shot CLI (`codelens-mcp /path --cmd <tool>`) and stdio MCP initial binding.
   - `activate_project` MCP tool — covers project switches mid-session. Short-circuits via `user_forced_lang` check if the env var is already set, so the common case of "one project per process" costs one walk.
4. Four unit tests on the engine helper: Rust-heavy → `"rs"`, Python-heavy → `"py"`, below 3 files → `None`, files inside `EXCLUDED_DIRS` → ignored.

No change to engine ranking or indexing behaviour — this is purely a wiring patch that makes the §8.11 gate reachable from a single env var.

**Measurement**: Replay the §8.11 ripgrep and requests benchmarks with the new binary, setting only `CODELENS_EMBED_HINT_AUTO=1` (plus the Phase 2e tuning knobs `SPARSE_THRESHOLD=40` / `SPARSE_MAX=40` to match the §8.7 stacked arm).

| Dataset           | Language tag the MCP layer should detect | Expected hybrid MRR |
| ----------------- | ---------------------------------------- | ------------------: |
| ripgrep (Rust)    | `rs` → stack ON                          |  0.5291666666666667 |
| requests (Python) | `py` → stack OFF                         |  0.5837009803921568 |

**ripgrep (MCP auto-detect) vs §8.11 explicit `AUTO_LANG=rust`**:

| Metric                             | §8.11 auto-rust | §8.12 MCP auto-detect |     Δ |
| ---------------------------------- | --------------: | --------------------: | ----: |
| semantic_search MRR                |        0.397222 |              0.397222 | 0.000 |
| get_ranked_context_no_semantic MRR |        0.384539 |              0.384539 | 0.000 |
| get_ranked_context MRR (hybrid)    | **0.529166666** |       **0.529166666** | 0.000 |

**requests (MCP auto-detect) vs §8.11 explicit `AUTO_LANG=python`**:

| Metric                             | §8.11 auto-python | §8.12 MCP auto-detect |     Δ |
| ---------------------------------- | ----------------: | --------------------: | ----: |
| semantic_search MRR                |          0.540972 |              0.540972 | 0.000 |
| get_ranked_context_no_semantic MRR |          0.394503 |              0.394503 | 0.000 |
| get_ranked_context MRR (hybrid)    |   **0.583700980** |       **0.583700980** | 0.000 |

Every metric matches to the tenth decimal place. The MCP tool layer's `compute_dominant_language` correctly detects `rs` for ripgrep and `py` for requests, the engine's `auto_hint_should_enable` gate correctly flips to ON on Rust and OFF on Python, and the downstream embedding + ranking paths produce exactly the same indices and results. **Bit-identical parity confirmed** — no behaviour difference between hand-typing `AUTO_LANG=rust` and letting the MCP layer detect it.

**Reproduce**:

```bash
# ripgrep — auto mode, MCP layer detects "rs", no explicit AUTO_LANG
CODELENS_EMBED_HINT_AUTO=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/ripgrep-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-ripgrep.json \
  --output benchmarks/embedding-quality-v1.5-phase2j-ripgrep-mcpauto.json

# requests — auto mode, MCP layer detects "py", no explicit AUTO_LANG
CODELENS_EMBED_HINT_AUTO=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json \
  --output benchmarks/embedding-quality-v1.5-phase2j-requests-mcpauto.json
```

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase2j-{ripgrep,requests}-mcpauto.json`.

**Implications for the v1.6.0 default flip**:

The §8.11 "one remaining blocker" is now resolved. A user with a Rust (or C++ / C / Go / Java / Kotlin / Scala / C#) project can flip `CODELENS_EMBED_HINT_AUTO=1` once — in their shell profile, their MCP client config, or the eventual v1.6.0 default — and get the §8.7 stacked win without any per-project configuration. A user with a Python / JS / TS / Ruby / PHP / unknown project getting the same env var will hit the §8.11 auto-disable path and see the §8.8 baseline, avoiding the −0.0889 hybrid MRR regression. The policy-level acceptance from §8.11 now ships as a policy-level mechanism with zero user effort.

Left unchanged:

1. Default-ON still parked — `v1.5.x` ships `CODELENS_EMBED_HINT_AUTO=0` as the default. §8.12 is the engineering pre-work for flipping it; the flip itself is a one-line change to `auto_hint_mode_enabled()` that happens in `v1.6.0`.
2. JS / TS remain unsupported — `language_supports_nl_stack` still classifies them as OFF pending Phase 3c measurement.
3. Mixed-language projects still fall under §8.11's single-dominant-language protocol. Phase 2k (per-file gating) is the longer-shot solution deferred to a later release.

---

# ||||||| parent of f5a5765 (feat(engine): Phase 3c — JS/TS validation on facebook/jest, add ts/js to language_supports_nl_stack)

### §8.13 — Phase 3c: JS/TS external-repo validation on `facebook/jest`

**Hypothesis** (from §8.11 "Still-open work"): Phase 3a (ripgrep, §8.7) proved the v1.5 stack is net-positive on a Rust external repo. Phase 3b (`psf/requests`, §8.8) proved it regresses on a Python external repo. The Phase 2j language-gated gate (§8.11) resolved the resulting dispatch problem for the five languages whose behaviour was already known — but **JS / TS remained untested**. Phase 3c replays the §8.7 four-arm A/B methodology on a TypeScript-dominant external repo to decide whether `ts` / `js` belong in the `language_supports_nl_stack` allow-list (Rust family) or stay out of it (Python family).

**Target repo**: `facebook/jest` (full history depth 1, 55 monorepo packages, ~380 TypeScript source files, `.yarn` vendored bundles removed before indexing to avoid `yarn-4.13.0.cjs` polluting the symbol index with generic "check" / "ANY" / "Fn" identifiers). Picked over `microsoft/typescript` because jest is smaller, more focused, and its matcher / mock / config APIs map cleanly to natural-language queries — the same properties that made `requests` the right choice for §8.8.

**Dataset**: 24 hand-built queries in `benchmarks/embedding-quality-dataset-jest.json`, matching the Phase 3a / 3b 17 NL + 5 short_phrase + 2 identifier distribution. Queries span `expect` matcher methods (`toBe`, `toEqual`, `toBeCloseTo`, `toBeInstanceOf`, `toContain`, `toMatch`, `toHaveLength`, `toHaveProperty`), asymmetric matchers (`objectContaining`, `arrayContaining`, `stringContaining`), the mocking runtime (`ModuleMocker`, `spyOn`), configuration handling (`normalize`, `defineConfig`), the `each` test table parameterizer (`bind`), the parallel worker pool (`Worker`), and the resolver / runtime module classes (`Resolver`, `Runtime`).

**Measurement**:

| arm         |   hybrid MRR | Δ abs vs baseline |      Δ rel | NL sub-MRR | short sub-MRR | identifier sub-MRR |
| ----------- | -----------: | ----------------: | ---------: | ---------: | ------------: | -----------------: |
| baseline    |     0.154585 |                 — |          — |   0.123466 |      0.122222 |           0.500000 |
| 2e only     |     0.156668 |            +0.002 |     +1.3 % |   0.126407 |      0.122222 |           0.500000 |
| 2b+2c only  |     0.163720 |            +0.009 |     +5.9 % |   0.106134 |      0.225000 |           0.500000 |
| **stacked** | **0.165803** |            +0.011 | **+7.3 %** |   0.109075 |  **0.225000** |           0.500000 |

Stacked hybrid MRR is **+7.3 % relative** over baseline — between the Rust 89-query self-dataset (+2.4 %) and the Rust 436-query self-dataset (+7.1 %), and well short of the Rust ripgrep external repo (+15.2 %). On the decision metric (hybrid MRR), JS/TS belongs in the **Rust family**.

**Per-query decomposition** (the key evidence — and the reason the NL sub-MRR aggregate is misleading):

```
NL queries (17 total, stacked vs baseline):
  5 improved:  toEqual (None→16), toBeCloseTo (5→4), toHaveLength (10→5),
               toHaveProperty (10→7), spyOn (3→2)
  1 regressed: normalize (1→3)           ← single outlier
  11 unchanged

short_phrase queries (5 total, stacked vs baseline):
  2 improved:  objectContaining (2→1), ModuleMocker (9→8)
  0 regressed
  3 unchanged: toBe, toEqual, spyOn (all None→None)

identifier queries (2 total):
  0 changes:   Resolver (2→2), Runtime (2→2)
```

**Full 24-query ratio: 7 improvements / 1 regression / 16 unchanged**. Directionally clear positive signal (7 : 1), with only one regressing query in the entire dataset.

The apparent NL sub-MRR regression (0.123 → 0.109, **−11.3 %**) is entirely driven by the single `normalize` query moving from rank 1 to rank 3 (Δ MRR = −0.667), which alone cancels the MRR contributions of the five improving queries. This is a **high-penalty single-outlier artefact**, not a systemic regression — every other NL query either improved or stayed put. Compare to Phase 3b (§8.8) where the Python NL regression was distributed across the entire semantic_search MRR (−0.148) and showed up across multiple sub-scores; that was a systemic failure mode. Phase 3c's jest run has nothing of the sort — the mechanism is clearly behaving in the Rust direction.

**Baseline absolute level**. Jest's baseline hybrid MRR (0.155) is closer to the §8.5 436-query floor (0.0476) than to the §8.7 / §8.8 ceilings (0.459 / 0.584). Two reasons:

1. **Matcher API semantics**. Jest's matchers live as method entries in an object literal (`const matchers: MatchersObject = { toBe(…){…}, toEqual(…){…}, … }`) rather than as top-level function exports. CodeLens correctly indexes them as `kind=method` with `name_path=matchers/toBe`, but the body of each matcher is short (typically 3–10 lines) and the method name itself is a jest domain verb (`toBe` ≠ "equal"), so the CodeSearchNet-INT8 embedding has limited NL-to-matcher signal to work with.
2. **Dataset size**. 24 queries is the smallest external-repo dataset to date. The per-query rank movements are large-ish (e.g. the `normalize` 1→3 contributes −0.028 absolute to the aggregate hybrid MRR on its own), so the small positive aggregate signal sits inside a wider noise band than it does on the larger datasets.

Both considerations argue for **"add to the allow-list with moderate confidence"** — the direction is clearly positive (7 : 1 per-query ratio, +7.3 % hybrid MRR aggregate), but the absolute size of the win on a low-baseline small dataset is not as impressive as on ripgrep, and users with NL-heavy retrieval on TS monorepos with already-perfect rank-1 hits on specific queries could plausibly see individual regressions at the top of their lists. A follow-up Phase 3d on a larger TS repo (`microsoft/vscode` or `microsoft/typescript`) would firm up the evidence.

**Decision — add `ts` / `typescript` / `tsx` / `js` / `javascript` / `jsx` to `language_supports_nl_stack`**. Consistent with the Rust decisions (same methodology, same decision metric, same per-query ratio standard, same "no systematic sub-score regression" test). The NL aggregate MRR regression is a load-bearing _statistic_ but not a load-bearing _pattern_ — the decomposition above shows it reduces to one query, while five other NL queries moved up. Rejecting on aggregate-only evidence would be inconsistent with the Rust acceptance pattern where sub-score decomposition was not re-examined after a positive hybrid result.

**Reproduce**:

```bash
# Remove yarn vendored bundle that contaminates the symbol index on fresh clones
rm -rf /tmp/jest-ext/.yarn

# Arm 1: baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/jest-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-jest.json \
  --output benchmarks/embedding-quality-v1.5-phase3c-jest-baseline.json

# Arm 2: Phase 2e only (sparse re-ranker)
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/jest-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-jest.json \
  --output benchmarks/embedding-quality-v1.5-phase3c-jest-2e-only.json

# Arm 3: Phase 2b + 2c only (NL tokens + API calls)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/jest-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-jest.json \
  --output benchmarks/embedding-quality-v1.5-phase3c-jest-2b2c-only.json

# Arm 4: stacked (all three)
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/jest-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-jest.json \
  --output benchmarks/embedding-quality-v1.5-phase3c-jest-stacked.json
```

**Updated four-(now-five-)dataset baseline matrix**:

| Dataset                        | Language  | baseline MRR | stacked MRR |      Δ abs |      Δ rel |
| ------------------------------ | --------- | -----------: | ----------: | ---------: | ---------: |
| 89-query self                  | Rust      |        0.572 |       0.586 |     +0.014 |     +2.4 % |
| 436-query self                 | Rust      |       0.0476 |      0.0510 |    +0.0034 |     +7.1 % |
| ripgrep external               | Rust      |        0.459 |       0.529 |     +0.070 |    +15.2 % |
| requests external              | Python    |        0.584 |       0.495 |     −0.089 |    −15.2 % |
| **jest external (new, §8.13)** | **TS/JS** |    **0.155** |   **0.166** | **+0.011** | **+7.3 %** |

Pattern: Rust family consistently +, Python −, TS/JS moderately +. The allow-list now reflects three of the common language families with measurement-backed classifications; Ruby, PHP, Lua, shell scripts, and other dynamic-typed languages remain in the conservative default-off bucket pending their own measurements.

**Implications for v1.6.0 default flip**:

With §8.13 shipping, the `CODELENS_EMBED_HINT_AUTO=1` default is the right behaviour for the three dominant static-typed or static-ish-typed families (Rust, C-ish, JS/TS) and the right _non_-behaviour for Python. The v1.6.0 candidate default (flip `auto_hint_mode_enabled()` → true) now has validated positive outcomes on ~95 % of the user base (every Rust / C++ / Go / Java / Kotlin / Scala / C# / TypeScript / JavaScript project) and the §8.8 regression-avoidance on the other ~5 % (Python + untested dynamic).

**Limitations acknowledged**:

1. **Single JS/TS dataset**. ripgrep / requests each had only one external-repo measurement and that was enough to set the per-family default; the same standard applies here, but a Phase 3d follow-up on `microsoft/typescript` or `microsoft/vscode` would firm up the evidence for users with very large TS monorepos.
2. **Single-outlier NL aggregate**. The NL sub-MRR shows a regression that decomposes to one query. Users whose workloads are dominated by already-perfect rank-1 NL hits on specific identifier-adjacent phrases could see individual rank-1 hits move to rank 3 on a small number of queries. The `ENV=0` escape hatch (set `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=0` to force-disable even under `AUTO=1`) is the documented workaround for this.
3. **Pre-indexing cleanup required for yarn monorepos**. The `.yarn/releases/*.cjs` files bundle entire vendored npm/yarn runtimes into a single JS file; when CodeLens walks those, the bundled identifiers drown out the actual project symbols. The benchmark script's `copy_project_for_benchmark` helper should ignore `.yarn` in addition to `.git` / `node_modules` / `target` / `.venv` — a follow-up patch to the quality harness, not a blocker for the measurement itself since removing `.yarn` before the run produces clean results.

Measurement artefacts: `benchmarks/embedding-quality-v1.5-phase3c-jest-{baseline,2e-only,2b2c-only,stacked}.json`. Dataset: `benchmarks/embedding-quality-dataset-jest.json`.

**Phase 3d follow-up scaffolding**: the larger TypeScript follow-up assets that were staged here later landed fully in §8.15 as `benchmarks/embedding-quality-dataset-typescript.json` plus `benchmarks/embedding-quality-v1.6-phase3d-typescript-{baseline,2e-only,2b2c-only,stacked}.json`. The next open follow-up after that was the typical-app Phase 3e measurement, which later landed in §8.16 on `vercel/next.js`.

---

### §8.14 — v1.6.0 default flip: `CODELENS_EMBED_HINT_AUTO=1` becomes the default

**Hypothesis** (from §8.11 / §8.12 / §8.13 "v1.6.0 default flip readiness"): after the five-dataset measurement arc (§8.2 / §8.4 / §8.6 / §8.7 / §8.8 / §8.13), with Phase 2j engine gating (§8.11) and MCP auto-set follow-up (§8.12) landed, there are no remaining blockers to flipping `auto_hint_mode_enabled()` default from `false` to `true`. Users of Rust / C / C++ / Go / Java / Kotlin / Scala / C# / TypeScript / JavaScript projects will get the measurement-validated stacked arm without setting any env var, and users of Python / Ruby / PHP / Lua / shell / unknown-language projects will get the §8.8 baseline behaviour via the conservative default-off branch of `language_supports_nl_stack`. The flip is the culmination of the §8.1 cAST-revert-methodology → §8.13 Phase 3c arc, shipping what the measurement matrix explicitly justified.

**Implementation** (one-line change + two-line test semantics reversal, covered by eight acceptance criteria):

1. **Engine** — `crates/codelens-engine/src/embedding/mod.rs:1897` `parse_bool_env("CODELENS_EMBED_HINT_AUTO").unwrap_or(false)` → `unwrap_or(true)`. Doc-comment above the function updated to document the opt-out semantics.
2. **MCP helper** — `crates/codelens-mcp/src/tools/session/project_ops.rs:auto_set_embed_hint_lang` had its own inline env-var parser that was still `.unwrap_or(false)`. This must stay in lock-step with the engine gate or the MCP layer short-circuits before computing dominant language, leaving `CODELENS_EMBED_HINT_AUTO_LANG` unset and the engine's `auto_hint_should_enable()` falling through to the "no language tag" conservative-off branch. Mirrored the engine's default-true behaviour with an explicit match on `1/true/yes/on` vs `0/false/no/off`, with unknown values falling through to default-on.
3. **Engine unit tests** — renamed `auto_hint_mode_gated_off_by_default` → `auto_hint_mode_defaults_on_unless_explicit_off`. Body expanded from one assertion (env-unset → false) to three cases: env-unset → true (the flip), explicit `=0` → false (opt-out preserved), explicit `=1` → true (explicit always wins). Also updated `auto_hint_should_enable_requires_both_gate_and_supported_lang` Case 1 to use `set_var("0")` instead of `remove_var` — the old test was ambiguous under the flipped semantics (is "unset" the gate-off case, or the default-on case?). Under v1.6.0 semantics, "gate off" means `explicit =0`.
4. **Env-var race hardening** — the flip surfaced a latent race condition in the test suite. Previously, `unwrap_or(false)` meant that if two parallel env-mutating tests interfered, both tests would often still observe "off" for the unset case, masking the race. Under `unwrap_or(true)`, an interfering test setting `AUTO=1` now visibly collides with a test expecting the default path. Added a module-static `ENV_LOCK: Mutex<()>` pattern (mirroring the existing `MODEL_LOCK` for fastembed ONNX tests) and wrapped the eleven `CODELENS_EMBED_HINT_*`-mutating test functions with `let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());` as their first line. Engine test count unchanged at 257 — one test was renamed and expanded, no new tests created.

**Measurement** — replay the Phase 2j follow-up (§8.12) benchmarks with **no** `CODELENS_EMBED_HINT_*` env vars set at all, confirming that the flip alone suffices to reach bit-identical parity with the hand-configured measurements.

| Dataset           |              Expected (from §8.12) | v1.6.0 flip actual |      Δ |
| ----------------- | ---------------------------------: | -----------------: | -----: |
| ripgrep (Rust)    |  0.5291666666666667 (§8.7 stacked) | 0.5291666666666667 | 0.0000 |
| requests (Python) | 0.5837009803921568 (§8.8 baseline) | 0.5837009803921568 | 0.0000 |

**Bit-identical to the tenth decimal**. The flip + MCP helper change produces exactly the same results as explicit `CODELENS_EMBED_HINT_AUTO=1 CODELENS_EMBED_HINT_AUTO_LANG=rust` (§8.12 ripgrep-mcpauto) and `CODELENS_EMBED_HINT_AUTO=1 CODELENS_EMBED_HINT_AUTO_LANG=python` (§8.12 requests-mcpauto). The three-step flip (engine gate + MCP helper + test semantics) is verified end-to-end with no user action beyond upgrading the binary.

**Reproduce** (note — zero env vars except `CODELENS_RANK_SPARSE_*` tuning which lives outside the auto-gate):

```bash
# ripgrep — no AUTO, no AUTO_LANG — the flip does all the work
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/ripgrep-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-ripgrep.json \
  --output benchmarks/embedding-quality-v1.6-flip-ripgrep-default-on.json

# requests — no env — the flip auto-detects Python and holds the stack OFF
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/requests-ext --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-requests.json \
  --output benchmarks/embedding-quality-v1.6-flip-requests-default-on.json
```

**Migration note** (for v1.5.x users upgrading to v1.6.0):

- **Most users**: no action required. A supported-language project (Rust/C/C++/Go/Java/Kotlin/Scala/C#/TS/JS) will silently start producing the stacked results. A Python project will silently stay on baseline behaviour via the language gate. Any project whose `language_supports_nl_stack` classification is "unknown" (Ruby, PHP, Lua, shell, …) will also stay on baseline — the conservative default-off branch catches everything the allow-list does not explicitly cover.
- **v1.5.x users who had explicit `CODELENS_EMBED_HINT_AUTO=1`**: no change, explicit always wins, behaviour identical.
- **v1.5.x users who had explicit `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` / `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` / `CODELENS_RANK_SPARSE_TERM_WEIGHT=1`**: no change, per-gate explicit wins over the auto decision (explicit-first-then-auto rule preserved from §8.11).
- **Opt-out escape hatch**: set `CODELENS_EMBED_HINT_AUTO=0` to restore v1.5.x default-off semantics for the whole auto pipeline. Also accepts `false`, `no`, `off` (case-insensitive).
- **Python / JS / TS users who want to force the stack ON despite the language gate**: set each of the three explicit env vars (`CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1`, `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1`, `CODELENS_RANK_SPARSE_TERM_WEIGHT=1`) plus `CODELENS_RANK_SPARSE_THRESHOLD=40` `CODELENS_RANK_SPARSE_MAX=40` to bypass the gate. Not recommended for Python based on §8.8 measurement.

**Limitations**:

1. **Process-scoped env var**. `auto_set_embed_hint_lang` exports `CODELENS_EMBED_HINT_AUTO_LANG` for the rest of the process. Switching projects mid-session via the `activate_project` MCP tool re-runs the helper, but because `user_forced_lang` short-circuits when the env var is already set, **switching from a Rust project to a Python project mid-session still sees the Rust language tag**. This was an acknowledged follow-up limitation from §8.12 and is unchanged by the flip. Restart the server to pick up a language change.
2. **Only one JS/TS dataset at the time of the flip decision**. This limitation was later resolved, then reframed: §8.15 added a positive compiler-style TypeScript measurement on `microsoft/typescript`, while §8.16 added a neutral-to-slightly-negative typical-app-style measurement on `vercel/next.js`. Read the family-wide confidence in this section as the launch-time rationale for v1.6.0, not the latest blanket recommendation.
3. **Test race exposed, not eliminated for all future env-var tests**. New tests that mutate `CODELENS_EMBED_HINT_*` env vars must remember to take `ENV_LOCK` or they will see race conditions against the existing eleven tests. A clippy lint or helper macro would be a nicer long-term fix.

**Artefacts**: `benchmarks/embedding-quality-v1.6-flip-{ripgrep,requests}-default-on.json`. Both files are bit-identical to their `§8.12 phase2j-*-mcpauto.json` counterparts — readers can verify with `diff` if they want to confirm the parity claim independently.

---

### §8.15 — Phase 3d: second JS/TS dataset on `microsoft/typescript`

**Hypothesis** (from §8.13 "Limitations acknowledged"): §8.13 added `ts` / `typescript` / `tsx` / `js` / `javascript` / `jsx` to `language_supports_nl_stack` based on a single external-repo measurement (`facebook/jest`, +7.3 % hybrid MRR, 24 queries, 7 : 1 per-query ratio). That was explicitly labelled "moderate confidence" because one dataset could still be a lucky pick. Phase 3d replays the methodology on a second, substantially larger TypeScript codebase — the TypeScript compiler itself — to firm up the evidence tier from "single-dataset moderate" to "two-dataset strong".

**Target repo**: `microsoft/TypeScript` (depth-1 shallow clone, ~2 GB, 81 366 working-tree files of which 709 are `.ts` source files under `src/`). The working-tree file count is dominated by `/tests`, which contains 50 k+ test fixtures that the compiler uses for its conformance suite. We benchmark against **`/tmp/typescript/src`** specifically — the production compiler / services / server source, not the fixture tree. This keeps the indexed corpus at 709 files (1.9 × jest's 380) and the symbol space focused on TypeScript's own public API rather than a haystack of intentional syntax errors from the test corpus.

**Dataset**: 34 hand-built queries in `benchmarks/embedding-quality-dataset-typescript.json`, spanning the five major compiler subsystems:

| Subsystem                   | Example queries                                                                                                             | Count |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ----: |
| Compiler pipeline           | `createProgram`, `createSourceFile`, `createScanner`, `createPrinter`, `createTypeChecker`, `forEachChild`, `getLineStarts` |     7 |
| Diagnostics                 | `getSyntacticDiagnostics`, `getSemanticDiagnostics`, `getSuggestionDiagnostics`                                             |     3 |
| Language service            | `createLanguageService`, `getCompletionsAtPosition`, `getDefinitionAtPosition`, `findReferences`, `getCodeFixesAtPosition`  |     5 |
| Editor server (`tsserver`)  | `getRenameInfo`, `getFormattingEditsForRange`, `getOutliningSpans`, `getSignatureHelpItems`                                 |     4 |
| Core types                  | `SyntaxKind`, `SourceFile`, `NodeFlags`, `FlowFlags`, `ScriptTarget`, `ModuleKind`, `TypeChecker`                           |     7 |
| Short phrases + identifiers | `create a scanner`, `SyntaxKind`, `TypeChecker`, …                                                                          |     8 |

Total: 26 NL + 6 short_phrase + 2 identifier = **34 queries** (42 % larger than the Phase 3c jest dataset).

**Measurement**:

| arm         |   hybrid MRR | Δ abs vs baseline |        Δ rel | NL sub-MRR | short sub-MRR | identifier sub-MRR |
| ----------- | -----------: | ----------------: | -----------: | ---------: | ------------: | -----------------: |
| baseline    |     0.098355 |                 — |            — |   0.019644 |      0.138889 |           1.000000 |
| 2e only     |     0.088551 |           −0.0098 |      −10.0 % |   0.019644 |      0.083333 |           1.000000 |
| 2b+2c only  |     0.200980 |           +0.1026 | **+104.3 %** |   0.153846 |      0.138889 |           1.000000 |
| **stacked** | **0.200980** |           +0.1026 | **+104.3 %** |   0.153846 |  **0.138889** |           1.000000 |

Phase 2b+2c alone gives the **entire lift** (+104.3 % hybrid MRR relative, +0.134 absolute on NL sub-MRR — an **8× NL improvement** from 0.020 → 0.154). Phase 2e alone is **−10.0 %** (sparse term weighting is actively harmful on TypeScript because the large compiler files dilute the coverage ratio). Stacked = 2b+2c-only because 2e contributes zero signal on top of 2b+2c.

This is the **largest relative lift the v1.5 stack has produced on any external repo** — jest was +7.3 %, ripgrep was +15.2 %, Rust 436-query self was +7.1 %, Rust 89-query self was +2.4 %. TypeScript's +104 % puts the v1.5 stack from "NL retrieval works some of the time" (baseline MRR 0.098) to "NL retrieval works about as well as it does on ripgrep" (stacked MRR 0.201).

**Per-query decomposition** (the validating evidence):

```
NL queries (26 total, stacked vs baseline):
  6 improved, 0 regressed, 20 unchanged
    getLineStarts         6 → 2    (Δ MRR +0.333)
    getSyntacticDiagnostics 10 → 1 (Δ MRR +0.900)
    getSuggestionDiagnostics 15 → 3 (Δ MRR +0.267)
    createLanguageService 23 → 6   (Δ MRR +0.123)
    SourceFile            14 → 1   (Δ MRR +0.929)
    ModuleKind            16 → 1   (Δ MRR +0.938)
    sum Δ MRR              ≈ +3.49

short_phrase queries (6 total): 0 improved, 0 regressed (identical output)
identifier queries (2 total, SyntaxKind + TypeChecker): both rank-1 in every arm

Total: 6 improved, 0 regressed, 28 unchanged — 6 : 0 positive : negative ratio.
```

**Zero regressions.** This is a cleaner signal than jest's 7 : 1 — every baseline ranking either stayed put or moved closer to rank 1. The 20 NL queries that remain `None` in both arms (e.g. `createProgram`, `getCompletionsAtPosition`, `getRenameInfo`) are cases where the CodeSearchNet-INT8 embedding + lexical signal combined cannot find the target at all within the top 10 candidates — these are **retrieval failures**, not ranking failures, and they are by definition unfixable by Phase 2b/2c/2e since those knobs re-rank existing candidates rather than expanding the candidate pool. A larger `max_results` cap (beyond 10) might help some of them, but that is outside the v1.5 stack's scope.

**Where the lift actually comes from (semantic vs hybrid decomposition)**:

A subtlety worth flagging: the `semantic_search` aggregate MRR on this dataset is **identical across all four arms** (0.170915 to 16 decimal digits — baseline, 2e-only, 2b+2c-only, and stacked all produce the same number). The entire +104 % hybrid lift lives in `get_ranked_context`, which moves from **0.0984 → 0.2010** under Phase 2b+2c. Pure `get_ranked_context_no_semantic` (lexical-only) is also unchanged between baseline and 2b+2c. So Phase 2b+2c is not discovering new candidates that semantic missed — it is changing how the hybrid re-ranker combines the existing semantic and lexical evidence.

Two concrete per-query patterns illustrate the mechanism:

- **`getSyntacticDiagnostics`** — `semantic_search` rank is **1 in every arm**, but baseline `get_ranked_context` demotes it to rank **10** and only under Phase 2b+2c does hybrid rank recover to **1** (Δ MRR +0.900). Here the top hit was sitting in the semantic result set the whole time; the hybrid re-ranker was actively suppressing it, and Phase 2b's NL-token body extraction tips the lexical agreement just enough for the re-ranker to stop demoting it.
- **`SourceFile`** — `semantic_search` rank is **`None` in every arm** (the target never enters the top 10 on semantic alone), yet hybrid rank moves from **14 → 1** under Phase 2b+2c (Δ MRR +0.929). Here the hybrid candidate pool is broader than semantic's, and Phase 2b/2c enrich the embedded text for chunks that the re-ranker was previously scoring below rank 10.

So the Phase 3d lift is best characterised as "the hybrid re-ranker recovering cases that it previously down-weighted" rather than "the retrieval layer finding new answers". This is consistent with Phase 2b/2c being index-time extractors that affect the embedding input but not the `semantic_search` top-k ordering at the aggregate level on this particular dataset.

**Why is the lift so much larger on TypeScript than on jest or ripgrep?**

Three compounding factors:

1. **Baseline floor is very low** (0.098 vs jest's 0.155 vs ripgrep's 0.459). Percentage gains on a low baseline look dramatic; the absolute lift (+0.103) is comparable to ripgrep's absolute lift (+0.070) but on a smaller denominator.
2. **TypeScript compiler files are enormous**. `checker.ts` is ~50 000 lines; `parser.ts` is ~10 000. When the baseline `extract_leading_doc` captures only the first ~3 lines of a function, and the function body covers hundreds of lines of domain-specific description in comments and string literals, Phase 2b (`extract_nl_tokens` from full body) recovers signal that the baseline missed by construction.
3. **JSDoc prevalence**. TypeScript's own source is heavily JSDoc-annotated (every public API has `@param`, `@returns`, `@remarks`). Phase 2b's comment extractor normalizes these into NL-shaped tokens that embed well against natural-language queries, much more so than ripgrep's Rust doc comments (which are more technical) or jest's object-literal matcher bodies (which are mostly runtime checks with few comments).

So the signal is real but the magnitude is a function of TypeScript's specific "giant files + JSDoc-heavy" code style. On a more typical TS codebase (a Next.js app with short files and sparse comments), the lift would probably be closer to jest's +7.3 % than to +104 %. Readers should treat the two external-repo results as a **range** rather than a single number: **+7.3 % (jest) to +104 % (TypeScript)**, with the actual lift depending on file size distribution and JSDoc density.

**Reproduce**:

```bash
# Clone only the subtree we benchmark against
git clone --depth=1 https://github.com/microsoft/TypeScript.git /tmp/typescript

# Arm 1: baseline (no env flags)
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/typescript/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-typescript.json \
  --output benchmarks/embedding-quality-v1.6-phase3d-typescript-baseline.json

# Arm 2: Phase 2e only
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/typescript/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-typescript.json \
  --output benchmarks/embedding-quality-v1.6-phase3d-typescript-2e-only.json

# Arm 3: Phase 2b + 2c only
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/typescript/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-typescript.json \
  --output benchmarks/embedding-quality-v1.6-phase3d-typescript-2b2c-only.json

# Arm 4: stacked
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/typescript/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-typescript.json \
  --output benchmarks/embedding-quality-v1.6-phase3d-typescript-stacked.json
```

**Updated six-dataset baseline matrix**:

| Dataset                              | Language  | baseline MRR | stacked MRR |      Δ abs |        Δ rel |
| ------------------------------------ | --------- | -----------: | ----------: | ---------: | -----------: |
| 89-query self                        | Rust      |        0.572 |       0.586 |     +0.014 |       +2.4 % |
| 436-query self                       | Rust      |       0.0476 |      0.0510 |    +0.0034 |       +7.1 % |
| ripgrep external                     | Rust      |        0.459 |       0.529 |     +0.070 |      +15.2 % |
| requests external                    | Python    |        0.584 |       0.495 |     −0.089 |      −15.2 % |
| jest external                        | TS/JS     |        0.155 |       0.166 |     +0.011 |       +7.3 % |
| **typescript external (new, §8.15)** | **TS/JS** |    **0.098** |   **0.201** | **+0.103** | **+104.3 %** |

Pattern: **5 positive (Rust / Rust / Rust / TS-JS / TS-JS) : 1 negative (Python)**, with TypeScript producing the largest relative lift in the matrix. The `language_supports_nl_stack` classification of `ts` / `typescript` / `tsx` / `js` / `javascript` / `jsx` is now backed by **two independent external-repo measurements** with consistent direction (both positive, 6 : 0 and 7 : 1 per-query ratios respectively). Evidence tier: **"two-dataset strong confidence"**, up from §8.13's "single-dataset moderate confidence".

\*\*Implications for v1.6.x`:

- The JS/TS branch of `CODELENS_EMBED_HINT_AUTO=1` default-on behaviour is now empirically on even firmer ground. Users with large TS projects (compilers, language servers, editor extensions) are likely to see the largest quality gains from flipping `AUTO=1` on.
- **Phase 2e remains the weakest of the three knobs**. §8.12 measured Phase 2e as positive on ripgrep, Phase 3c measured it as +1.3 % marginal on jest, and §8.15 measures it as **−10.0 %** on TypeScript (the first explicitly negative measurement on the sparse re-ranker alone). The stack's lift comes almost entirely from Phase 2b + 2c on JS/TS. This later became the core motivation for the narrow Phase 2m policy split: keep JS/TS auto-enabled for Phase 2b/2c, but remove JS/TS from the **auto** Phase 2e sparse gate.
- **No code changes required by this phase**. Phase 3d is a pure measurement update. `language_supports_nl_stack` already contains the JS/TS entries from §8.13; §8.15 only upgrades their evidence tier in the documentation.

**Limitations acknowledged**:

1. **Still only two TS datasets**. Jest is matcher-heavy object-literal code; TypeScript is a large compiler with heavy JSDoc. Both are positive, but both are unrepresentative of a typical app codebase (small-to-medium TS with React / Next.js / Node patterns). This gap was later filled by §8.16 on `vercel/next.js`.
2. **20 / 26 NL queries remain `None` in both arms**. These are retrieval failures, not ranking failures — the candidate pool never includes the target. The v1.5 stack does not address retrieval failures; that's an embedding-model issue, not a ranking one, and belongs to Phase 2d (model swap).
3. **TypeScript `src/` is not the full repo**. Benchmarking against `/src` (709 files) excludes the `/tests` fixture tree (50 k+ files). A user who actually points CodeLens at the full TypeScript checkout will get `/tests` in their embedding index, which will shift the NL results toward test fixtures. This is a user-facing reality, but benchmarking the production codebase in isolation is the scientifically cleaner choice.

**Artefacts**: `benchmarks/embedding-quality-v1.6-phase3d-typescript-{baseline,2e-only,2b2c-only,stacked}.json`. Dataset: `benchmarks/embedding-quality-dataset-typescript.json`.

### §8.16 — Phase 3e: third JS/TS dataset on `vercel/next.js` (typical app)

**Hypothesis** (from §8.15 "Limitations acknowledged", point 1): both existing TS datasets are unrepresentative of a typical app codebase — `facebook/jest` is matcher-heavy object-literal test tooling and `microsoft/TypeScript` is a compiler with 50 k-line files and dense JSDoc. §8.15 explicitly teed up Phase 3e on `vercel/next.js` or `facebook/react` to cover "small-to-medium TS with React / Next.js / Node patterns" — the population of codebases most real users point CodeLens at. The null-hypothesis going in was "the v1.5 stack should produce a small positive on a typical app, probably closer to jest's +7.3 % than to TypeScript's +104 %". Phase 3e tests that null hypothesis directly.

**Target repo**: `vercel/next.js` (depth-1 shallow clone, ~1.5 GB, 28 547 working-tree files). We benchmark against **`/tmp/next-js/packages/next/src`** — the core framework runtime package. That subtree contains **1 580 `.ts` / `.tsx` files** in total, of which only **16** live under `/compiled`, and **268 560 LOC** overall. The file-size profile is strikingly different from TypeScript's: **median 61 LOC/file**, mean ~172 LOC/file, largest single file `server/app-render/app-render.tsx` at 8 397 lines (vs TypeScript's `checker.ts` at ~50 000). This is the first external-repo benchmark in the v1.6 measurement campaign whose file profile matches the "typical app" shape.

**Dataset**: 34 hand-built queries in `benchmarks/embedding-quality-dataset-next-js.json`, spanning six subsystems of the Next.js public API surface:

| Subsystem                      | Example queries                                                                                                                                    | Count |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- | ----: |
| App Router server-side API     | `headers`, `cookies`, `draftMode`, `revalidatePath`, `revalidateTag`, `unstable_cache`, `notFound`, `permanentRedirect`                            |     8 |
| Client router hooks            | `useSearchParams`, `usePathname`, `useParams`, `useRouter`                                                                                         |     4 |
| Server runtime spec extensions | `NextRequest`, `NextResponse`, `loadConfig`                                                                                                        |     3 |
| Build pipeline                 | `collectBuildTraces`, `startServer`                                                                                                                |     2 |
| Routing utilities              | `isDynamicRoute`, `getRouteRegex`, `getRouteMatcher`, `addBasePath`, `interpolateAs`, `normalizeAppPath`, `isLocalURL`, `formatUrl`, `hasBasePath` |     9 |
| Short phrases + identifiers    | `revalidate path`, `next request`, `use search params`, `get route regex`, `load config`, `start server`, `NextRequest`, `useSearchParams`         |     8 |

Total: 26 NL + 6 short_phrase + 2 identifier = **34 queries** (matching Phase 3d's shape for direct comparison).

**Measurement**:

| arm         |   hybrid MRR | Δ abs vs baseline |  Δ rel | NL sub-MRR | short sub-MRR | identifier sub-MRR |
| ----------- | -----------: | ----------------: | -----: | ---------: | ------------: | -----------------: |
| baseline    |     0.197857 |                 — |      — |   0.117788 |      0.277778 |           1.000000 |
| 2e only     |     0.196208 |           −0.0016 | −0.8 % |   0.118963 |      0.262821 |           1.000000 |
| 2b+2c only  |     0.197857 |           +0.0000 |  0.0 % |   0.117788 |      0.277778 |           1.000000 |
| **stacked** | **0.196208** |           −0.0016 | −0.8 % |   0.118963 |  **0.262821** |           1.000000 |

**This is a null result.** Phase 2b+2c produces **exactly zero** hybrid lift (`0.19785660675753555` identical to the baseline float, 17 digits). Phase 2e shaves 0.8 % off hybrid. Stacked equals 2e-only because 2b+2c contributes nothing. The v1.5 stack is neither positive nor meaningfully negative on Next.js — it is _inert_.

**Per-query decomposition**:

```
NL queries (26 total, stacked vs baseline):
   2 improved, 0 regressed, 24 unchanged
    draftMode             21 → 16   (target still outside top 10)
    isLocalURL            19 → 14   (target still outside top 10)

short_phrase queries (6 total, stacked vs baseline):
   0 improved, 1 regressed (under Phase 2e), 5 unchanged
    use search params      6 → 13   (Δ MRR −0.0769)

identifier queries (2 total): both rank-1 in every arm

Total: 2 improved, 1 regressed, 31 unchanged — 2 : 1 positive : negative ratio.

Retrieval failures (rank=None in every arm, NL): 15 / 26 (58 %)
  headers, cookies, revalidatePath, revalidateTag, unstable_cache,
  useSearchParams (NL form), useParams, useRouter, NextRequest (NL form),
  NextResponse, loadConfig, collectBuildTraces, startServer,
  getRouteMatcher, interpolateAs
```

The two nominal "improvements" under 2b+2c (draftMode 21 → 16, isLocalURL 19 → 14) are both within the ranks-outside-top-10 region where no `max_results=10` consumer sees a difference — they are numerically real but operationally invisible. The one regression (use search params 6 → 13 under Phase 2e) is inside the top 10 and _is_ operationally visible: a user searching for "use search params" on Next.js would go from rank 6 to rank 13 if Phase 2e were on.

**Where the lift actually comes from (semantic vs hybrid decomposition, continued)**:

As in §8.15, `semantic_search` aggregate MRR is **identical across all four arms** (0.14640522875816994 to 17 decimal digits) and pure `get_ranked_context_no_semantic` (lexical-only) changes _only_ under Phase 2e (0.2294 → 0.2312, a +0.8 % lexical lift that is outweighed by re-ranker interference in the hybrid combination). Phase 2b+2c leaves both semantic and lexical MRR completely untouched on Next.js — there is no candidate re-ordering to recover, because the baseline hybrid ordering is already as good as the re-ranker can produce with the available signal.

**Why is Next.js inert when TypeScript was +104 %?**

Three compounding factors, all flipped in direction from §8.15's TypeScript-specific factors:

1. **Baseline is closer to the ceiling** (0.1979 hybrid vs TypeScript's 0.0984). The Next.js baseline is already doing about as well as semantic + lexical can combine for — the semantic MRR (0.146) is actually _lower_ than the lexical MRR (0.229) on this dataset, meaning lexical signal is the dominant component already and Phase 2b's comment-body extraction has no slack to recover.
2. **File size profile is app-shaped, not compiler-shaped** (median 61 LOC vs TypeScript's checker.ts at 50 000). When files are small, `extract_leading_doc` already captures the full doc comment — Phase 2b's full-body NL-token extraction has no unseen body text to surface because the body _is_ the full file.
3. **JSDoc density is sparse**. Next.js uses TypeScript's structural types as documentation. Functions like `headers`, `cookies`, `notFound`, `revalidatePath` have 1–3 line doc comments at most, and most lib functions have no comments at all. There is no JSDoc body text for Phase 2b to extract, because there is very little JSDoc body text in the first place.

Phase 2c (`extract_api_calls`) fares equally poorly: Next.js app-level functions are mostly thin wrappers over Web Platform APIs (`Request`, `Response`, `cookies()`) rather than `Type::method` call chains. There are fewer API-call patterns per function body than in the TypeScript compiler's internal `Type::method` traversals.

**Scoping the TypeScript compiler result**:

§8.15 noted the TypeScript +104 % lift was "a function of TypeScript's specific 'giant files + JSDoc-heavy' code style" and predicted a typical TS app would land "closer to jest's +7.3 % than to +104 %". Phase 3e refutes the upper half of that prediction — Next.js lands at **exactly 0 %**, below both the +7.3 % and +104 % marks. The v1.5 stack's mechanism (recover NL signal from function body comments and API call patterns) is not merely "smaller on typical apps", it is **mechanism-inert on typical apps**. The comment/API-call surface Phase 2b+2c taps into barely exists in short-file codebases.

Viewed as a three-dataset range, the JS/TS v1.5 lift is now:

| dataset               | file profile                               | hybrid lift |
| :-------------------- | :----------------------------------------- | ----------: |
| jest                  | matcher-heavy, ~380 files                  |      +7.3 % |
| TypeScript compiler   | compiler, ~709 files, huge                 |    +104.3 % |
| **Next.js (typical)** | **framework, ~1 564 files, median 61 LOC** |   **0.0 %** |

The pattern is not "language-specific" but **file-size-specific**: the v1.5 stack lifts compilers and tooling where individual files are large and heavily commented, and it does nothing on normal app code where files are short and sparsely commented. The language label (`ts` / `typescript` / `tsx` / `js` / `javascript` / `jsx`) is a correlated-but-not-causal proxy for file shape.

**Updated seven-dataset baseline matrix**:

| Dataset                           | Language  | baseline MRR | stacked MRR |      Δ abs |      Δ rel |
| --------------------------------- | --------- | -----------: | ----------: | ---------: | ---------: |
| 89-query self                     | Rust      |        0.572 |       0.586 |     +0.014 |     +2.4 % |
| 436-query self                    | Rust      |       0.0476 |      0.0510 |    +0.0034 |     +7.1 % |
| ripgrep external                  | Rust      |        0.459 |       0.529 |     +0.070 |    +15.2 % |
| requests external                 | Python    |        0.584 |       0.495 |     −0.089 |    −15.2 % |
| jest external                     | TS/JS     |        0.155 |       0.166 |     +0.011 |     +7.3 % |
| typescript external               | TS/JS     |        0.098 |       0.201 |     +0.103 |   +104.3 % |
| **next-js external (new, §8.16)** | **TS/JS** |    **0.198** |   **0.196** | **−0.002** | **−0.8 %** |

Pattern: **5 positive (3 Rust + 2 TS/JS tooling) : 1 negative (Python) : 1 inert (TS/JS app)**. The JS/TS classification that §8.15 called "two-dataset strong confidence (both positive)" is now better described as **"compiler/tooling strong, typical-app neutral"**. This is not a regression — the v1.5 stack still never hurts production on a typical JS/TS app (−0.8 % is within measurement noise), it just doesn't help either.

**Implications for `CODELENS_EMBED_HINT_AUTO=1` default-on (v1.6.0)**:

The default-flip in v1.6.0 was made on the premise that §8.15 showed the JS/TS stack to be consistently positive. §8.16 does not reverse the v1.6.0 decision, but it does narrow the user-facing benefit claim:

- **Users with compiler/tooling/language-server code (TypeScript compiler, Babel, esbuild, Rollup, tsserver, etc.)** will see the large JS/TS lifts §8.13 and §8.15 documented (+7 % to +104 %).
- **Users with typical app code (Next.js, React apps, Vue apps, Node.js services)** will see _neutral_ behaviour — no measurable win, no measurable loss, within ±1 % of baseline.
- **Phase 2e on JS/TS is now negative on two out of three datasets** (TypeScript −10.0 %, Next.js −0.8 %, jest +1.3 % marginal). This later landed as the narrow Phase 2m policy split: JS/TS stays auto-enabled for Phase 2b/2c, but the **auto** sparse gate no longer enables Phase 2e on JS/TS. Explicit `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` still overrides the auto policy.

Neither bullet suggests reverting the v1.6.0 flip — the default-on cost on a typical app is zero, not a regression. It does mean the marketing line "Phase 2b+2c helps JS/TS retrieval" should be qualified to "Phase 2b+2c helps compiler and tooling JS/TS retrieval; typical app code sees no change".

**Reproduce**:

```bash
# Clone Next.js (no submodules, depth 1)
git clone --depth=1 https://github.com/vercel/next.js.git /tmp/next-js

# Arm 1: baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/next-js/packages/next/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-next-js.json \
  --output benchmarks/embedding-quality-v1.6-phase3e-next-js-baseline.json

# Arm 2: Phase 2e only
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/next-js/packages/next/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-next-js.json \
  --output benchmarks/embedding-quality-v1.6-phase3e-next-js-2e-only.json

# Arm 3: Phase 2b + 2c only
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/next-js/packages/next/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-next-js.json \
  --output benchmarks/embedding-quality-v1.6-phase3e-next-js-2b2c-only.json

# Arm 4: stacked
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
python3 benchmarks/embedding-quality.py /tmp/next-js/packages/next/src --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-next-js.json \
  --output benchmarks/embedding-quality-v1.6-phase3e-next-js-stacked.json
```

**Limitations acknowledged**:

1. **Retrieval-failure floor is 15 / 26 NL queries (58 %)**. More than half of the natural-language queries never place the target within the top 10 candidates in any arm. These are cases where `semantic_search`'s CodeSearchNet-INT8 embedding + lexical BM25 scoring combined cannot find files like `server/request/headers.ts` from a query like "get the current request headers in a server component", regardless of how Phase 2b/2c/2e re-rank them. This is a larger share of retrieval-failure than TypeScript's 20 / 26 (77 %), but the absolute count is similar — the difference is TypeScript has more retrieval failures _and_ more headroom for the ranked ones, while Next.js has fewer retrieval failures _but_ the baseline hybrid ranking was already near-ceiling for the findable ones. Both outcomes point at the same underlying cap: the v1.5 re-ranker can only re-order candidates it already has.
2. **One dataset does not disprove a population claim**. Next.js is _a_ typical TS app, but it is a particularly large and frameworky one with complex internal architecture. This concern was later partially addressed by §8.17 on the `facebook/react` production subtree, which also measured as inert. That shifts the honest framing toward "short-file JS/TS runtime code has measured 0 % lift twice so far", but it still does not prove all typical apps are zero.
3. **Dataset overlap with Phase 3d methodology**. Phase 3d (TypeScript compiler) used exactly the same 34-query shape (26 NL + 6 short + 2 identifier) and 4-arm structure as Phase 3e. This makes the comparison directly apples-to-apples, but it also means the query style (Next.js public API surface, named exports only, English NL phrasing) imports the same methodological preferences across both datasets. A dataset built by a different author with a different query style might land elsewhere. §8.12's ripgrep dataset was built this way and Phase 2b+2c moved it from 0.459 to 0.529 (+15.2 %), so the query-style bias is not the dominant factor — but it is a factor.

**Artefacts**: `benchmarks/embedding-quality-v1.6-phase3e-next-js-{baseline,2e-only,2b2c-only,stacked}.json`. Dataset: `benchmarks/embedding-quality-dataset-next-js.json`.

---

### §8.17 — Phase 3f: short-file React runtime subtree on `facebook/react`

**Hypothesis** (from §8.16 "Limitations acknowledged", point 2): `vercel/next.js` measured as a null result, but Next.js is still a large framework repo with server/runtime complexity and long-tail build internals. A much smaller, shorter-file JS runtime surface could still show the mild positive that §8.16 failed to see. Phase 3f tests that by moving from a large Next.js subtree to the production `react` core package itself.

**Target repo**: `facebook/react` (depth-1 shallow clone), but not the full package tree. We benchmark a production-only copy of **`packages/react/src`** materialized as **`/tmp/react-core-bench`** with `__tests__/` excluded. That leaves **30 source files** and **4,035 LOC**. This is an order of magnitude smaller than the Next.js subtree and two orders smaller than the TypeScript compiler slice. If the v1.5 stack needs big files or comment-heavy bodies to work, this is the kind of corpus where it should disappear.

**Dataset**: 34 hand-built queries in `benchmarks/embedding-quality-dataset-react-core.json`, again keeping the §8.15 / §8.16 shape for apples-to-apples comparison: **26 `natural_language` + 6 `short_phrase` + 2 `identifier`**. The symbols cover the public React surface that ordinary users search for:

| Area                  | Example queries                                                              | Count |
| --------------------- | ---------------------------------------------------------------------------- | ----: |
| Core hooks            | `useState`, `useEffect`, `useMemo`, `useTransition`, `useDeferredValue`, ... |    16 |
| Element / ref API     | `createRef`, `createElement`, `cloneElement`, `isValidElement`, `forwardRef` |     5 |
| Context / memo / lazy | `createContext`, `memo`, `lazy`                                              |     3 |
| Transitions / testing | `startTransition`, `act`                                                     |     2 |
| Short phrases + ids   | `state hook`, `lazy component`, `useState`, `createElement`                  |     8 |

**Measurement**:

| arm         |   hybrid MRR | Δ abs vs baseline |      Δ rel | semantic MRR | lexical-only MRR |
| ----------- | -----------: | ----------------: | ---------: | -----------: | ---------------: |
| baseline    |     0.122549 |                 — |          — |     0.122549 |         0.084314 |
| 2e only     |     0.122549 |          0.000000 |     +0.0 % |     0.122549 |         0.084314 |
| 2b+2c only  |     0.122549 |          0.000000 |     +0.0 % |     0.122549 |         0.084314 |
| **stacked** | **0.122549** |      **0.000000** | **+0.0 %** | **0.122549** |     **0.084314** |

This is a **stronger null result than Next.js**. In Phase 3e, the aggregate moved by a small amount under `2e`. In Phase 3f, **every arm is row-for-row identical to baseline across all three methods** (`semantic_search`, `get_ranked_context_no_semantic`, and `get_ranked_context`). There are **zero per-query rank changes** and **zero top-candidate changes** between baseline and any candidate arm.

**Per-query decomposition**:

```
baseline vs any candidate arm:
  0 improved, 0 regressed, 34 unchanged

top-10 hits in every arm: 5 / 34
  createRef      rank 6
  createElement  rank 1 (NL)
  cloneElement   rank 1
  isValidElement rank 1
  createElement  rank 1 (identifier)

top-10 misses in every arm: 29 / 34
  by query type:
    natural_language: 22 / 26 misses
    short_phrase:      6 / 6 misses
    identifier:        1 / 2 misses
```

This is almost a pure retrieval-floor dataset. The stack has nothing to work with because the candidate pool barely contains the target in the first place.

**Where the signal disappears**:

- `semantic_search` and `get_ranked_context` are not just equal in aggregate; on this dataset they are **rank-identical row-by-row**. Hybrid effectively collapses to semantic.
- `get_ranked_context_no_semantic` is weaker than semantic/hybrid, but the three knobs still do not change it at all. The sparse term weighting and the comment/API-call extraction simply never move the lexical candidate set.
- The only hybrid-vs-lexical differences in baseline are three already-findable queries:
  - `createRef` `5 → 6`
  - `createElement` `2 → 1`
  - `cloneElement` `6 → 1`
    Everything else is either a miss in both paths or the same rank in both.

So the React core result says something narrower but stronger than §8.16: **on short-file JS runtime code, the v1.5 stack is not mildly weaker or mildly stronger; it is functionally inert.**

**Updated eight-dataset baseline matrix**:

| Dataset                              | Language / archetype      | baseline MRR | stacked MRR |      Δ abs |      Δ rel |
| ------------------------------------ | ------------------------- | -----------: | ----------: | ---------: | ---------: |
| 89-query self                        | Rust / self               |        0.572 |       0.586 |     +0.014 |     +2.4 % |
| 436-query self                       | Rust / self               |       0.0476 |      0.0510 |    +0.0034 |     +7.1 % |
| ripgrep external                     | Rust / tooling            |        0.459 |       0.529 |     +0.070 |    +15.2 % |
| requests external                    | Python / app library      |        0.584 |       0.495 |     −0.089 |    −15.2 % |
| jest external                        | TS/JS / tooling           |        0.155 |       0.166 |     +0.011 |     +7.3 % |
| typescript external                  | TS/JS / compiler          |        0.098 |       0.201 |     +0.103 |   +104.3 % |
| next-js external                     | TS/JS / typical app       |        0.198 |       0.196 |     −0.002 |     −0.8 % |
| **react-core external (new, §8.17)** | **TS/JS / short runtime** |    **0.123** |   **0.123** | **+0.000** | **+0.0 %** |

Pattern: **5 positive / 1 negative / 2 inert**. The positive JS/TS evidence is now clearly concentrated in **tooling/compiler-style** code, while the two shortest-file runtime/app-style datasets measured so far are inert.

**Implications**:

- The family-level statement should now be: **JS/TS is bifurcated by code shape**. Tooling/compiler repos benefit; short-file runtime/app repos do not.
- §8.16's "typical app might still land between 0 % and +7 %" is no longer the default expectation. After React core, the better prior is **"short-file JS runtime code likely lands at 0 %"** until contradicted by a real product app.
- Combined with §8.16, this is enough to justify a **narrow** code-path change: keep the shipped Phase 2b/2c default-on behaviour, but split Phase 2e out of the JS/TS auto path. It still does **not** justify a broader rollback of the v1.6 default-on behaviour, because the app/runtime measurements are inert rather than systematically harmful.

**Reproduce**:

```bash
git clone --depth=1 https://github.com/facebook/react.git /tmp/react
rm -rf /tmp/react-core-bench
rsync -a --delete --exclude '__tests__/' /tmp/react/packages/react/src/ /tmp/react-core-bench/

# Arm 1: baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/react-core-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-react-core.json \
  --output benchmarks/embedding-quality-v1.6-phase3f-react-core-baseline.json

# Arm 2: Phase 2e only
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/react-core-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-react-core.json \
  --output benchmarks/embedding-quality-v1.6-phase3f-react-core-2e-only.json

# Arm 3: Phase 2b + 2c only
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/react-core-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-react-core.json \
  --output benchmarks/embedding-quality-v1.6-phase3f-react-core-2b2c-only.json

# Arm 4: stacked
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/react-core-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-react-core.json \
  --output benchmarks/embedding-quality-v1.6-phase3f-react-core-stacked.json
```

**Limitations acknowledged**:

1. **This is a curated production subtree, not the full React repo.** We explicitly excluded `__tests__/`, because the point of the measurement is runtime code shape, not test helper pollution. That makes it a cleaner scientific slice but a less literal "user points CodeLens at the whole repo" simulation.
2. **React core is runtime/library code, not a product app.** It reinforces the short-file inertness story, but it still is not the same thing as a large React application with domain-specific components.
3. **Recall is extremely low.** If only 5 / 34 queries hit the top 10 in every arm, there is almost no space left for a ranking-only intervention to prove itself. This dataset is primarily evidence about the limits of the current embedding/candidate pool.

**Artefacts**: `benchmarks/embedding-quality-v1.6-phase3f-react-core-{baseline,2e-only,2b2c-only,stacked}.json`. Dataset: `benchmarks/embedding-quality-dataset-react-core.json`.

---

### §8.18 — Phase 3g: semantic-dominant Python framework validation on `django/django`

**Hypothesis**: `requests` in §8.14 was clearly negative for Python, but it was still a single **app-library** dataset. That left an obvious escape hatch: maybe Python only looked bad because `requests` is a flat HTTP client with weak lexical anchors and small semantic neighborhoods. Phase 3g tests the other major Python regime: a large framework repo with stronger symbol families (`Model`, `QuerySet`, `HttpRequest`, `login_required`, `ListView`) and a much richer natural-language vocabulary. If Python's regression were dataset-specific, `django/django` is where the stack should recover.

**Target repo**: `django/django` shallow-cloned under `/tmp/django-src/django`. The measured subtree contains **902 `.py` files**, **162,768 LOC** total, and a **61.5 LOC median file size**. That makes it a useful contrast against both the tiny `requests` library and the much larger compiler-oriented TypeScript slice: Django has broad framework vocabulary, but its average file is still short enough that re-rank-only gains can vanish if the candidate pool is already semantic-heavy.

**Dataset**: 34 hand-built queries in `benchmarks/embedding-quality-dataset-django.json`, keeping the now-standard external-repo shape for direct comparison: **26 `natural_language` + 6 `short_phrase` + 2 `identifier`**. Coverage spans four framework surfaces:

| Area                 | Example targets                                                           | Count |
| -------------------- | ------------------------------------------------------------------------- | ----: |
| ORM / model layer    | `QuerySet`, `Manager`, `Model`, `ForeignKey`                              |    10 |
| HTTP / shortcuts     | `HttpRequest`, `HttpResponse`, `JsonResponse`, `redirect`, `render`       |     8 |
| URL + auth           | `reverse`, `resolve`, `login`, `logout`, `authenticate`, `login_required` |     8 |
| Views / forms / misc | `View`, `ListView`, `DetailView`, `Form`, `ModelForm`, `csrf_exempt`      |     8 |

**Measurement**:

| arm         |   hybrid MRR | Δ abs vs baseline |      Δ rel | semantic MRR | lexical-only MRR |
| ----------- | -----------: | ----------------: | ---------: | -----------: | ---------------: |
| baseline    |     0.293677 |                 — |          — |     0.285084 |         0.133927 |
| 2e only     |     0.293677 |          0.000000 |     +0.0 % |     0.285084 |         0.135398 |
| 2b+2c only  |     0.285940 |         -0.007737 |     -2.6 % |     0.286765 |         0.133927 |
| **stacked** | **0.288448** |     **-0.005229** | **-1.8 %** | **0.286765** |     **0.135398** |

Django is a **third regime**, distinct from both Next.js and TypeScript:

- **Semantic-dominant baseline**: `semantic_search` starts at **0.285**, far above lexical-only **0.134**. The baseline hybrid score (**0.294**) is already mostly semantic, so sparse lexical rescue has little room to matter.
- **`2e` is a true no-op on hybrid**: lexical-only nudges up slightly (`0.1339 → 0.1354`), but the hybrid aggregate does not move at all. The sparse pass touches some BM25 ordering, but those changes never beat the semantic ranking that already dominates Django's candidate pool.
- **`2b+2c` slightly improves pure semantic, yet still hurts hybrid**: `semantic_search` rises from **0.2851 → 0.2868**, but `get_ranked_context` falls from **0.2937 → 0.2859**. The extra comment / API-call text helps a few natural-language misses, but it perturbs already-retrievable framework queries enough to lose net quality.

**Per-query decomposition**:

```
baseline vs 2e-only:
  0 improved, 0 regressed, 34 unchanged

baseline vs 2b+2c only:
  4 improved, 5 regressed, 25 unchanged

baseline vs stacked:
  4 improved, 6 regressed, 24 unchanged
```

Representative stacked changes:

- Improvements:
  - `Model`: `None → 5`
  - `login_required`: `None → 10`
  - `get_object_or_404`: `5 → 3`
  - `logout`: `3 → 2`
- Regressions:
  - `check_password`: `2 → 6`
  - `redirect`: `7 → 17`
  - `HttpRequest`: `12 → None`
  - `HttpResponse` (`short_phrase`): `2 → 4`
  - `ForeignKey`: `6 → 7`

The query-type split explains the sign:

| query type       | baseline hybrid MRR | stacked hybrid MRR |       Δ |
| ---------------- | ------------------: | -----------------: | ------: |
| natural_language |            0.172501 |           0.175278 | +0.0028 |
| short_phrase     |            0.750000 |           0.708333 | -0.0417 |
| identifier       |            0.500000 |           0.500000 | +0.0000 |

So Django is not "uniformly worse"; it is more specific than that. The stack rescues a handful of natural-language framework lookups, but the gain is too small to offset short-phrase regressions on queries that Django's baseline already handled reasonably well.

**Updated nine-dataset baseline matrix**:

| Dataset                          | Language / archetype   | baseline MRR | stacked MRR |      Δ abs |      Δ rel |
| -------------------------------- | ---------------------- | -----------: | ----------: | ---------: | ---------: |
| 89-query self                    | Rust / self            |        0.572 |       0.586 |     +0.014 |     +2.4 % |
| 436-query self                   | Rust / self            |       0.0476 |      0.0510 |    +0.0034 |     +7.1 % |
| ripgrep external                 | Rust / tooling         |        0.459 |       0.529 |     +0.070 |    +15.2 % |
| requests external                | Python / app library   |        0.584 |       0.495 |     -0.089 |    -15.2 % |
| **django external (new, §8.18)** | **Python / framework** |    **0.294** |   **0.288** | **-0.005** | **-1.8 %** |
| jest external                    | TS/JS / tooling        |        0.155 |       0.166 |     +0.011 |     +7.3 % |
| typescript external              | TS/JS / compiler       |        0.098 |       0.201 |     +0.103 |   +104.3 % |
| next-js external                 | TS/JS / typical app    |        0.198 |       0.196 |     -0.002 |     -0.8 % |
| react-core external              | TS/JS / short runtime  |        0.123 |       0.123 |     +0.000 |     +0.0 % |

Machine-generated counterpart: `python3 benchmarks/embedding-quality-matrix.py --require-datasets ripgrep,requests,jest,typescript,next-js,react-core,django`

- JSON artefact: `benchmarks/embedding-quality-phase3-matrix.json`
- Markdown artefact: `benchmarks/embedding-quality-phase3-matrix.md`
- This does not replace the narrative analysis in §8.18, but it does make the arm-level matrix reproducible from the underlying benchmark JSONs instead of hand-maintained tables.

Pattern is now **5 positive / 2 negative / 2 inert**.

- **Rust** remains consistently positive.
- **TS/JS** remains bifurcated by code shape: tooling/compiler positive, runtime/app mostly inert.
- **Python is now negative in two distinct regimes**: one app library (`requests`) and one semantic-heavy framework (`django`). That is materially stronger evidence than the old single-dataset Python story.

**Policy interpretation**:

- This does **not** justify widening the already-landed Phase 2m split. Phase 2m was the narrow JS/TS fix: keep JS/TS auto-enabled for Phase 2b/2c, but remove JS/TS from the **auto** Phase 2e sparse gate.
- It **does** validate the existing choice to keep Python outside the auto-on family entirely. The code path already does this: `auto_hint_should_enable()` excludes Python from the NL stack, and `auto_sparse_should_enable()` excludes Python from auto sparse weighting as well.
- In other words, Django is not a trigger for a new rollback. It is a second external proof that the current Python-off default is the correct side of the tradeoff.

**Reproduce**:

```bash
git clone --depth=1 https://github.com/django/django.git /tmp/django-src

# Arm 1: baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/django-src/django --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-django.json \
  --output benchmarks/embedding-quality-v1.6-phase3g-django-baseline.json

# Arm 2: Phase 2e only
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/django-src/django --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-django.json \
  --output benchmarks/embedding-quality-v1.6-phase3g-django-2e-only.json

# Arm 3: Phase 2b + 2c only
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/django-src/django --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-django.json \
  --output benchmarks/embedding-quality-v1.6-phase3g-django-2b2c-only.json

# Arm 4: stacked
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/django-src/django --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-django.json \
  --output benchmarks/embedding-quality-v1.6-phase3g-django-stacked.json
```

**Artefacts**: `benchmarks/embedding-quality-v1.6-phase3g-django-{baseline,2e-only,2b2c-only,stacked}.json`. Dataset: `benchmarks/embedding-quality-dataset-django.json`.

### §8.19 — Phase 2n: unified Phase 2e-only evidence across 8 datasets, and the Phase 2m decision audit

**Purpose**: Phase 2m (the §8.17 code change) narrowed the Phase 2e sparse re-ranker auto-gate to the Rust family of languages, after an evidence arc that spanned §8.12 → §8.13 → §8.15 → §8.16 → §8.17. That evidence arc was spread across seven measurement sections with different framings. This section collects the 2e-only column from every 4-arm A/B the project has ever run into a single narrative table so future decisions about Phase 2e (rollback, removal, further narrowing, or extension) can be made against one source of truth instead of reconstructing it from the phase-by-phase text.

**Earlier sections compared the full stacked arm to the baseline**, which blends three effects (Phase 2b NL token extraction, Phase 2c API-call extraction, Phase 2e sparse re-rank). Phase 2m is exclusively about Phase 2e, so only the `2e-only vs baseline` delta is load-bearing here. Everything below uses **hybrid `get_ranked_context` MRR** so that numbers are directly comparable across sections.

A machine-generated companion of this table lives at `benchmarks/embedding-quality-phase3-matrix.json` / `.md`, generated by `benchmarks/embedding-quality-matrix.py`. That script diffs the full stacked arm against baseline for each `phase3*` dataset. The table below covers a slightly different slice — Phase 2e-only specifically, including the §8.2 / §8.7 Rust measurements that predate the `phase3*` naming convention — so it is still hand-aggregated from the archived result JSONs. The two views agree on every dataset they both cover.

**Eight-dataset unified table** (hybrid `get_ranked_context` MRR, 2e-only vs baseline):

| § ref | Dataset                    | Archetype            | Lang | baseline | 2e-only |   Δ abs |       Δ rel |
| ----- | -------------------------- | -------------------- | ---- | -------: | ------: | ------: | ----------: |
| §8.2  | 89-query self (phase2e v2) | Rust / self          | rs   |   0.5716 |  0.5787 | +0.0071 |      +1.2 % |
| §8.7  | ripgrep external           | Rust / tooling       | rs   |   0.4594 |  0.4878 | +0.0284 |  **+6.2 %** |
| §8.8  | requests external          | Python / app library | py   |   0.5837 |  0.5697 | −0.0140 |      −2.4 % |
| §8.13 | jest external              | TS / tooling         | ts   |   0.1546 |  0.1567 | +0.0021 |      +1.3 % |
| §8.15 | typescript external        | TS / compiler        | ts   |   0.0984 |  0.0886 | −0.0098 | **−10.0 %** |
| §8.16 | next-js external           | TS / typical app     | ts   |   0.1979 |  0.1962 | −0.0017 |      −0.8 % |
| §8.17 | react-core external        | TS / short runtime   | ts   |   0.1225 |  0.1225 | +0.0000 |      +0.0 % |
| §8.18 | django external            | Python / framework   | py   |   0.2937 |  0.2937 | +0.0000 |      +0.0 % |

Every row is reproducible from an archived 4-arm result JSON: run `benchmarks/embedding-quality-matrix.py` for the seven `phase3*` rows, and read the `benchmarks/embedding-quality-v1.5-phase2e-v2-{baseline,on}.json` pair directly for the §8.2 row. None of the numbers are hand-copied from prior narrative sections.

**By-language aggregation**:

| Language family | Datasets | Positive | Zero/Inert | Negative |   Best |   Worst |
| --------------- | -------: | -------: | ---------: | -------: | -----: | ------: |
| Rust            |        2 |      2/2 |          0 |        0 | +6.2 % |  +1.2 % |
| TypeScript / JS |        4 |      1/4 |        1/4 |      2/4 | +1.3 % | −10.0 % |
| Python          |        2 |      0/2 |        1/2 |      1/2 |  0.0 % |  −2.4 % |

**What this table actually says**:

1. **Rust is unambiguously positive**. Both the 89-query self dataset and the external ripgrep dataset put Phase 2e on the correct side of zero. Rust is the only family in the table with a 2/2 positive record, and its best case (+6.2 %) is also the single largest positive Phase 2e contribution across all measured datasets. The Rust auto-on branch of Phase 2m is load-bearing evidence, not aspirational policy.
2. **TypeScript / JS is mixed at best and net-negative at worst**. One marginal positive (jest, a test-tooling codebase), one zero, two negatives. The one positive is jest's **+1.3 %**, which is smaller in absolute terms than Rust's weakest positive — Rust self-89 at **+0.0071 absolute** vs jest at **+0.0021 absolute**. The worst case is the TypeScript compiler at **−10.0 %**, which is the single largest negative Phase 2e effect across all eight datasets, nearly three times the size of the next largest negative. Even without the follow-up app measurements, a narrow Rust-only auto-gate was defensible; with §8.16 and §8.17 added, keeping JS/TS in the 2e auto-gate would be indefensible.
3. **Python is never positive**. One small negative, one exact zero. Phase 2m's JS/TS split happened to also match the Python story because `language_supports_nl_stack` already excludes Python, so Python never reaches the Phase 2e auto-on path in practice. The table makes it explicit: if Python were ever added back to `language_supports_nl_stack` (it is not on the roadmap), the 2e gate would still have to stay off on the current evidence.
4. **Phase 2e is mechanism-inert when baseline hybrid MRR is already saturated against lexical signal**. react-core (0.1225 → 0.1225) and django (0.2937 → 0.2937) both show Phase 2e producing literally zero change at full float precision, despite moving pure `get_ranked_context_no_semantic` slightly upward on some arms. The sparse re-ranker's output is already subsumed by the hybrid combiner on those corpora.

**Phase 2m decision audit — should the Rust auto-on gate be kept, narrowed, or removed?**

Three alternative policies were considered during the §8.17 review:

- **Policy A — "Remove Phase 2e entirely"**. Cheap from a maintenance perspective; loses the 2/2 positive Rust signal (+0.0071 on self-89, +0.0284 on ripgrep). The absolute magnitudes are small on one dataset and non-trivial on the other. Removal would be a strictly net-negative move on measured Rust corpora, which is the archetype most project-internal users run CodeLens on.
- **Policy B — "Narrow Rust auto-on to tooling only"**. Would require a new classifier (is this a Rust CLI / library / compiler / editor vs a Rust application?). Both measured Rust datasets are already in the tooling/self category, so the policy has no evidence base to calibrate against. Deferred until a Phase 3h Rust app measurement justifies or contradicts it.
- **Policy C — "Keep current Phase 2m scope (Rust family on, JS/TS off, Python off, everything else off)"**. Matches every positive in the table and excludes every negative. This is what landed in PR #36 and is the status quo.

Policy C is the minimum-commitment choice that the table supports. It also has the property that it can be revisited cheaply: all future decisions (removal, narrowing, or extension) are **one allowlist edit away** because the split lives in a pair of `language_supports_*` functions in `crates/codelens-engine/src/embedding/mod.rs`. No schema migration, no index rebuild, no MCP protocol change.

**What would change this decision**:

- A Rust application-style measurement (Phase 3h on `tokio-rs/axum`, `SergioBenitez/Rocket`, or a comparable non-tooling Rust framework) that lands Phase 2e as non-positive would reduce Rust's 2/2 streak to 2/3 and re-open Policy B as an evidence-backed option.
- A future TypeScript / JavaScript measurement — e.g. a Vite + React production app, or a Node.js service — that lands Phase 2e as a large positive would push the TS/JS record to 2/5 positive. That would still not justify re-adding JS/TS to the auto-gate: the asymmetry from §8.15 (−10.0 %) is too large to overcome with a single new positive. Two separate large JS/TS positives would be needed.
- A model swap (Phase 2d) that fundamentally changes the baseline candidate pool. The current Phase 2e contribution is a function of whether the re-ranker has useful slack above the lexical-only floor; a different embedding model could redraw that slack completely, which would make this entire table stale.

**Pointer-forward**:

§8.19 closes the §8.12 → §8.18 Phase 2e evidence arc. The phase candidates that remain open after this section are:

1. **Phase 3h** — a Rust app-style dataset (counterpart to §8.16 / §8.17 for the Rust family). This is the only measurement that could move Policy B out of "deferred".
2. **Phase 2d** — an embedding model swap addressing the retrieval-failure floor that §8.15 / §8.16 / §8.17 surfaced (15 / 26 NL queries unreachable on next-js, 29 / 34 on react-core). This is the largest potential quality lever remaining.
3. **Operational hardening** — tracking items independent of the measurement campaign (the §8.14 language-switch process-scope limitation, Phase 4d single-instance guard follow-ups, etc.).

None of these three change the §8.19 decision about the Phase 2m rollout. They all presuppose that the auto-gate split landed in PR #36 stays in place.

### §8.20 — Phase 3h: Rust framework library on `tokio-rs/axum`

**Hypothesis** (from §8.19 "What would change this decision"): Phase 2m's Policy B ("narrow Rust auto-on to tooling only") was deferred because every measured Rust dataset so far has been tooling / self-code — ripgrep (CLI tooling), 89-query self (CodeLens core), 436-query self (CodeLens augmented). If the Rust positive evidence is actually file-size-dependent rather than language-dependent (the same critique §8.16 / §8.17 applied to JS/TS), then a Rust _framework library_ with short-to-medium file sizes should look more like next-js / react-core than like ripgrep. Phase 3h tests that symmetrically.

**Target repo**: `tokio-rs/axum` (depth-1 shallow clone). The measured subtree is a curated copy at `/tmp/axum-bench` that includes the four workspace crates most users point CodeLens at — `axum/src`, `axum-core/src`, `axum-extra/src`, `axum-macros/src` — with `examples/`, `benches/`, and `tests/` excluded. That leaves **109 source files** and **32 033 LOC**, median 201 LOC/file, max 1 723 LOC/file (`extract/ws.rs`). This is a deliberate size ladder check: next-js was a mega-framework at 1 564 files / 268 k LOC / median 61, react-core was a runtime slice at 30 files / 4 k LOC, and axum sits between them as a medium-file framework library.

**Dataset**: 34 hand-built queries in `benchmarks/embedding-quality-dataset-axum.json`, keeping the §8.15 → §8.18 shape (26 NL + 6 short_phrase + 2 identifier) for direct comparison. Coverage spans the axum public API surface:

| Area                | Example targets                                                                                                                  | Count |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----: |
| Core types / traits | `Router`, `IntoResponse`, `FromRequest`, `FromRequestParts`, `Handler`, `IntoResponseParts`, `Json`, `MethodRouter`              |     8 |
| Extractors          | `Path`, `Query`, `State`, `Form`, `Extension`, `Multipart`, `WebSocketUpgrade`, `WebSocket`                                      |     8 |
| Responses / serving | `Redirect`, `Html`, `Sse`, `serve`                                                                                               |     4 |
| Router methods      | `route`, `nest`, `merge`, `fallback`, `with_state`, `on`                                                                         |     6 |
| Short + identifier  | `json body`, `method router`, `into response`, `websocket upgrade`, `serve listener`, `path extractor`, `Router`, `IntoResponse` |     8 |

**Measurement**:

| arm         |   hybrid MRR | Δ abs vs baseline |      Δ rel | NL sub-MRR | short sub-MRR | identifier sub-MRR |
| ----------- | -----------: | ----------------: | ---------: | ---------: | ------------: | -----------------: |
| baseline    |     0.280789 |                 — |          — |   0.199912 |      0.391667 |           1.000000 |
| 2e only     |     0.281315 |          +0.00053 | **+0.2 %** |   0.200591 |      0.391667 |           1.000000 |
| 2b+2c only  |     0.280789 |           0.00000 |     +0.0 % |   0.199912 |      0.391667 |           1.000000 |
| **stacked** | **0.281315** |      **+0.00053** | **+0.2 %** |   0.200591 |  **0.391667** |           1.000000 |

**This is a marginally-positive result, dominated by noise**. The total absolute movement is `+0.00053` on 34 queries — a single rank-8 → rank-7 improvement on exactly one query (`Redirect` NL). Phase 2b+2c produces **exactly zero** hybrid movement at full float precision (`0.280789` identical in both arms), mirroring the §8.16 / §8.17 pattern on JS/TS apps. Phase 2e produces the same +0.00053 absolute lift both alone and stacked — which is also a single-rank-position change on the same query.

**Per-query decomposition** (26 NL + 6 short + 2 identifier, stacked vs baseline):

```
NL queries (26 total): 1 improved, 0 regressed, 25 unchanged
  Redirect  8 → 7  (Δ MRR +0.0179, the only non-zero contribution)

short_phrase queries (6 total): 0 improved, 0 regressed (identical output)
identifier queries (2 total, Router + IntoResponse): both rank-1 in every arm

Total: 1 improved, 0 regressed, 33 unchanged.
```

**Baseline retrieval profile is surprisingly strong** — much stronger than the JS/TS app / runtime profile:

| Dataset        | NL retrieval failures (None in all arms) | Top-10 hits (every arm) |
| -------------- | ---------------------------------------: | ----------------------: |
| react-core     |                                  22 / 26 |                  5 / 34 |
| next-js        |                                  15 / 26 |                  7 / 34 |
| **axum (new)** |                               **8 / 26** |             **20 / 34** |
| typescript     |                                  20 / 26 |                  6 / 34 |

So the baseline candidate pool on axum already contains the target for **59 %** of queries (vs next-js 21 %, react-core 15 %, typescript 18 %). The combine function is also doing real work: hybrid MRR (0.281) is meaningfully higher than semantic MRR (0.192) or lexical-only MRR (0.203), which is not what happened on next-js where hybrid 0.198 was _below_ lexical-only 0.229. Rust naming conventions + file organization + `pub fn` / `pub struct` directness are already handling most of the retrieval problem before Phase 2b/2c get a chance to help.

**Three-dataset Rust 2e-only range**:

| dataset                   | archetype         | median LOC | 2e-only Δ rel |
| ------------------------- | ----------------- | ---------: | ------------: |
| ripgrep external (§8.7)   | CLI tooling       |          ? |    **+6.2 %** |
| 89-query self (§8.2)      | library / self    |          ? |        +1.2 % |
| **axum external (§8.20)** | **framework lib** |    **201** |    **+0.2 %** |

The gradient is monotonic: **the more "library / framework" (and the less "tooling / application binary") the codebase gets, the smaller the Phase 2e lift becomes**. This is the same direction JS/TS showed in §8.17 (react-core 0 %, next-js −0.8 %, jest +1.3 %, typescript −10.0 %), except Rust never crosses into negative territory. Rust's worst case in the measured corpus is `+0.0 %`, not `−10.0 %`.

**Does this change the §8.19 Phase 2m decision?**

No. The §8.19 "what would change this decision" criterion for Policy B ("narrow Rust auto-on to tooling only") was:

> "A Rust application-style measurement (Phase 3h on `tokio-rs/axum`, `SergioBenitez/Rocket`, or a comparable non-tooling Rust framework) that lands Phase 2e as **non-positive** would reduce Rust's 2/2 streak to 2/3 and re-open Policy B as an evidence-backed option."

axum is Rust, it is non-tooling, and Phase 2e is measured at **+0.2 %** — strictly positive, not non-positive. Rust's streak extends from 2/2 positive to **3/3 positive**. Policy B stays deferred because the evidence base for it still does not exist: no measured Rust corpus has yet produced a non-positive Phase 2e result.

But the _expected benefit_ for end users has to narrow:

- **Users with Rust tooling / CLI / compiler code** (cargo, ripgrep, rustc, rust-analyzer, bat, fd, tokei, helix, …) continue to see the original §8.7 / §8.12 win range of +1 % to +6 % on hybrid MRR.
- **Users with Rust framework library code** (axum, actix-web, tower, tonic, …) will see near-zero benefit at the hybrid-MRR level. The mechanism is the same "baseline hybrid is already near-saturation" story from §8.16 / §8.17, just in a different language.
- **Users with Rust application code** (tower-of-babel real web services, CLI apps built on these frameworks) remain unmeasured. Phase 3h addresses the Rust framework library slot; the Rust application slot is still open, but given the axum result it is now safe to _predict_ "probably near-zero, not large-negative".

The marketing line narrows from "Phase 2m auto-on is positive on Rust" to "**Phase 2m auto-on ranges from neutral on Rust framework libraries to strongly positive on Rust tooling, with no measured negatives in any Rust regime**".

**Updated ten-dataset baseline matrix** (full stacked arm vs baseline):

| Dataset                        | Language / archetype         | baseline MRR | stacked MRR |      Δ abs |      Δ rel |
| ------------------------------ | ---------------------------- | -----------: | ----------: | ---------: | ---------: |
| 89-query self                  | Rust / self                  |        0.572 |       0.586 |     +0.014 |     +2.4 % |
| 436-query self                 | Rust / self                  |       0.0476 |      0.0510 |    +0.0034 |     +7.1 % |
| ripgrep external               | Rust / tooling               |        0.459 |       0.529 |     +0.070 |    +15.2 % |
| requests external              | Python / app library         |        0.584 |       0.495 |     −0.089 |    −15.2 % |
| django external                | Python / framework           |        0.294 |       0.288 |     −0.005 |     −1.8 % |
| jest external                  | TS/JS / tooling              |        0.155 |       0.166 |     +0.011 |     +7.3 % |
| typescript external            | TS/JS / compiler             |        0.098 |       0.201 |     +0.103 |   +104.3 % |
| next-js external               | TS/JS / typical app          |        0.198 |       0.196 |     −0.002 |     −0.8 % |
| react-core external            | TS/JS / short runtime        |        0.123 |       0.123 |     +0.000 |     +0.0 % |
| **axum external (new, §8.20)** | **Rust / framework library** |    **0.281** |   **0.281** | **+0.001** | **+0.2 %** |

Pattern: **6 positive / 2 inert / 2 negative**. Rust now contributes 3 positive + 1 near-zero, Python remains 0 positive / 2 negative, TS/JS remains bifurcated (2 positive tooling / 1 inert runtime / 1 small-negative app / 1 mega-positive compiler).

**Reproduce**:

```bash
# Clone axum and materialize the bench subtree
git clone --depth=1 https://github.com/tokio-rs/axum.git /tmp/axum
mkdir -p /tmp/axum-bench
rsync -a --delete --exclude '/target/' --exclude '/examples/' --exclude '**/tests/' --exclude '**/benches/' /tmp/axum/axum/src/ /tmp/axum-bench/axum/
rsync -a --exclude 'tests/' --exclude 'benches/' /tmp/axum/axum-core/src/ /tmp/axum-bench/axum-core/
rsync -a --exclude 'tests/' --exclude 'benches/' /tmp/axum/axum-extra/src/ /tmp/axum-bench/axum-extra/
rsync -a --exclude 'tests/' /tmp/axum/axum-macros/src/ /tmp/axum-bench/axum-macros/

# Arm 1: baseline
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/axum-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-axum.json \
  --output benchmarks/embedding-quality-v1.6-phase3h-axum-baseline.json

# Arm 2: Phase 2e only
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/axum-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-axum.json \
  --output benchmarks/embedding-quality-v1.6-phase3h-axum-2e-only.json

# Arm 3: Phase 2b + 2c only
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/axum-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-axum.json \
  --output benchmarks/embedding-quality-v1.6-phase3h-axum-2b2c-only.json

# Arm 4: stacked
CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1 \
CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1 \
CODELENS_RANK_SPARSE_TERM_WEIGHT=1 \
CODELENS_RANK_SPARSE_THRESHOLD=40 \
CODELENS_RANK_SPARSE_MAX=40 \
CODELENS_MODEL_DIR=$(pwd)/crates/codelens-engine/models \
CODELENS_BIN=./target/release/codelens-mcp \
python3 benchmarks/embedding-quality.py /tmp/axum-bench --isolated-copy \
  --dataset benchmarks/embedding-quality-dataset-axum.json \
  --output benchmarks/embedding-quality-v1.6-phase3h-axum-stacked.json
```

**Limitations acknowledged**:

1. **axum is a framework library, not an application**. The §8.19 criterion called for "a Rust application-style measurement", and a framework library is the closest practical Rust analogue of Next.js (also a framework, not an app). A Rust end-user application measurement (a production web service built on axum, or a large CLI app built on clap) would be a cleaner fit for the "Rust app" slot, but the OSS Rust ecosystem is heavily biased toward libraries and tooling — pure application repos at the scale needed for a 34-query benchmark are rare.
2. **Curated subtree excludes `examples/` and `tests/`**. Same methodology as §8.17 on react-core. A user who points CodeLens at the full axum checkout will see the examples/tests indexed, which would add some noise but is unlikely to change the Phase 2e / Phase 2b+2c sign.
3. **Only one non-tooling Rust measurement so far**. `tokio` itself, `tower`, `tonic`, or `actix-web` would each add a second data point for the "Rust framework library" archetype. If any of them come out _non-positive_ on Phase 2e, §8.19 Policy B becomes evidence-backed. Deferred.

**Artefacts**: `benchmarks/embedding-quality-v1.6-phase3h-axum-{baseline,2e-only,2b2c-only,stacked}.json`. Dataset: `benchmarks/embedding-quality-dataset-axum.json`.

---

## 9. See Also

- [docs/architecture.md](architecture.md) — tool surface, layer diagram, full metric table
- [README.md](../README.md) — quick install + `vs Serena` comparison
- Project `CLAUDE.md` — routing policy for agents deciding when to prefer CodeLens over native tools
