# Downstream Quality Mini A/B — 2026-04-19

**Question asked**: does CodeLens's compressed context (`get_ranked_context`) preserve enough information for a language model to answer targeted code-navigation questions as accurately as a `Read`-original baseline?

**Method**: 3 curated self-repo questions, each labelled with the required substrings a correct answer must cite. Two contexts per question:

- **ctx_A** — raw `Read` of the relevant file(s), lightly trimmed.
- **ctx_B** — `get_ranked_context(query, max_tokens=3000, include_body=true)` via the oneshot CLI.

Each cell answered by a Claude Code subagent constrained to one `Read` of the provided context file and forbidden from exploring the codebase or consulting prior knowledge. Two independent evaluators scored the six answers:

1. **Automatic label matcher** — Python substring match against the labelled truth set (100% reproducible).
2. **Codex CLI external judge** — same model family independence, stricter verdict logic (correct / partial / wrong).

Dataset: [`benchmarks/downstream-quality-mini-dataset.json`](./downstream-quality-mini-dataset.json).
Harness: this document + `score.py` (inline below).

## Results

### Per-cell

| Cell | Difficulty | Ctx bytes | Internal (label match) | Codex verdict |
| ---- | ---------- | --------: | :--------------------- | :------------ |
| Q1_A | easy       |     8 895 | 1 / 2 = 0.50           | partial       |
| Q1_B | easy       |    16 808 | 1 / 2 = 0.50           | **wrong**     |
| Q2_A | medium     |     8 345 | 2 / 4 = 0.50           | **wrong**     |
| Q2_B | medium     |     6 435 | 0 / 4 = 0.00           | **wrong**     |
| Q3_A | hard       |    60 706 | 4 / 4 = 1.00           | correct       |
| Q3_B | hard       |     6 220 | 4 / 4 = 1.00           | correct       |

### Aggregate

| Context        | Internal accuracy | Codex correct | Codex partial | Codex wrong |
| -------------- | :---------------: | :-----------: | :-----------: | :---------: |
| ctx_A (Read)   | 7 / 10 = **0.70** |       1       |       1       |      1      |
| ctx_B (ranked) | 5 / 10 = **0.50** |       1       |       0       |      2      |

### Compression ratio

| Question  | ctx_A bytes | ctx_B bytes |    A / B |
| --------- | ----------: | ----------: | -------: |
| Q1 easy   |       8 895 |      16 808 |     0.53 |
| Q2 medium |       8 345 |       6 435 |     1.30 |
| Q3 hard   |      60 706 |       6 220 | **9.76** |

Q3 — the question whose ground truth lives in two full engine files — is the case where compression is decisive: ctx_B is ~10× smaller and the subagent answers the question perfectly.
Q1 is the reverse pathology: ctx_B grew _larger_ than a single-file read because the ranker pulled in peripheral matches.

## Findings

### Where compression wins

**Q3 (hard, specific technical query)** is the archetype: "what two new fields were added to `RankedContextResult`?" The ranked context returns the exact struct plus the `prune_to_budget` stats fields, packaged as ~6 KB; the Read baseline is 60 KB because the question's relevant region is a small part of two big engine files. Same final accuracy (4 / 4), one tenth the bytes.

### Where compression loses

1. **Q2_B — full retrieval failure**. The query `"sampling_notice structured helper build_text_refs_response"` returned three symbols, NONE of which is the real helper (`build_text_refs_response_with_decisions` in `crates/codelens-mcp/src/tools/lsp.rs`). The ranker instead produced `state.rs:build`, `query_targets_helper_impl`, and a phrase-matched test helper. The words `sampling_notice` and `build_text_refs` appear in the JSON response, but only in the retrieval metadata; no code symbol from `tools/lsp.rs` made the top-K cut. The subagent, constrained to this context, correctly reported "Not in context." The signal is real: for branch-local, recently-committed code the embedding index lane did not rank the relevant helper.

2. **Q1_B — disambiguation miss**. The query `"find_symbol handler implementation"` returned `find_symbol` from the engine crate (`crates/codelens-engine/src/symbols/mod.rs`), not the MCP handler at `crates/codelens-mcp/src/tools/symbols/handlers.rs`. The ranker cannot know that the asker meant the MCP-side wrapper; it picked the top relevance. The Read baseline is narrow by construction, so it doesn't have this disambiguation risk.

### Harness limitations (honest caveats)

