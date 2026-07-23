---
id: explore-impact
title: Explore and Impact
risk: read
implicit: true
description: >
  Onboard onto an unfamiliar repository or measure the blast radius of a planned
  change using the CodeLens index. Use for structure mapping, symbol/reference
  tracing, and "what breaks if I change X". Not for single-file reads or plain
  text/string audits — use native tools for those.
tools:
  - prepare_harness_session
  - overview
  - search
  - graph
  - get_ranked_context
  - find_symbol
---

# Explore & Impact

1. **Bind first.** `prepare_harness_session(project=<abs repo root>)` — verify
   `activated: true` and index health before any query; if the index is stale beyond
   the reported threshold, trigger refresh and say so.
2. **Map before drilling.** `overview` for the area of interest, then `search`
   (mode=symbol|refs) to locate anchors. Prefer one ranked query over many greps.
3. **Trace structure, not text.** `graph` (callers/callees/trace/impact) for
   relationships; `find_symbol`/`get_ranked_context` for bounded evidence packs.
   Batch symbol lookups in one array call where supported.
4. **Report with provenance.** Every claim carries file:line plus the index
   generation/freshness from the response envelope. If CodeLens fails, fall back to
   native tools and label the evidence as unindexed.
