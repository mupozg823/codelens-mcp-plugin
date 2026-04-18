# BM25 Sparse Lane — Operational Spec

Status: active
Date: 2026-04-18
Owners: retrieval (sparse lane)

## Role

BM25 is **not** a semantic model. It is a lexical first-stage retriever
that scores token overlap between query and document with the classic
probabilistic form: rare tokens weigh more (idf), repeated tokens
saturate (tf with `k1=1.2`), and long documents get a mild length
penalty (`b=0.75`). These are the same defaults Lucene and
Elasticsearch still ship today.

In CodeLens, BM25 is the **common sparse substrate** across three
corpora (code symbols, rules/memory, future: logs/telemetry). Its job
is to cut a candidate list fast and structured — not to explain intent,
not to reason about blast radius, not to judge mutation safety.

### Well-suited queries

- Exact symbol names, path tokens, module names
- Signature fragments (`fn foo(&self, arg: Bar)`), error strings,
  config keys
- Short lexical phrases (2-4 tokens)

### Poorly-suited queries

- Long natural-language intent ("why is this refactor risky")
- Architecture-level semantic similarity
- Cross-file behavior inference, paraphrase, alias resolution

These belong to the dense lane (`get_ranked_context`) or to graph /
mutation-safety workflows.

## Query Class → Retriever → Output Card → Follow-up

| Query class                  | Example                                     | First-stage retriever                     | Output card fields                                          | Typical follow-up tools                             |
| ---------------------------- | ------------------------------------------- | ----------------------------------------- | ----------------------------------------------------------- | --------------------------------------------------- |
| identifier                   | `dispatch_tool`, `SymbolIndex`              | `bm25_symbol_search`                      | `symbol_id`, `name`, `name_path`, `signature`, `file_path`  | `find_symbol` (body) → `get_file_diagnostics`       |
| path / module                | `tools/symbols/handlers`, `src/dispatch`    | `bm25_symbol_search`                      | same + `module_path`, `flags.exported`                      | `get_symbols_overview` → `find_referencing_symbols` |
| short phrase                 | `mutation gate`, `rename symbol`            | `bm25_symbol_search`                      | same + `why_matched`                                        | `analyze_change_request` → `impact_report`          |
| signature fragment           | `fn rename(..., new_name: &str)`            | `bm25_symbol_search`                      | emphasises `signature` field hits                           | `find_symbol` → `plan_symbol_rename`                |
| error / log string           | `"failed to bind port"`                     | `bm25_symbol_search` (log corpus, future) | snippet + file + line                                       | `search_for_pattern` (narrow) → `find_symbol`       |
| rule / policy query          | `"verify_change_readiness before mutation"` | `find_relevant_rules`                     | `source_file`, `frontmatter_name`, `section_title`, preview | Read the pointed memory/CLAUDE.md                   |
| long natural-language intent | `"how does dispatch work end-to-end"`       | `get_ranked_context` (dense/hybrid)       | ranked symbols with `relevance_score`, body on request      | `trace_request_path` → `get_impact_analysis`        |
| architectural / impact       | `"what breaks if I change X"`               | `get_impact_analysis` / `impact_report`   | blast radius, coupling                                      | `review_architecture`, `verify_change_readiness`    |

The router is `analyze_retrieval_query`. It emits
`prefer_sparse_symbol_search` for the first four rows and natural-
language / path hints for the others. Harnesses read the hint off
`retrieval.preferred_lane` (exposed by `get_ranked_context` responses)
and pick accordingly.

## Corpus Separation

Same BM25 engine, three disjoint corpora. Mixing them pollutes IDF —
rare-in-code tokens become common-in-rules, and relevance collapses.

| Corpus           | Built by                             | Unit             | Loader                                                            |
| ---------------- | ------------------------------------ | ---------------- | ----------------------------------------------------------------- |
| Code symbols     | `symbol_corpus::build_symbol_corpus` | Symbol           | `SymbolIndex::indexed_file_paths` + `get_symbols_overview_cached` |
| Rules / memory   | `rule_corpus::load_rule_corpus`      | Markdown section | CLAUDE.md, project memory, global policy                          |
| Logs / telemetry | (deferred — R.7+)                    | Log line / entry | To be defined                                                     |

Each corpus ships its own scorer module (`symbol_retrieval`,
`rule_retrieval`) so field weights stay corpus-specific. Do not share
field weight constants across corpora.

## Output Card Contract

Frontier-model harnesses (Claude, Codex) select their next tool off
the card, so the fields are part of the contract and cannot drift
silently.

Required, per result:

- `symbol_id` / `source_file` — stable handle
- `name` / `section_title` — short human-readable label
- `kind` / `source_kind` — helps the model route
- `score` — monotonic within a response, rounded to 3dp
- `why_matched` — the unique query terms that actually scored.
  Without this, the model guesses what the retriever "meant".
- `provenance.source` — `"sparse_bm25f"` or `"rules_bm25"`; explicit
  lane attribution
- `provenance.retrieval_rank` — 1-based position in this response

Symbol-specific fields: `name_path`, `signature`, `file_path`,
`module_path`, `language`, `line`, `flags.{is_test, is_generated,
exported}`.

Rule-specific fields: `frontmatter_name`, `line_start`,
`content_preview` (≤400 chars).