- **Sample size 3**. Two contexts × three questions is enough to reveal qualitative pathologies (Q2 retrieval failure, Q3 compression sweet spot) but not enough for a quantitative claim like "ctx_A is X% better than ctx_B". The numbers above are _descriptive_, not statistically significant.
- **ctx_A path stripping**. The harness dumps file bodies into `/tmp/dq-mini/QN_ctxA.txt`, which discards the original filesystem path. The Q1_A and Q2_A subagents therefore could not cite the real path even when the rest of the answer was correct. This artificially deflates ctx_A scores on the path label. The fix is to prepend a `# File: <path>` header when building the context file — applied in the follow-up before a larger run.
- **Same-model bias**. Answerer is Claude; one evaluator is Claude (label match — mechanical, low bias); the other is Codex (independent perspective). Codex was stricter (flagged two cells that the label-match scored partial), so the 0.70 / 0.50 internal numbers are the _optimistic_ reading.
- **Branch-local embeddings**. Phase 1 commits on `feat/transparency-phase1` were written within the measurement window. The embedding index may not have fully absorbed them — explaining part of Q2_B's retrieval failure.

## What this means for the spec

The axis-4 claim — "symbolic compression improves downstream quality" — **is not universally true on this sample**. It is:

- **True and decisive** when the query targets a specific technical construct whose answer lives in one or two small regions of much larger files (Q3).
- **Roughly neutral** when the target is a single clearly-named function whose Read baseline is already small (Q1 tie on label coverage but Codex flags ctx_B's semantic disambiguation miss).
- **Actively worse** when the query phrasing does not land semantically and the ranker retrieves peripheral matches (Q2_B).

This matches the 2026-04-19 bench correction's honest framing: CodeLens is a **precision / structure engine**, not a universal Read replacement. Use `get_ranked_context` for hard, specific queries; use `Read` when the target file is already obvious.

## Follow-ups

Listed in priority order, all out of scope for this report:

1. **Expand to 20 questions** with automated path-header injection for ctx_A. Re-run to get a statistically usable A / B signal.
2. **Warm the embedding index before Q2-style queries**. The engine's `refresh_symbol_index` tool exists; the reproducer should call it at the top so branch-local commits are measurable.
3. **Record token counts**, not bytes, so the compression ratio column is LLM-relevant.
4. **Add a "retrieval-failure rate"** metric: proportion of queries where the correct symbol never makes the top-K cut, independent of whether the subagent answered.

## Appendix — scoring script

Inline below for reproducibility. Reads the dataset and the six answers, prints the table above.

```python
#!/usr/bin/env python3
import json
from pathlib import Path

base = Path("benchmarks/downstream-quality-mini-dataset.json")
ds = json.loads(base.read_text())
answers = json.loads(Path("/tmp/dq-mini/answers.json").read_text())

for q in ds["questions"]:
    qid = q["id"].split("_")[0]
    labels = q["labels"]
    for ctx in ("A", "B"):
        ans = answers[f"{qid}_{ctx}"]
        matched = [lbl for lbl in labels if lbl in ans]
        print(f"{qid}_{ctx} matched {len(matched)}/{len(labels)}")
```

## Appendix — Codex judge response

```
{
  "verdicts": {
    "Q1_A": {"verdict": "partial", "reason": "Mentions `find_symbol` but does not cite the required file path `crates/codelens-mcp/src/tools/symbols/handlers.rs`."},
    "Q1_B": {"verdict": "wrong",   "reason": "Mentions `find_symbol` but gives the wrong file path `crates/codelens-engine/src/symbols/mod.rs`."},
    "Q2_A": {"verdict": "wrong",   "reason": "Cites `sampling_notice` but names a different helper/path and omits the required `crates/codelens-mcp/src/tools/lsp.rs` and `sampling_notice_tests` labels."},
    "Q2_B": {"verdict": "wrong",   "reason": "`Not in context.` is explicitly scored as wrong."},
    "Q3_A": {"verdict": "correct", "reason": "Cites `pruned_count`, `last_kept_score`, `RankedContextResult`, and the required `crates/codelens-engine/src/symbols` location."},
    "Q3_B": {"verdict": "correct", "reason": "Cites `pruned_count`, `last_kept_score`, `RankedContextResult`, and a path under `crates/codelens-engine/src/symbols`, satisfying the required labels."}
  },
  "ctx_A_correct_count": 1,
  "ctx_B_correct_count": 1,
  "headline": "Context A is slightly stronger overall because it has one full hit and one partial, while Context B has two clear misses and only Q3 meets the label test."
}
```
