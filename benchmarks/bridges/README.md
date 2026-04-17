# Per-repo bridges overrides

One JSON file per external repo slug. Each file is an array of
`{"nl": "...", "code": "..."}` entries appended to the generic bridge
table when the 3-arm runner (`benchmarks/external-3arm.py`) is invoked
with `arm=repo-on`.

Example (`benchmarks/bridges/axum.bridges.json`):

```json
[
  { "nl": "http handler", "code": "handler Router route" },
  { "nl": "extract query", "code": "Query extractor" },
  { "nl": "middleware layer", "code": "Service layer tower" }
]
```

## When to write one

Only when the repo's query set contains natural-language phrases that
match the repo's vocabulary poorly. The generic table already covers
language-agnostic terms (categorize, rename, search, callers, etc.).
Repo-specific overrides should capture framework/library jargon the
repo uses.

## What not to write

- Terms already in `GENERIC_BRIDGES` (crates/codelens-mcp/src/tools/query_analysis.rs).
- Symbol-identifier aliases — those belong in the dataset's
  `expected_symbol`, not the bridge table.
- Project-specific jargon that does not appear in any query. The bridge
  table is applied per-query; dead entries are wasted tokens at runtime.

## How to measure a proposed override

1. Add `{slug}.bridges.json` here.
2. Run `benchmarks/external-3arm.py` locally against a prepared
   `external-repos/{slug}/` worktree.
3. Compare the `generic-on` vs `repo-on` hybrid MRR columns in the
   resulting matrix. If `repo-on` is not strictly higher, the override
   is not paying for itself — delete it.