**Never return full file body in a first-stage response.** Body is a
second-stage fetch via `find_symbol` (include_body=true), paid only
for the 1-3 cards the caller actually consumes.

## Two-stage Packing

First stage (cheap):

1. Caller hits `bm25_symbol_search` (or `find_relevant_rules`) with
   `max_results=10..30`.
2. CodeLens returns structured cards — identifiers, paths, scores,
   flags — and nothing heavier.

Second stage (only for consumed cards):

3. Caller opens 1-3 cards via `find_symbol` / `get_symbols_overview`
   / graph tools.
4. Diagnostic / mutation gates (`get_file_diagnostics`,
   `verify_change_readiness`, `safe_rename_report`) sit after stage 2
   — never before stage 1.

This keeps the expensive body reads off the retrieval hot path.

## Do / Don't

**Do:**

- Route queries per the matrix above.
- Keep corpora separate. Mix only at the caller's decision point,
  never at the indexer.
- Always include `why_matched` + `provenance.source` on cards.
- Oversample inside the retriever (3× `max_results`) then truncate
  after adjustments (test/generated downweight, exported boost,
  session recency boost).

**Don't:**

- Use BM25 as the default ranker for long NL queries.
- Always-on fuse BM25 with dense ranking. That's been measured as a
  regression (`retrieval-regression-bisect-2026-04-17.md`). Selective
  routing is the safer default, consistent with Repoformer-style
  "selective retrieval".
- Ship body text in first-stage responses.
- Share field-weight constants across corpora.
- Treat BM25 output as a substitute for graph / mutation-safety
  workflows.

## Measurement (updated 2026-04-18 evening)

### Baseline run

Dataset: `benchmarks/embedding-quality-dataset-self.json` (n=104, Rust-heavy, v1.9.46). Archived as `benchmarks/results/v1.9.46-bm25-vs-hybrid-self.json`.

| Method                           |       MRR | Acc@1 | identifier (n=31) | natural_language (n=62) | short_phrase (n=11) | avg ms |
| -------------------------------- | --------: | ----: | ----------------: | ----------------------: | ------------------: | -----: |
| `get_ranked_context` (hybrid)    | **0.681** | 64.4% |         **0.935** |               **0.575** |           **0.561** |    123 |
| `semantic_search`                |     0.633 | 58.7% |             0.919 |                   0.519 |               0.470 |    552 |
| `bm25_symbol_search`             |     0.607 | 56.7% |             0.935 |                   0.487 |               0.364 |    102 |
| `get_ranked_context_no_semantic` |     0.527 | 49.0% |             0.935 |                   0.371 |               0.258 |     48 |

### Findings

1. **No identifier-class win.** `bm25_symbol_search` ties `get_ranked_context` at 0.935 on identifier queries. The spec's headline claim — "BM25 wins on identifiers" — is **not supported** by this dataset.
2. **Net regression overall** (−0.074 MRR vs hybrid).
3. **Significant loss on natural_language** (−15.3%) and **short_phrase** (−35.1%). Routing these to the sparse lane would strictly degrade quality.
4. **Overlap with existing lexical-only mode.** On identifier queries `bm25_symbol_search` (0.935) and `get_ranked_context_no_semantic` (0.935) are identical; the sparse lane adds no retrieval signal beyond what the existing lexical path already provides, and is ~2× slower (102 vs 48 ms).

### Routing policy (binding)

Given the measurement above:

- **Do NOT wire `bm25_symbol_search` into the default `get_ranked_context` / `analyze_change_request` router.** Any such wiring regresses NL and short_phrase queries on this dataset.
- `bm25_symbol_search` remains available as an **explicit opt-in tool** — debugging / inspection / dense-disabled builds / future labeled benchmarks.
- Before this policy changes, a new measurement must show **a strict identifier-class MRR win over `get_ranked_context` on at least one labeled dataset** — not a tie, a win. And must show **no NL / short_phrase regression**.

### Going forward

Before any field-weight or policy change lands:

- Run `benchmarks/embedding-quality.py` with the self dataset on + off
  the change. Compare MRR and recall@5.
- Log per-query class recall separately. A net-neutral MRR can still
  regress identifier queries if NL queries compensate.
- Keep the change behind a default-off flag until the numbers are on
  file.

This mirrors the Phase 2c/2b stacking lesson already captured in
`project_phase2c_stacking.md` — marginal extractors only earn a
default-on slot after A/B evidence, and only per language where it
was measured.

## Future — Learned Sparse

SPLADE v2 and 2026-03 SPLADE-Code are the next step above BM25, not a
replacement. They inherit the "sparse first-stage" role with neural
term weighting. Gate them on two conditions:

1. Labeled code-search benchmark in this repo beyond `self` — shipping
   BM25 without labels is fine; shipping learned sparse without labels
   is over-investment.
2. An inference cost budget. BM25 is ~µs/query; learned sparse is
   ms/query. The harness needs to tolerate the new latency.

Until both conditions hold, learned sparse stays parked.

## References

- Lucene `BM25Similarity` — the reference implementation and the source
  of the `k1=1.2 / b=0.75` defaults.
- Robertson & Zaragoza, _The Probabilistic Relevance Framework: BM25
  and Beyond_, 2009.
- Repoformer (2024-03) — selective retrieval for code.
- SPLADE v2 (2021-09) — learned sparse.
- SPLADE-Code (2026-03) — learned sparse for code retrieval.
