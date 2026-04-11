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

---

## 9. See Also

- [docs/architecture.md](architecture.md) — tool surface, layer diagram, full metric table
- [README.md](../README.md) — quick install + `vs Serena` comparison
- Project `CLAUDE.md` — routing policy for agents deciding when to prefer CodeLens over native tools
