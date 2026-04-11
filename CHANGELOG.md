# Changelog

All notable changes to **CodeLens MCP** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
