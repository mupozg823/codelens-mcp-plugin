# Symbol-query seam (`tools/symbol_query/`)

> Moved verbatim from `CLAUDE.md` on 2026-09-16 to keep the always-on file small. This is the
> authoritative description of the seam; update it when the module layout changes.

## Symbol-query path lives behind one seam

`get_ranked_context`, `find_symbol`, and `get_symbols_overview` all dispatch through a single deep module: `crates/codelens-mcp/src/tools/symbol_query/`. The three tool entry points are renamed re-exports in `tools/symbols/mod.rs` (`run_find_symbol as find_symbol`, `run_ranked_context as get_ranked_context`, `run_symbols_overview as get_symbols_overview`). The orchestration body (query analysis → retrieval → rank fusion → SCIP enrichment → payload shaping) lives **inside** the pipeline module, not in `symbols/`.

Module layout (post-PR-F/G/H):

```
crates/codelens-mcp/src/tools/
├── semantic_retriever.rs           ← cross-cutting (pipeline + impact reports)
├── symbol_query/
│   ├── mod.rs                       ← SymbolQueryPipeline + SymbolQueryRequest
│   ├── find_symbol.rs               ← stage body for find_symbol
│   ├── ranked_context.rs            ← stage body for get_ranked_context
│   ├── symbols_overview.rs          ← stage body for get_symbols_overview
│   ├── sparse_retriever.rs          ← BM25F + context-window-adaptive budget + flatten_symbols
│   └── rank_fusion.rs               ← stage-4 helpers (5 fn + RankFusionPolicy, all pub(super))
└── symbols/
    ├── mod.rs                       ← renamed re-exports of the 3 pipeline entry fns
    ├── bm25_search.rs               ← bm25_symbol_search + suggested_follow_up + confidence_tier
    ├── fuzzy_search.rs              ← search_symbols_fuzzy (hybrid + semantic boost)
    ├── inventory.rs                 ← refresh_symbol_index + get_complexity + get_project_structure
    ├── formatter.rs                 ← compact_symbol_bodies (used by pipeline)
    └── analyzer.rs                  ← semantic_scores_for_query
```

When changing symbol-query semantics:
- Body of `run_ranked_context` / `run_find_symbol` / `run_symbols_overview` is in `tools/symbol_query/<tool>.rs`.
- Cross-cutting retrieval seams owned by the pipeline:
  - `tools/semantic_retriever.rs` (dense ONNX semantic results) — used by the pipeline **and** the impact-report family.
  - `tools/symbol_query/sparse_retriever.rs` (BM25F sparse hits, context-window-adaptive budget, `flatten_symbols` utility) — used by the pipeline **and** `symbols::{bm25_search, inventory}`.
- Rank-fusion stage (PR-H): the 5 helpers + `RankFusionPolicy` are `pub(super)` in `symbol_query/rank_fusion.rs`. `ranked_context.rs` is the only legitimate caller — the seam exists so the pipeline owns stage-4 entirely. Do not export rank-fusion items out of `symbol_query/`.
- Other stage helpers (SCIP signature/body slicing in `find_symbol.rs`, body Jaccard, query analysis) are file-private inside their `symbol_query/<tool>.rs` — do not promote to `pub(super)` casually.

Dependency direction is one-way: `symbols::*` → `symbol_query::*`. Never reach upward from the pipeline back into `symbols::*` — that was the cycle PR-F removed (`review_architecture` reported a 3-node loop `mod.rs → ranked_context.rs → handlers.rs`). If new sparse/retrieval helpers are needed, add them to `symbol_query/sparse_retriever.rs` (or a sibling sub-module).
